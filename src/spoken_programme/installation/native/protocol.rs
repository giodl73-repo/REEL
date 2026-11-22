//! Strict private session messages. Observations deserialize, capabilities do
//! not: no message can construct Transport, Prepared or RestartPermit.
use super::{
    wire::{Direction, MAX_PAYLOAD_BYTES, Packet},
    *,
};
use serde::de::DeserializeOwned;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    Authorize {
        request_sha256: String,
        runtime_sha256: String,
    },
    TextShown {
        transcript_sha256: String,
        columns: u16,
        rows: u16,
        input_code_page: u32,
        output_code_page: u32,
    },
    Control {
        id: u64,
        command: Command,
        received_counter: i64,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preparation {
    Checking,
    AwaitingText,
    Fault,
    Stopped,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Output {
    Preparing {
        phase: Preparation,
        observed_counter: i64,
    },
    PreparationAck {
        id: u64,
        phase: Preparation,
        refusal: Option<TransportError>,
        observed_counter: i64,
    },
    PreparationEnded {
        phase: Preparation,
        observed_counter: i64,
    },
    Hello {
        request_sha256: String,
        runtime_sha256: String,
        process_id: u32,
    },
    Transcript {
        text: String,
        sha256: String,
    },
    Ready {
        snapshot: Snapshot,
        observed_counter: i64,
    },
    State {
        snapshot: Snapshot,
        observed_counter: i64,
    },
    Ack {
        id: u64,
        snapshot: Snapshot,
        refusal: Option<TransportError>,
        observed_counter: i64,
    },
    Ended {
        snapshot: Snapshot,
        observed_counter: i64,
    },
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn parse<T: DeserializeOwned>(packet: &Packet, direction: Direction) -> Result<T> {
    ensure!(
        packet.as_bytes()[5] == direction as u8,
        "installation message direction mismatch"
    );
    serde_json::from_slice(packet.payload())
        .map_err(|_| anyhow::anyhow!("installation strict message refused"))
}
fn bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(value)?;
    ensure!(raw.len() <= MAX_PAYLOAD_BYTES, "installation message bound");
    Ok(raw)
}
impl Input {
    pub fn parse(packet: &Packet) -> Result<Self> {
        let value: Self = parse(packet, Direction::Command)?;
        value.validate()?;
        Ok(value)
    }
    pub fn bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        bytes(self)
    }
    fn validate(&self) -> Result<()> {
        let valid = match self {
            Self::Authorize {
                request_sha256,
                runtime_sha256,
            } => hash(request_sha256) && hash(runtime_sha256),
            Self::TextShown {
                transcript_sha256,
                columns,
                rows,
                input_code_page,
                output_code_page,
            } => {
                hash(transcript_sha256)
                    && *columns >= 80
                    && *rows >= 30
                    && *input_code_page == 65001
                    && *output_code_page == 65001
            }
            Self::Control {
                id,
                received_counter,
                ..
            } => *id > 0 && *received_counter > 0,
        };
        ensure!(valid, "installation message profile");
        Ok(())
    }
}
fn snapshot(value: &Snapshot) -> Result<()> {
    ensure!(
        value.source_frame <= 480000
            && value.interval_frame <= INTERVAL_FRAMES
            && (1..=3).contains(&value.pass)
            && (value.state == State::Fault) == value.fault.is_some(),
        "installation observed snapshot profile"
    );
    Ok(())
}
impl Output {
    pub fn parse(packet: &Packet) -> Result<Self> {
        let value: Self = parse(packet, Direction::Report)?;
        value.validate()?;
        if let Some((status, _)) = value.status() {
            ensure!(
                status.generation == packet.generation(),
                "installation observed generation mismatch"
            );
        } else {
            ensure!(
                packet.generation() == 0,
                "installation bootstrap generation mismatch"
            );
        }
        Ok(value)
    }
    pub fn bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        bytes(self)
    }
    fn status(&self) -> Option<(&Snapshot, i64)> {
        match self {
            Self::Ready {
                snapshot,
                observed_counter,
            }
            | Self::State {
                snapshot,
                observed_counter,
            }
            | Self::Ack {
                snapshot,
                observed_counter,
                ..
            }
            | Self::Ended {
                snapshot,
                observed_counter,
            } => Some((snapshot, *observed_counter)),
            _ => None,
        }
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::Preparing {
                observed_counter, ..
            }
            | Self::PreparationAck {
                observed_counter, ..
            }
            | Self::PreparationEnded {
                observed_counter, ..
            } => {
                ensure!(
                    *observed_counter > 0,
                    "installation preparation counter profile"
                );
                if let Self::PreparationAck { id, .. } = self {
                    ensure!(
                        *id > 0,
                        "installation preparation command identifier profile"
                    );
                }
                if let Self::PreparationEnded { phase, .. } = self {
                    ensure!(
                        matches!(phase, Preparation::Fault | Preparation::Stopped),
                        "installation preparation end profile"
                    );
                }
            }
            Self::Hello {
                request_sha256,
                runtime_sha256,
                process_id,
            } => ensure!(
                hash(request_sha256) && hash(runtime_sha256) && *process_id > 0,
                "installation hello profile"
            ),
            Self::Transcript { text, sha256 } => {
                ensure!(
                    text.len() <= 4096 && hash(sha256),
                    "installation transcript message profile"
                );
                super::super::text(text.as_bytes().to_vec())?;
                ensure!(
                    crate::production::sha256_bytes(text.as_bytes()) == *sha256,
                    "installation exact transcript digest differs"
                );
            }
            _ => {
                let (status, counter) = self.status().unwrap();
                snapshot(status)?;
                ensure!(counter > 0, "installation observation counter profile");
                match self {
                    Self::Ready { .. } => ensure!(
                        status.state == State::Ready
                            && status.generation == 0
                            && !status.session_closed,
                        "installation Ready observation differs"
                    ),
                    Self::Ack { id, .. } => {
                        ensure!(*id > 0, "installation command identifier profile")
                    }
                    Self::Ended { .. } => ensure!(
                        status.session_closed
                            && matches!(status.state, State::Stopped | State::Fault),
                        "installation end observation differs"
                    ),
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

/// Immutable expected hashes from a guarded current-player binding. Not Serde
/// or a programme admission token. The declaration's producer is unrelated.
#[derive(Clone)]
#[cfg(any(windows, test))]
pub struct Context {
    request_sha256: String,
    runtime_sha256: String,
    #[cfg(windows)]
    _guards: Option<(
        std::sync::Arc<crate::media_runtime::BoundTool>,
        std::sync::Arc<crate::media_runtime::BoundInput>,
    )>,
}
#[cfg(windows)]
pub struct RuntimeBinding {
    request: SessionRequest,
    context: Context,
    tool: std::sync::Arc<crate::media_runtime::BoundTool>,
    lock: std::sync::Arc<crate::media_runtime::BoundInput>,
}
#[cfg(windows)]
impl RuntimeBinding {
    /// Explicit blocking file operation; call off the controller. Checks no
    /// originals/devices. The embedded lock binds this build's actual lock bytes;
    /// source_commit remains independently declared, not an invented attestation.
    pub fn bind(raw: &[u8]) -> Result<Self> {
        let request = SessionRequest::parse(raw)?;
        let tool = std::sync::Arc::new(crate::media_runtime::BoundTool::bind(
            &request.playback.runtime,
        )?);
        ensure!(
            tool.path().canonicalize()? == std::env::current_exe()?.canonicalize()?,
            "installation player must be current retained image"
        );
        let lock = std::sync::Arc::new(crate::media_runtime::BoundInput::bind(
            &request.playback.cargo_lock,
        )?);
        let actual_lock = lock.read(4 * 1024 * 1024)?;
        ensure!(
            actual_lock.as_slice()
                == include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock")),
            "installation playback lock differs from compiled lock"
        );
        let context = Context {
            request_sha256: crate::production::sha256_bytes(raw),
            runtime_sha256: tool.identity().sha256.clone(),
            _guards: Some((tool.clone(), lock.clone())),
        };
        Ok(Self {
            request,
            context,
            tool,
            lock,
        })
    }
    pub fn request(&self) -> &SessionRequest {
        &self.request
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
    pub fn runtime_path(&self) -> &std::path::Path {
        self.tool.path()
    }
    pub fn recheck(&self) -> Result<()> {
        self.lock.recheck()?;
        ensure!(
            crate::media_runtime::ByteIdentity::observe_file(self.tool.path())?
                == *self.tool.identity(),
            "installation guarded player bytes changed"
        );
        Ok(())
    }
}

/// Worker-side admission, not playback authority. Source checks can start only
/// after actual peer verification plus matching exact request/player hashes.
/// Commands still apply to the real core; no wire observation sets its state.
#[cfg(any(windows, test))]
pub struct WorkerGate {
    context: Context,
    nonce: [u8; 16],
    parent_pid: u32,
    authorized: bool,
    text_sha256: Option<String>,
    shown: bool,
    last_id: u64,
    last_counter: i64,
    pending: [Option<u64>; 8],
    ended: bool,
}
#[cfg(any(windows, test))]
pub enum Admission {
    Authorized,
    TextShown,
    Refused {
        id: u64,
        command: Command,
        reason: TransportError,
        received_counter: i64,
    },
    Command {
        id: u64,
        command: Command,
        received_counter: i64,
        generation: u64,
    },
}
#[cfg(any(windows, test))]
impl WorkerGate {
    pub fn new(context: &Context, parent_pid: u32, nonce: [u8; 16]) -> Result<Self> {
        ensure!(
            parent_pid > 0 && nonce != [0; 16],
            "installation parent identity unavailable"
        );
        Ok(Self {
            context: context.clone(),
            nonce,
            parent_pid,
            authorized: false,
            text_sha256: None,
            shown: false,
            last_id: 0,
            last_counter: 0,
            pending: [None; 8],
            ended: false,
        })
    }
    pub fn hello(&self) -> Output {
        Output::Hello {
            request_sha256: self.context.request_sha256.clone(),
            runtime_sha256: self.context.runtime_sha256.clone(),
            process_id: std::process::id(),
        }
    }
    #[cfg(windows)]
    pub fn admit(
        &mut self,
        channel: &super::pipe::Channel,
        packet: &Packet,
        now_counter: i64,
        origin_counter: i64,
    ) -> Result<Admission> {
        if !channel.verify_peer(self.parent_pid).unwrap_or(false) {
            self.end();
            anyhow::bail!("installation actual parent peer unavailable");
        }
        self.admit_verified(packet, now_counter, origin_counter, self.parent_pid)
    }
    fn admit_verified(
        &mut self,
        packet: &Packet,
        now: i64,
        origin: i64,
        peer_pid: u32,
    ) -> Result<Admission> {
        ensure!(!self.ended, "installation worker admission ended");
        let result = if peer_pid != self.parent_pid || packet.as_bytes()[8..24] != self.nonce {
            Err(anyhow::anyhow!("installation actual parent peer differs"))
        } else {
            self.admit_inner(packet, now, origin)
        };
        if result.is_err() {
            self.ended = true;
        }
        result
    }
    fn admit_inner(&mut self, packet: &Packet, now: i64, origin: i64) -> Result<Admission> {
        ensure!(
            origin > 0 && now >= origin,
            "installation message clock context"
        );
        match Input::parse(packet)? {
            Input::Authorize {
                request_sha256,
                runtime_sha256,
            } => {
                ensure!(
                    !self.authorized
                        && packet.generation() == 0
                        && request_sha256 == self.context.request_sha256
                        && runtime_sha256 == self.context.runtime_sha256,
                    "installation authorization differs"
                );
                self.authorized = true;
                Ok(Admission::Authorized)
            }
            Input::TextShown {
                transcript_sha256, ..
            } => {
                ensure!(
                    self.authorized
                        && !self.shown
                        && packet.generation() == 0
                        && self.text_sha256.as_deref() == Some(&transcript_sha256),
                    "installation transcript acknowledgement differs"
                );
                self.shown = true;
                Ok(Admission::TextShown)
            }
            Input::Control {
                id,
                command,
                received_counter,
            } => {
                ensure!(
                    self.authorized
                        && id == self.last_id.checked_add(1).unwrap_or(0)
                        && received_counter >= origin
                        && received_counter >= self.last_counter
                        && received_counter <= now,
                    "installation command context differs"
                );
                let slot = self
                    .pending
                    .iter_mut()
                    .find(|entry| entry.is_none())
                    .ok_or_else(|| {
                        anyhow::anyhow!("installation eight outstanding commands exceeded")
                    })?;
                *slot = Some(id);
                self.last_id = id;
                self.last_counter = received_counter;
                if !self.shown && !matches!(command, Command::Status | Command::Quit) {
                    return Ok(Admission::Refused {
                        id,
                        command,
                        reason: TransportError::InvalidState,
                        received_counter,
                    });
                }
                Ok(Admission::Command {
                    id,
                    command,
                    received_counter,
                    generation: packet.generation(),
                })
            }
        }
    }
    pub fn transcript(&mut self, text: String) -> Result<Output> {
        ensure!(
            self.authorized && self.text_sha256.is_none() && !self.ended,
            "installation transcript admission state"
        );
        let sha256 = crate::production::sha256_bytes(text.as_bytes());
        let value = Output::Transcript {
            text,
            sha256: sha256.clone(),
        };
        value.validate()?;
        self.text_sha256 = Some(sha256);
        Ok(value)
    }
    /// Release only after the real worker/core has applied/refused that command.
    /// Queue acceptance is not an acknowledgement and cannot free the budget.
    pub fn acknowledged(&mut self, id: u64) -> Result<()> {
        let slot = self
            .pending
            .iter_mut()
            .find(|entry| **entry == Some(id))
            .ok_or_else(|| anyhow::anyhow!("installation unknown command acknowledgement"))?;
        *slot = None;
        Ok(())
    }
    pub fn end(&mut self) {
        self.ended = true;
    }
}

/// Parent-side bootstrap gate. An authorization message is produced only after
/// the exact held worker PID, run context and guarded request/image hashes agree.
/// This is not admission of originals or a displayed-text receipt.
#[cfg(any(windows, test))]
pub struct ParentGate {
    context: Context,
    worker_pid: u32,
    nonce: [u8; 16],
    completed: bool,
    ended: bool,
}
#[cfg(any(windows, test))]
impl ParentGate {
    pub fn new(context: &Context, worker_pid: u32, nonce: [u8; 16]) -> Result<Self> {
        ensure!(
            worker_pid > 0 && nonce != [0; 16],
            "installation held worker identity unavailable"
        );
        Ok(Self {
            context: context.clone(),
            worker_pid,
            nonce,
            completed: false,
            ended: false,
        })
    }
    #[cfg(windows)]
    pub fn authorize(&mut self, channel: &super::pipe::Channel, packet: &Packet) -> Result<Input> {
        if !channel.verify_peer(self.worker_pid).unwrap_or(false) {
            self.ended = true;
            anyhow::bail!("installation held worker pipe differs");
        }
        self.authorize_verified(packet, self.worker_pid)
    }
    fn authorize_verified(&mut self, packet: &Packet, peer: u32) -> Result<Input> {
        ensure!(
            !self.ended && !self.completed,
            "installation parent bootstrap ended"
        );
        let result = (|| -> Result<Input> {
            ensure!(
                peer == self.worker_pid && packet.as_bytes()[8..24] == self.nonce,
                "installation parent peer/context differs"
            );
            let Output::Hello {
                request_sha256,
                runtime_sha256,
                process_id,
            } = Output::parse(packet)?
            else {
                anyhow::bail!("installation expected worker hello")
            };
            ensure!(
                request_sha256 == self.context.request_sha256
                    && runtime_sha256 == self.context.runtime_sha256
                    && process_id == self.worker_pid,
                "installation worker hello differs from held context"
            );
            Ok(Input::Authorize {
                request_sha256,
                runtime_sha256,
            })
        })();
        if result.is_ok() {
            self.completed = true;
        } else {
            self.ended = true;
        }
        result
    }
    pub fn end(&mut self) {
        self.ended = true;
    }
}

#[cfg(test)]
mod tests {
    use super::super::wire::Encoder;
    use super::*;
    fn context() -> Context {
        Context {
            request_sha256: "a".repeat(64),
            runtime_sha256: "b".repeat(64),
            #[cfg(windows)]
            _guards: None,
        }
    }
    fn packet(input: Input, generation: u64) -> Packet {
        Encoder::new([1; 16], Direction::Command)
            .unwrap()
            .encode(generation, &input.bytes().unwrap())
            .unwrap()
    }
    fn authorized() -> WorkerGate {
        let mut gate = WorkerGate::new(&context(), 42, [1; 16]).unwrap();
        let input = Input::Authorize {
            request_sha256: "a".repeat(64),
            runtime_sha256: "b".repeat(64),
        };
        assert!(matches!(
            gate.admit_verified(&packet(input, 0), 200, 100, 42)
                .unwrap(),
            Admission::Authorized
        ));
        gate
    }
    fn shown() -> WorkerGate {
        let mut gate = authorized();
        let Output::Transcript { sha256, .. } = gate
            .transcript("Engineering signal; acción\n".into())
            .unwrap()
        else {
            panic!("missing transcript")
        };
        let input = Input::TextShown {
            transcript_sha256: sha256,
            columns: 80,
            rows: 30,
            input_code_page: 65001,
            output_code_page: 65001,
        };
        assert!(matches!(
            gate.admit_verified(&packet(input, 0), 200, 100, 42)
                .unwrap(),
            Admission::TextShown
        ));
        gate
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "contained authentication child; explicitly launched by parent"]
    fn admission_child_fixture() {
        use super::super::pipe::{Channel, Invitation};
        let raw = std::fs::read("session.json").unwrap();
        let binding = RuntimeBinding::bind(&raw).unwrap();
        assert!(!binding.request().original.declaration.path.exists());
        let invitation: Invitation =
            serde_json::from_slice(&std::fs::read("invitation.json").unwrap()).unwrap();
        let mut channel = Channel::connect(&invitation).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        while !channel.verify_peer(invitation.server_process_id).unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut gate = WorkerGate::new(
            binding.context(),
            invitation.server_process_id,
            invitation.nonce,
        )
        .unwrap();
        channel.send(0, &gate.hello().bytes().unwrap()).unwrap();
        let packet = loop {
            if let Some(packet) = channel.receive() {
                break packet;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        };
        assert!(matches!(
            gate.admit(&channel, &packet, 200, 100).unwrap(),
            Admission::Authorized
        ));
        binding.recheck().unwrap();
        std::fs::write("authorized.marker", b"authorized before original checks").unwrap();
        channel.shutdown().unwrap();
    }
    #[test]
    #[cfg(windows)]
    fn actual_contained_authentication_binds_both_peers_request_image_and_lock() {
        use super::super::pipe::Channel;
        use crate::scene_delivery::FileRef;
        use reel_assets::jobs::{QuietEvent, QuietLimits, run_quiet_observed};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicU32, Ordering},
        };
        let root = tempfile::tempdir().unwrap();
        let mut request = super::super::session::fixture_request();
        let identity =
            crate::media_runtime::ByteIdentity::observe_file(&std::env::current_exe().unwrap())
                .unwrap();
        request.playback.runtime = FileRef {
            path: std::env::current_exe().unwrap(),
            sha256: identity.sha256,
            bytes: identity.bytes,
        };
        request.original.declaration.path = root.path().join("unavailable-original.json");
        let lock = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"));
        let lock_path = root.path().join("retained.lock");
        std::fs::write(&lock_path, lock).unwrap();
        request.playback.cargo_lock = FileRef {
            path: lock_path,
            sha256: crate::production::sha256_bytes(lock),
            bytes: lock.len() as u64,
        };
        let raw = serde_json::to_vec(&request).unwrap();
        let binding = RuntimeBinding::bind(&raw).unwrap();
        std::fs::write(root.path().join("session.json"), raw).unwrap();
        let (invitation, mut channel) = Channel::server().unwrap();
        std::fs::write(
            root.path().join("invitation.json"),
            serde_json::to_vec(&invitation).unwrap(),
        )
        .unwrap();
        let actual_pid = AtomicU32::new(0);
        let cancel = Arc::new(AtomicBool::new(false));
        struct Cancel(Arc<AtomicBool>);
        impl Drop for Cancel {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        std::thread::scope(|scope| {
            let (program, directory, pid) = (binding.runtime_path(), root.path(), &actual_pid);
            let child_cancel = cancel.clone();
            let job=scope.spawn(move||run_quiet_observed(program,&[
                "--ignored".into(),"--exact".into(),"spoken_programme::installation::native::protocol::tests::admission_child_fixture".into(),"--test-threads=1".into()
            ],directory,Duration::from_secs(8),QuietLimits{memory_bytes:256*1024*1024,processes:1},|event|{
                if child_cancel.load(Ordering::Acquire){return Err("controlled authentication cancelled".into());}
                if let QuietEvent::Process(info)=event{pid.store(info.process_id,Ordering::Release);}
                Ok(())
            }).map_err(|error|error.to_string()));
            let _cancel = Cancel(cancel.clone());
            let deadline = Instant::now() + Duration::from_secs(5);
            let hello = loop {
                let pid = actual_pid.load(Ordering::Acquire);
                if pid != 0 && channel.verify_peer(pid).unwrap() {
                    if let Some(packet) = channel.receive() {
                        break packet;
                    }
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            };
            let mut parent = ParentGate::new(
                binding.context(),
                actual_pid.load(Ordering::Acquire),
                invitation.nonce,
            )
            .unwrap();
            let authorization = parent.authorize(&channel, &hello).unwrap();
            channel.send(0, &authorization.bytes().unwrap()).unwrap();
            let result = job.join().unwrap();
            assert!(result.is_ok(), "{result:?}");
            assert!(root.path().join("authorized.marker").is_file());
            assert!(!request.original.declaration.path.exists());
            channel.shutdown().unwrap();
        });
    }
    #[test]
    fn preparing_controls_do_not_invent_a_transport_snapshot() {
        let message = Output::PreparationAck {
            id: 1,
            phase: Preparation::Checking,
            refusal: None,
            observed_counter: 150,
        };
        let body = message.bytes().unwrap();
        assert!(!std::str::from_utf8(&body).unwrap().contains("snapshot"));
        let packet = Encoder::new([1; 16], Direction::Report)
            .unwrap()
            .encode(0, &body)
            .unwrap();
        assert!(matches!(
            Output::parse(&packet).unwrap(),
            Output::PreparationAck {
                phase: Preparation::Checking,
                ..
            }
        ));
        assert!(
            Output::PreparationEnded {
                phase: Preparation::Checking,
                observed_counter: 150
            }
            .bytes()
            .is_err()
        );
        assert!(
            Output::PreparationEnded {
                phase: Preparation::Stopped,
                observed_counter: 150
            }
            .bytes()
            .is_ok()
        );
    }
    #[test]
    fn parent_bootstrap_checks_held_peer_context_and_rejects_replay() {
        let hello = Output::Hello {
            request_sha256: "a".repeat(64),
            runtime_sha256: "b".repeat(64),
            process_id: 42,
        };
        let packet = Encoder::new([1; 16], Direction::Report)
            .unwrap()
            .encode(0, &hello.bytes().unwrap())
            .unwrap();
        for (pid, nonce, peer) in [(43, [1; 16], 42), (42, [2; 16], 42), (42, [1; 16], 43)] {
            let mut gate = ParentGate::new(&context(), pid, nonce).unwrap();
            assert!(gate.authorize_verified(&packet, peer).is_err());
            assert!(gate.ended);
        }
        let mut gate = ParentGate::new(&context(), 42, [1; 16]).unwrap();
        assert!(matches!(
            gate.authorize_verified(&packet, 42).unwrap(),
            Input::Authorize { .. }
        ));
        assert!(gate.authorize_verified(&packet, 42).is_err());
        let mut worker = WorkerGate::new(&context(), 42, [2; 16]).unwrap();
        assert!(
            worker
                .admit_verified(
                    &super::tests::packet(
                        Input::Authorize {
                            request_sha256: "a".repeat(64),
                            runtime_sha256: "b".repeat(64)
                        },
                        0
                    ),
                    200,
                    100,
                    42
                )
                .is_err()
        );
    }
    #[test]
    fn strict_bodies_reject_unknown_fields_directions_and_counter_shapes() {
        for raw in [
            r#"{"kind":"control","id":1,"command":"start","received_counter":100,"success":true}"#,
            r#"{"kind":"control","id":1,"command":"START","received_counter":100}"#,
            r#"{"kind":"control","id":0,"command":"start","received_counter":100}"#,
            r#"{"kind":"control","id":1,"command":"start","received_counter":0}"#,
            r#"{"kind":"grant_restart","success":true}"#,
        ] {
            let input = Encoder::new([1; 16], Direction::Command)
                .unwrap()
                .encode(0, raw.as_bytes())
                .unwrap();
            assert!(Input::parse(&input).is_err());
        }
        let valid = Input::Control {
            id: 1,
            command: Command::Start,
            received_counter: 100,
        }
        .bytes()
        .unwrap();
        let wrong = Encoder::new([1; 16], Direction::Report)
            .unwrap()
            .encode(0, &valid)
            .unwrap();
        assert!(Input::parse(&wrong).is_err());
    }
    #[test]
    fn authorization_requires_exact_request_runtime_and_actual_peer_once() {
        for (request, runtime, pid, generation) in [
            ("c", "b", 42, 0),
            ("a", "c", 42, 0),
            ("a", "b", 43, 0),
            ("a", "b", 42, 1),
        ] {
            let mut gate = WorkerGate::new(&context(), 42, [1; 16]).unwrap();
            let input = Input::Authorize {
                request_sha256: request.repeat(64),
                runtime_sha256: runtime.repeat(64),
            };
            assert!(
                gate.admit_verified(&packet(input, generation), 200, 100, pid)
                    .is_err()
            );
            assert!(gate.ended && !gate.authorized);
        }
        let mut gate = authorized();
        let input = Input::Authorize {
            request_sha256: "a".repeat(64),
            runtime_sha256: "b".repeat(64),
        };
        assert!(
            gate.admit_verified(&packet(input, 0), 200, 100, 42)
                .is_err()
        );
    }
    #[test]
    fn start_needs_exact_text_acknowledgement_but_status_and_quit_do_not() {
        for command in [Command::Status, Command::Quit] {
            let mut gate = authorized();
            assert!(matches!(
                gate.admit_verified(
                    &packet(
                        Input::Control {
                            id: 1,
                            command,
                            received_counter: 150
                        },
                        99
                    ),
                    200,
                    100,
                    42
                )
                .unwrap(),
                Admission::Command { generation: 99, .. }
            ));
        }
        let mut gate = authorized();
        assert!(matches!(
            gate.admit_verified(
                &packet(
                    Input::Control {
                        id: 1,
                        command: Command::Start,
                        received_counter: 150
                    },
                    0
                ),
                200,
                100,
                42
            )
            .unwrap(),
            Admission::Refused {
                reason: TransportError::InvalidState,
                ..
            }
        ));
        assert!(!gate.ended);
        let mut gate = authorized();
        gate.transcript("Texto español\n".into()).unwrap();
        let bad = Input::TextShown {
            transcript_sha256: "a".repeat(64),
            columns: 80,
            rows: 30,
            input_code_page: 65001,
            output_code_page: 65001,
        };
        assert!(gate.admit_verified(&packet(bad, 0), 200, 100, 42).is_err());
        let mut gate = shown();
        assert!(matches!(
            gate.admit_verified(
                &packet(
                    Input::Control {
                        id: 1,
                        command: Command::Start,
                        received_counter: 150
                    },
                    0
                ),
                200,
                100,
                42
            )
            .unwrap(),
            Admission::Command {
                command: Command::Start,
                ..
            }
        ));
    }
    #[test]
    fn eight_unique_commands_remain_outstanding_until_actual_acknowledgement() {
        let mut gate = shown();
        for id in 1..=8 {
            gate.admit_verified(
                &packet(
                    Input::Control {
                        id,
                        command: Command::Status,
                        received_counter: 150,
                    },
                    0,
                ),
                200,
                100,
                42,
            )
            .unwrap();
        }
        assert!(gate.acknowledged(9).is_err());
        gate.acknowledged(1).unwrap();
        assert!(gate.acknowledged(1).is_err());
        gate.admit_verified(
            &packet(
                Input::Control {
                    id: 9,
                    command: Command::Status,
                    received_counter: 150,
                },
                0,
            ),
            200,
            100,
            42,
        )
        .unwrap();
        assert!(
            gate.admit_verified(
                &packet(
                    Input::Control {
                        id: 10,
                        command: Command::Status,
                        received_counter: 150
                    },
                    0
                ),
                200,
                100,
                42
            )
            .is_err()
        );
        assert!(gate.ended);
    }
    #[test]
    fn repeated_skipped_future_and_reversed_commands_cannot_resume_admission() {
        for (id, stamp) in [(1, 151), (3, 151), (2, 201), (2, 149)] {
            let mut gate = shown();
            gate.admit_verified(
                &packet(
                    Input::Control {
                        id: 1,
                        command: Command::Status,
                        received_counter: 150,
                    },
                    0,
                ),
                200,
                100,
                42,
            )
            .unwrap();
            assert!(
                gate.admit_verified(
                    &packet(
                        Input::Control {
                            id,
                            command: Command::Status,
                            received_counter: stamp
                        },
                        0
                    ),
                    200,
                    100,
                    42
                )
                .is_err()
            );
            assert!(gate.ended);
        }
    }
    #[test]
    fn transcript_bytes_hash_layout_and_observed_generation_are_checked() {
        let text = "Texto español\n".to_string();
        let message = Output::Transcript {
            sha256: crate::production::sha256_bytes(text.as_bytes()),
            text,
        };
        let packet = Encoder::new([1; 16], Direction::Report)
            .unwrap()
            .encode(0, &message.bytes().unwrap())
            .unwrap();
        assert!(matches!(
            Output::parse(&packet).unwrap(),
            Output::Transcript { .. }
        ));
        assert!(
            Output::Transcript {
                text: "Texto español\n".into(),
                sha256: "a".repeat(64)
            }
            .bytes()
            .is_err()
        );
        assert!(
            Input::TextShown {
                transcript_sha256: "a".repeat(64),
                columns: 79,
                rows: 30,
                input_code_page: 65001,
                output_code_page: 65001
            }
            .bytes()
            .is_err()
        );
        let status = Snapshot {
            state: State::Ready,
            generation: 0,
            source_frame: 0,
            interval_frame: 0,
            pass: 1,
            command_sequence: 0,
            session_closed: false,
            fault: None,
        };
        let message = Output::Ready {
            snapshot: status,
            observed_counter: 150,
        };
        let wrong = Encoder::new([1; 16], Direction::Report)
            .unwrap()
            .encode(1, &message.bytes().unwrap())
            .unwrap();
        assert!(Output::parse(&wrong).is_err());
        assert!(
            Output::Ended {
                snapshot: status,
                observed_counter: 150
            }
            .bytes()
            .is_err()
        );
    }
}

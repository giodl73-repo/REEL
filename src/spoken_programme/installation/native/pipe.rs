//! Explicit local IPC port. No source check, endpoint or playback is initiated.
//! Two one-way file objects keep blocked reads independent of writes. The
//! session still owes runtime/request authentication and strict message admission.
use super::wire::{Decoder, Direction, Encoder, MAX_FRAME_BYTES, Packet};
use anyhow::{Result, ensure};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Write,
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Security::{Authorization::*, Cryptography::*, *},
        Storage::FileSystem::*,
        System::{IO::CancelSynchronousIo, Pipes::*, Threading::*},
    },
    core::{PCWSTR, PWSTR},
};

/// Private invitation only, never a programme, credential receipt or authority.
/// Its exact local pipe names derive from the random run context, not caller paths.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invitation {
    pub nonce: [u8; 16],
    pub server_process_id: u32,
}
impl Invitation {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.nonce != [0; 16] && self.server_process_id != 0,
            "installation pipe invitation context"
        );
        Ok(())
    }
    fn name(&self, command: bool) -> Vec<u16> {
        let hex: String = self.nonce.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "\\\\.\\pipe\\reel-installation-v1-{hex}-{}",
            if command { "command" } else { "report" }
        )
        .encode_utf16()
        .chain(Some(0))
        .collect()
    }
}
struct Local(HLOCAL);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(self.0));
        }
    }
}
struct Security {
    descriptor: Local,
}
impl Security {
    fn current_user() -> Result<Self> {
        let mut raw = HANDLE::default();
        unsafe {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw)?;
        }
        let token_owner = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let token = HANDLE(token_owner.as_raw_handle());
        let mut size = 0;
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut size) };
        ensure!(
            (std::mem::size_of::<TOKEN_USER>()..=65536).contains(&(size as usize)),
            "installation pipe token profile"
        );
        // Aligned storage keeps TOKEN_USER/SID live through string conversion.
        let mut storage = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(storage.as_mut_ptr().cast()),
                size,
                &mut size,
            )?;
        }
        let sid = unsafe { (*storage.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut sid_text = PWSTR::null();
        unsafe {
            ConvertSidToStringSidW(sid, &mut sid_text)?;
        }
        let _sid_owner = Local(HLOCAL(sid_text.0.cast()));
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})", unsafe { sid_text.to_string()? })
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )?;
        }
        Ok(Self {
            descriptor: Local(HLOCAL(descriptor.0)),
        })
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0.0,
            bInheritHandle: false.into(),
        }
    }
}
fn server_file(invitation: &Invitation, command: bool, security: &Security) -> Result<File> {
    let name = invitation.name(command);
    let attributes = security.attributes();
    let raw = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            (if command {
                PIPE_ACCESS_OUTBOUND
            } else {
                PIPE_ACCESS_INBOUND
            }) | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            MAX_FRAME_BYTES as u32,
            MAX_FRAME_BYTES as u32,
            0,
            Some(&attributes),
        )
    };
    ensure!(
        !raw.is_invalid(),
        "installation local pipe creation refused"
    );
    Ok(unsafe { File::from_raw_handle(raw.0) })
}
fn client_file(invitation: &Invitation, command: bool) -> Result<File> {
    let name = invitation.name(command);
    let raw = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            if command {
                GENERIC_READ.0
            } else {
                GENERIC_WRITE.0
            },
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            None,
        )?
    };
    Ok(unsafe { File::from_raw_handle(raw.0) })
}
fn peer(file: &File, server: bool, expected: u32) -> Result<u32> {
    let handle = HANDLE(file.as_raw_handle());
    let mut pid = 0;
    if server {
        match unsafe { ConnectNamedPipe(handle, None) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
            Err(_) => anyhow::bail!("installation pipe connection ended"),
        }
        unsafe {
            GetNamedPipeClientProcessId(handle, &mut pid)?;
        }
    } else {
        unsafe {
            GetNamedPipeServerProcessId(handle, &mut pid)?;
        }
        ensure!(pid == expected, "installation pipe server process differs");
    }
    ensure!(pid != 0, "installation pipe peer unavailable");
    Ok(pid)
}
struct Facts {
    stopped: AtomicBool,
    failed: AtomicBool,
    reader_peer: AtomicU32,
    writer_peer: AtomicU32,
}
impl Facts {
    fn finish(&self, result: Result<()>) {
        if result.is_err() && !self.stopped.load(Ordering::Acquire) {
            self.failed.store(true, Ordering::Release);
        }
        self.stopped.store(true, Ordering::Release);
    }
}
/// A bounded local channel, not a session/authentication success. The parent
/// must verify BOTH peers against the PID observed by its actual job owner.
/// Construction/teardown can block on the OS; run outside the control/callback
/// loops. Cancellation repeatedly targets the owned I/O threads and joins real
/// completion. It never drops a pending thread's buffers/handles early.
pub struct Channel {
    outgoing: Producer<Packet>,
    incoming: Consumer<Packet>,
    encoder: Encoder,
    facts: Arc<Facts>,
    threads: Vec<JoinHandle<()>>,
}
impl Channel {
    pub fn server() -> Result<(Invitation, Self)> {
        let mut nonce = [0; 16];
        let status = unsafe { BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
        ensure!(
            status.0 >= 0,
            "installation pipe random context unavailable"
        );
        let invitation = Invitation {
            nonce,
            server_process_id: std::process::id(),
        };
        invitation.validate()?;
        let security = Security::current_user()?;
        let command = server_file(&invitation, true, &security)?;
        let report = server_file(&invitation, false, &security)?;
        let channel = Self::start(&invitation, true, command, report)?;
        Ok((invitation, channel))
    }
    /// Explicit local connection only. No path argument, remote machine, retry
    /// or default endpoint. Actual server PID must match the private invitation.
    pub fn connect(invitation: &Invitation) -> Result<Self> {
        invitation.validate()?;
        let command = client_file(invitation, true)?;
        let report = client_file(invitation, false)?;
        Self::start(invitation, false, report, command)
    }
    fn start(
        invitation: &Invitation,
        server: bool,
        mut writer: File,
        mut reader: File,
    ) -> Result<Self> {
        let direction = if server {
            Direction::Command
        } else {
            Direction::Report
        };
        let (outgoing, mut send) = RingBuffer::<Packet>::new(if server { 8 } else { 256 });
        let (mut received, incoming) = RingBuffer::new(if server { 256 } else { 8 });
        let facts = Arc::new(Facts {
            stopped: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            reader_peer: AtomicU32::new(0),
            writer_peer: AtomicU32::new(0),
        });
        let mut decoder = Decoder::new(invitation.nonce, direction.opposite())?;
        let mut channel = Self {
            outgoing,
            incoming,
            encoder: Encoder::new(invitation.nonce, direction)?,
            facts,
            threads: Vec::with_capacity(2),
        };
        let expected = invitation.server_process_id;
        let facts = channel.facts.clone();
        let owner = std::thread::current();
        channel.threads.push(
            std::thread::Builder::new()
                .name("reel-installation-pipe-reader".into())
                .spawn(move || {
                    let result = (|| -> Result<()> {
                        if facts.stopped.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        let pid = peer(&reader, server, expected)?;
                        facts.reader_peer.store(pid, Ordering::Release);
                        while !facts.stopped.load(Ordering::Acquire) {
                            let packet = decoder.read(&mut reader)?;
                            ensure!(
                                received.push(packet).is_ok(),
                                "installation pipe incoming queue full"
                            );
                            owner.unpark();
                        }
                        Ok(())
                    })();
                    facts.finish(result);
                    owner.unpark();
                })?,
        );
        let facts = channel.facts.clone();
        channel.threads.push(
            std::thread::Builder::new()
                .name("reel-installation-pipe-writer".into())
                .spawn(move || {
                    let result = (|| -> Result<()> {
                        if facts.stopped.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        let pid = peer(&writer, server, expected)?;
                        facts.writer_peer.store(pid, Ordering::Release);
                        while !facts.stopped.load(Ordering::Acquire) {
                            if let Ok(packet) = send.pop() {
                                writer.write_all(packet.as_bytes()).map_err(|_| {
                                    anyhow::anyhow!("installation pipe write ended")
                                })?;
                            } else {
                                // A send that races this park leaves a wake token.
                                std::thread::park();
                            }
                        }
                        Ok(())
                    })();
                    facts.finish(result);
                })?,
        );
        Ok(channel)
    }
    pub fn failed(&self) -> bool {
        self.facts.failed.load(Ordering::Acquire)
    }
    pub fn stopped(&self) -> bool {
        self.facts.stopped.load(Ordering::Acquire)
    }
    /// Nonblocking. Both native pipe peers must be the same actual owned PID;
    /// a foreign connection faults this run instead of reconnecting it.
    pub fn verify_peer(&self, expected: u32) -> Result<bool> {
        ensure!(
            expected != 0 && !self.failed() && !self.stopped(),
            "installation pipe peer verification refused"
        );
        let reader = self.facts.reader_peer.load(Ordering::Acquire);
        let writer = self.facts.writer_peer.load(Ordering::Acquire);
        if (reader != 0 && reader != expected) || (writer != 0 && writer != expected) {
            self.facts.failed.store(true, Ordering::Release);
            self.request_stop();
            anyhow::bail!("installation pipe actual peer differs");
        }
        Ok(reader == expected && writer == expected)
    }
    pub fn send(&mut self, generation: u64, payload: &[u8]) -> Result<()> {
        ensure!(!self.stopped(), "installation pipe stopped");
        let packet = self.encoder.encode(generation, payload)?;
        if self.outgoing.push(packet).is_err() {
            self.facts.failed.store(true, Ordering::Release);
            self.request_stop();
            anyhow::bail!("installation pipe outgoing queue full");
        }
        self.wake_writer();
        Ok(())
    }
    pub fn receive(&mut self) -> Option<Packet> {
        self.incoming.pop().ok()
    }
    pub fn request_stop(&self) {
        self.facts.stopped.store(true, Ordering::Release);
        self.wake_writer();
    }
    fn wake_writer(&self) {
        if let Some(writer) = self.threads.get(1) {
            writer.thread().unpark();
        }
    }
    /// Returns success only after both real threads/handles have closed within
    /// two seconds. A broken OS operation may exceed it; actual joins still take
    /// precedence over a fake timer success or abandoned I/O ownership.
    pub fn shutdown(&mut self) -> Result<()> {
        let start = Instant::now();
        self.request_stop();
        while self.threads.iter().any(|thread| !thread.is_finished()) {
            for thread in &self.threads {
                unsafe {
                    let _ = CancelSynchronousIo(HANDLE(thread.as_handle().as_raw_handle()));
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        for thread in self.threads.drain(..) {
            if thread.join().is_err() {
                self.facts.failed.store(true, Ordering::Release);
            }
        }
        ensure!(
            start.elapsed() <= Duration::from_secs(2),
            "installation pipe actual cleanup exceeded profile"
        );
        Ok(())
    }
}
impl Drop for Channel {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait(mut ready: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(3);
        while !ready() {
            assert!(Instant::now() < until, "controlled pipe fixture deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn connected() -> (Channel, Channel) {
        let (invitation, parent) = Channel::server().unwrap();
        let child = Channel::connect(&invitation).unwrap();
        wait(|| {
            parent.verify_peer(std::process::id()).unwrap()
                && child.verify_peer(std::process::id()).unwrap()
        });
        (parent, child)
    }
    #[test]
    fn separate_pipe_objects_deliver_both_directions_with_blocked_readers() {
        let (mut parent, mut child) = connected();
        parent.send(1, b"start").unwrap();
        child.send(1, "Texto español: acción\n".as_bytes()).unwrap();
        let mut command = None;
        let mut report = None;
        wait(|| {
            if command.is_none() {
                command = child.receive();
            }
            if report.is_none() {
                report = parent.receive();
            }
            command.is_some() && report.is_some()
        });
        assert_eq!(command.unwrap().payload(), b"start");
        let report = report.unwrap();
        assert_eq!(report.payload(), "Texto español: acción\n".as_bytes());
        assert_eq!((report.generation(), report.sequence()), (1, 1));
        assert!(!parent.failed() && !child.failed());
        parent.shutdown().unwrap();
        wait(|| child.stopped());
        child.shutdown().unwrap();
        assert!(parent.threads.is_empty() && child.threads.is_empty());
        assert!(parent.verify_peer(std::process::id()).is_err());
    }
    #[test]
    fn cancellation_joins_unconnected_and_read_blocked_threads() {
        for _ in 0..24 {
            let (_, mut parent) = Channel::server().unwrap();
            parent.shutdown().unwrap();
            assert!(parent.threads.is_empty() && !parent.failed());
        }
        let (mut parent, mut child) = connected();
        child.shutdown().unwrap();
        wait(|| parent.failed());
        assert!(parent.send(0, b"status").is_err());
        parent.shutdown().unwrap();
    }
    #[test]
    fn actual_foreign_peer_faults_and_bad_server_invitation_refuses() {
        let (mut parent, mut child) = connected();
        assert!(parent.verify_peer(1).is_err());
        assert!(parent.failed() && parent.stopped());
        parent.shutdown().unwrap();
        child.shutdown().unwrap();
        let (mut invitation, mut parent) = Channel::server().unwrap();
        invitation.server_process_id = 1;
        let mut child = Channel::connect(&invitation).unwrap();
        wait(|| child.failed());
        child.shutdown().unwrap();
        parent.shutdown().unwrap();
    }
    #[test]
    fn outgoing_and_incoming_queue_overflow_are_terminal() {
        let (_, mut parent) = Channel::server().unwrap();
        for _ in 0..8 {
            parent.send(0, b"status").unwrap();
        }
        assert!(parent.send(0, b"status").is_err());
        assert!(parent.failed() && parent.stopped());
        parent.shutdown().unwrap();

        let (mut parent, mut child) = connected();
        for _ in 0..257 {
            wait(|| child.outgoing.slots() > 0);
            child.send(0, b"observation").unwrap();
        }
        wait(|| parent.failed());
        assert_eq!(parent.incoming.slots(), 256);
        assert!(parent.stopped());
        parent.shutdown().unwrap();
        child.shutdown().unwrap();
    }
    #[test]
    fn pipe_is_exclusive_and_actual_dacl_contains_only_current_user() {
        let mut nonce = [0; 16];
        assert!(
            unsafe { BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.0 >= 0
        );
        let invitation = Invitation {
            nonce,
            server_process_id: std::process::id(),
        };
        let security = Security::current_user().unwrap();
        let file = server_file(&invitation, true, &security).unwrap();
        assert!(server_file(&invitation, true, &security).is_err());
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let mut actual_acl = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                GetSecurityInfo(
                    HANDLE(file.as_raw_handle()),
                    SE_KERNEL_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    Some(&mut actual_acl),
                    None,
                    Some(&mut descriptor),
                )
            },
            ERROR_SUCCESS
        );
        let _owner = Local(HLOCAL(descriptor.0));
        assert!(!actual_acl.is_null());
        assert_eq!(unsafe { (*actual_acl).AceCount }, 1);
        let (mut present, mut defaulted) = (false.into(), false.into());
        let mut expected_acl = std::ptr::null_mut();
        unsafe {
            GetSecurityDescriptorDacl(
                PSECURITY_DESCRIPTOR(security.descriptor.0.0),
                &mut present,
                &mut expected_acl,
                &mut defaulted,
            )
            .unwrap();
        }
        let (mut actual_ace, mut expected_ace) = (std::ptr::null_mut(), std::ptr::null_mut());
        unsafe {
            GetAce(actual_acl, 0, &mut actual_ace).unwrap();
            GetAce(expected_acl, 0, &mut expected_ace).unwrap();
        }
        let actual_ace = unsafe { &*actual_ace.cast::<ACCESS_ALLOWED_ACE>() };
        let expected_ace = unsafe { &*expected_ace.cast::<ACCESS_ALLOWED_ACE>() };
        assert_eq!(actual_ace.Header.AceType, 0);
        unsafe {
            EqualSid(
                PSID((&actual_ace.SidStart as *const u32).cast_mut().cast()),
                PSID((&expected_ace.SidStart as *const u32).cast_mut().cast()),
            )
            .unwrap();
        }
        let (mut control, mut revision) = (0u16, 0);
        unsafe {
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision).unwrap();
        }
        assert_ne!(control & SE_DACL_PROTECTED.0, 0);
        let mut encoded = serde_json::to_value(&invitation).unwrap();
        encoded["remote"] = "unadmitted".into();
        assert!(serde_json::from_value::<Invitation>(encoded).is_err());
    }
    #[test]
    fn invalid_native_headers_fault_without_waiting_for_the_claimed_body() {
        for offset in [4, 5, 7, 8, 24] {
            let (invitation, mut parent) = Channel::server().unwrap();
            let _command_reader = client_file(&invitation, true).unwrap();
            let mut report_writer = client_file(&invitation, false).unwrap();
            wait(|| parent.verify_peer(std::process::id()).unwrap());
            let packet = Encoder::new(invitation.nonce, Direction::Report)
                .unwrap()
                .encode(0, b"hello")
                .unwrap();
            let mut header = packet.as_bytes()[..super::super::wire::HEADER_BYTES].to_vec();
            header[offset] ^= 255;
            report_writer.write_all(&header).unwrap();
            // Keep the peer open and send no body: refusal must happen from
            // the actual header, rather than a body read timeout/EOF.
            wait(|| parent.failed());
            assert!(parent.stopped() && parent.receive().is_none());
            assert!(parent.verify_peer(std::process::id()).is_err());
            parent.shutdown().unwrap();
        }
    }
    #[test]
    fn unread_pipe_backlog_closes_without_draining_the_peer() {
        let (invitation, mut parent) = Channel::server().unwrap();
        let command_reader = client_file(&invitation, true).unwrap();
        let _report_writer = client_file(&invitation, false).unwrap();
        wait(|| parent.verify_peer(std::process::id()).unwrap());
        for _ in 0..8 {
            parent
                .send(1, &[7; super::super::wire::MAX_PAYLOAD_BYTES])
                .unwrap();
        }
        wait(|| {
            let mut available = 0;
            unsafe {
                PeekNamedPipe(
                    HANDLE(command_reader.as_raw_handle()),
                    None,
                    0,
                    None,
                    Some(&mut available),
                    None,
                )
                .unwrap();
            }
            available >= MAX_FRAME_BYTES as u32
        });
        // No peer reads occur. Cancellation must join actual I/O and close its
        // handles instead of waiting for a cooperative peer or flushing to it.
        parent.shutdown().unwrap();
        assert!(parent.threads.is_empty());
    }
    #[test]
    #[ignore = "contained local-pipe child only; explicitly invoked by parent"]
    fn contained_pipe_fixture() {
        let invitation: Invitation =
            serde_json::from_slice(&std::fs::read("invitation.json").unwrap()).unwrap();
        let mut channel = Channel::connect(&invitation).unwrap();
        wait(|| channel.verify_peer(invitation.server_process_id).unwrap());
        let mut command = None;
        wait(|| {
            command = channel.receive();
            command.is_some()
        });
        let command = command.unwrap();
        assert_eq!(command.payload(), "Hola, acción".as_bytes());
        assert_eq!(command.generation(), 7);
        channel.send(7, b"received").unwrap();
        let mut quit = None;
        wait(|| {
            quit = channel.receive();
            quit.is_some()
        });
        assert_eq!(quit.unwrap().payload(), b"quit");
        channel.shutdown().unwrap();
    }
    #[test]
    fn contained_child_peer_matches_the_held_runner_pid() {
        use crate::media_runtime::{BoundTool, ByteIdentity};
        use crate::scene_delivery::FileRef;
        use reel_assets::jobs::{QuietEvent, QuietLimits, run_quiet_observed};
        let exe = std::env::current_exe().unwrap();
        let identity = ByteIdentity::observe_file(&exe).unwrap();
        let tool = BoundTool::bind(&FileRef {
            path: exe,
            sha256: identity.sha256,
            bytes: identity.bytes,
        })
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let (invitation, mut parent) = Channel::server().unwrap();
        std::fs::write(
            root.path().join("invitation.json"),
            serde_json::to_vec(&invitation).unwrap(),
        )
        .unwrap();
        let pid = AtomicU32::new(0);
        let cancel = Arc::new(AtomicBool::new(false));
        struct Cancel(Arc<AtomicBool>);
        impl Drop for Cancel {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        std::thread::scope(|scope| {
            let child_cancel = cancel.clone();
            let (program, directory, pid_ref) = (tool.path(), root.path(), &pid);
            let job = scope.spawn(move || run_quiet_observed(program, &[
                "--ignored".into(),"--exact".into(),
                "spoken_programme::installation::native::pipe::tests::contained_pipe_fixture".into(),
                "--test-threads=1".into()
            ],directory,Duration::from_secs(6),QuietLimits {memory_bytes:256*1024*1024,processes:1},|event| {
                if child_cancel.load(Ordering::Acquire) { return Err("controlled pipe child cancelled".into()); }
                if let QuietEvent::Process(info) = event { pid_ref.store(info.process_id,Ordering::Release); }
                Ok(())
            }).map_err(|error|error.to_string()));
            let _cancel_on_failure = Cancel(cancel.clone());
            wait(|| {
                let actual = pid.load(Ordering::Acquire);
                actual != 0 && parent.verify_peer(actual).unwrap()
            });
            assert_ne!(pid.load(Ordering::Acquire), std::process::id());
            parent.send(7, "Hola, acción".as_bytes()).unwrap();
            let mut reply = None;
            wait(|| {
                reply = parent.receive();
                reply.is_some()
            });
            assert_eq!(reply.unwrap().payload(), b"received");
            parent.send(0, b"quit").unwrap();
            assert!(job.join().unwrap().is_ok());
            parent.shutdown().unwrap();
        });
    }
}

//! Authenticated native-worker orchestration. Setup and actual destruction are
//! explicit off-controller operations inside the externally owned process.
//! This is not a CLI, console implementation or process-reaping receipt.
use super::{
    controller::{Controller, Event},
    output::{OutputClock, OutputEvent, OutputOperation, OutputWorker},
    pipe::{Channel, Invitation},
    protocol::{Admission, Output, Preparation, RuntimeBinding, WorkerGate},
    source::{SourceEvent, SourceWorker},
    *,
};
use std::{collections::VecDeque, sync::Arc};
use windows::Win32::System::Performance::QueryPerformanceCounter;

struct DisplayAck {
    id: u64,
    received_us: u64,
    reported_counter: Option<i64>,
    generation: u64,
}

/// One actual native worker. Runtime binding and channel construction must have
/// completed off the control loop. Source checking starts only after matching
/// actual peers and Authorize. No endpoint is queried before exact TextShown.
pub struct WorkerSession {
    // Declaration order also protects unwinding: close native output before
    // original/runtime guards and before potentially blocking channel drop.
    controller: Option<Controller>,
    output: Option<OutputWorker>,
    source: Option<SourceWorker>,
    core: Option<Box<Transport>>,
    channel: Channel,
    gate: WorkerGate,
    runtime: Arc<RuntimeBinding>,
    parent_pid: u32,
    clock: OutputClock,
    hello: bool,
    probe: Option<OutputOperation>,
    probe_deadline_us: Option<u64>,
    shown: bool,
    source_fault: bool,
    fault_reported: bool,
    closing: bool,
    acknowledgements: VecDeque<DisplayAck>,
}

/// Returned only after actual source/output destruction. The channel and
/// runtime guards remain owned. The enclosing owner must still deliver/fence
/// the final report, shut down IPC and observe actual process-tree reaping.
pub struct WorkerClosure {
    pub channel: Channel,
    pub report: Output,
    pub generation: u64,
    _runtime: Arc<RuntimeBinding>,
}

impl WorkerSession {
    pub fn new(
        runtime: Arc<RuntimeBinding>,
        channel: Channel,
        invitation: &Invitation,
        clock: OutputClock,
    ) -> Result<Self> {
        let gate = WorkerGate::new(
            runtime.context(),
            invitation.server_process_id,
            invitation.nonce,
        )?;
        Ok(Self {
            runtime,
            channel,
            gate,
            parent_pid: invitation.server_process_id,
            clock,
            hello: false,
            source: None,
            core: None,
            output: None,
            probe: None,
            probe_deadline_us: None,
            controller: None,
            shown: false,
            source_fault: false,
            fault_reported: false,
            closing: false,
            acknowledgements: VecDeque::with_capacity(8),
        })
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    pub fn observed(&self) -> Option<Snapshot> {
        self.controller.as_ref().map(Controller::observed)
    }
    fn generation(&self) -> u64 {
        self.observed().map_or(0, |snapshot| snapshot.generation)
    }
    fn phase(&self) -> Preparation {
        if self.closing {
            Preparation::Stopped
        } else if self.source_fault {
            Preparation::Fault
        } else if self.core.is_some() {
            Preparation::AwaitingText
        } else {
            Preparation::Checking
        }
    }
    fn send(&mut self, generation: u64, report: Output) -> Result<()> {
        self.channel.send(generation, &report.bytes()?)
    }
    fn fault(&mut self) -> Result<()> {
        self.source_fault = true;
        if let Some(source) = &self.source {
            source.request_stop();
        }
        if let Some(probe) = &self.probe {
            probe.cancel();
        }
        if let Some(controller) = &mut self.controller {
            controller.abort();
        }
        if !self.fault_reported {
            self.fault_reported = true;
            let report = if self.controller.is_some() {
                Output::SessionFault {
                    generation: self.generation(),
                    observed_counter: counter()?,
                }
            } else {
                Output::Preparing {
                    phase: Preparation::Fault,
                    observed_counter: counter()?,
                }
            };
            self.send(self.generation(), report)?;
        }
        Ok(())
    }
    /// Nonblocking queue/controller work. The caller owns the <=20 ms schedule;
    /// no console, native join, file hashing or pipe read/write runs here.
    /// Error requests terminal silence/cleanup; never retry this admission.
    pub fn tick(&mut self) -> Result<()> {
        if self.closing {
            return Ok(());
        }
        let result = self.tick_inner();
        if result.is_err() {
            let _ = self.fault();
            self.gate.end();
            self.closing = true;
        }
        result
    }
    fn tick_inner(&mut self) -> Result<()> {
        // Terminal channel state dominates any previously queued commands.
        ensure!(
            !self.channel.failed() && !self.channel.stopped(),
            "installation worker channel ended"
        );
        if !self.channel.verify_peer(self.parent_pid)? {
            ensure!(
                self.clock.now_us().is_some_and(|us| us < 30_000_000),
                "installation worker peer deadline"
            );
            return Ok(());
        }
        if !self.hello {
            self.send(0, self.gate.hello())?;
            self.hello = true;
        }
        // A peer may refill the queue while we drain it. Bound this batch so
        // even valid Status/AckShown traffic cannot starve native/core ticks.
        for _ in 0..8 {
            let Some(packet) = self.channel.receive() else {
                break;
            };
            ensure!(
                !self.channel.failed() && !self.channel.stopped(),
                "installation worker channel ended"
            );
            // Read QPC after dequeue: a receipt may postdate tick's first sample.
            let admission =
                self.gate
                    .admit(&self.channel, &packet, counter()?, self.clock.origin().0)?;
            self.admission(admission, packet.generation())?;
            if self.closing {
                return Ok(());
            }
        }
        let now = self
            .clock
            .now_us()
            .ok_or_else(|| anyhow::anyhow!("installation worker clock"))?;
        if self.source.is_none() && now >= 30_000_000 {
            anyhow::bail!("installation worker authorization deadline");
        }
        if now >= MAX_SESSION_MS * 1000 {
            self.closing = true;
            if let Some(controller) = &mut self.controller {
                controller.abort();
            }
            return Ok(());
        }
        if self
            .acknowledgements
            .iter()
            .any(|ack| now.saturating_sub(ack.received_us) > 100_000)
        {
            self.fault()?;
        }
        if !self.source_fault
            && ((self.controller.is_none() && self.core.is_none() && now >= 30_000_000)
                || self
                    .probe_deadline_us
                    .is_some_and(|deadline| now >= deadline)
                || self.source.as_ref().is_some_and(SourceWorker::finished))
        {
            self.fault()?;
        }
        if self.controller.is_none() && !self.source_fault {
            if let Some(event) = self.source.as_mut().and_then(SourceWorker::event) {
                match event {
                    SourceEvent::Ready(core) => {
                        let text = self.gate.transcript(core.transcript().to_owned())?;
                        self.core = Some(core);
                        self.send(0, text)?;
                        self.send(
                            0,
                            Output::Preparing {
                                phase: Preparation::AwaitingText,
                                observed_counter: counter()?,
                            },
                        )?;
                    }
                    _ => self.fault()?,
                }
            }
            if self.shown && self.core.is_some() && self.output.is_none() {
                let mut output =
                    OutputWorker::new(self.runtime.request().endpoint.clone(), self.clock)?;
                let deadline = (now + 2_000_000).min(MAX_SESSION_MS * 1000);
                self.probe = Some(output.probe(deadline)?);
                self.probe_deadline_us = Some(deadline);
                self.output = Some(output);
            }
            if let Some(event) = self.output.as_mut().and_then(OutputWorker::event) {
                self.probe = None;
                self.probe_deadline_us = None;
                match event {
                    OutputEvent::Probed { .. } => {
                        let mut controller = Controller::new(
                            self.core.take().unwrap(),
                            self.output.take().unwrap(),
                            self.clock,
                        );
                        controller.require_startup_acknowledgement()?;
                        let snapshot = controller.observed();
                        self.controller = Some(controller);
                        self.send(
                            0,
                            Output::Ready {
                                snapshot,
                                observed_counter: counter()?,
                            },
                        )?;
                    }
                    _ => self.fault()?,
                }
            }
        }
        if let Some(controller) = &mut self.controller {
            controller.tick()?;
            if let Some(source) = &mut self.source {
                controller.drive_source(source)?;
            }
        }
        self.controller_events()?;
        if self
            .observed()
            .is_some_and(|snapshot| snapshot.session_closed)
        {
            self.closing = true;
        }
        Ok(())
    }
    fn admission(&mut self, admission: Admission, generation: u64) -> Result<()> {
        match admission {
            Admission::Authorized => {
                ensure!(
                    self.source.is_none(),
                    "installation repeated source admission"
                );
                // Run origin, not time of authorization, bounds the whole setup.
                self.source = Some(SourceWorker::with_runtime(
                    self.runtime.clone(),
                    self.clock,
                    30_000_000,
                )?);
                self.send(
                    0,
                    Output::Preparing {
                        phase: Preparation::Checking,
                        observed_counter: counter()?,
                    },
                )?;
            }
            Admission::TextShown => {
                ensure!(
                    self.core.is_some() && !self.source_fault,
                    "installation text display after source failure"
                );
                self.shown = true;
            }
            Admission::AckShown {
                id,
                observed_counter,
            } => {
                let index = self
                    .acknowledgements
                    .iter()
                    .position(|ack| ack.id == id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("installation unknown displayed acknowledgement")
                    })?;
                let ack = &self.acknowledgements[index];
                ensure!(
                    ack.generation == generation
                        && ack
                            .reported_counter
                            .is_some_and(|sent| sent <= observed_counter),
                    "installation acknowledgement display precedes report"
                );
                let now = self
                    .clock
                    .now_us()
                    .ok_or_else(|| anyhow::anyhow!("installation worker clock"))?;
                if now.saturating_sub(ack.received_us) > 100_000 {
                    self.fault()?;
                }
                self.acknowledgements.remove(index);
                self.gate.acknowledged(id)?;
                if let Some(controller) = &mut self.controller {
                    controller.acknowledgement_shown(id);
                }
            }
            Admission::Command {
                id,
                command,
                received_counter,
                generation,
            } => {
                self.track(id, received_counter)?;
                if self.controller.is_some() {
                    self.controller.as_mut().unwrap().command(
                        id,
                        command,
                        generation,
                        self.acknowledgements.back().unwrap().received_us,
                    )?;
                    self.controller_events()?;
                } else {
                    let refusal = if matches!(command, Command::Status | Command::Quit) {
                        None
                    } else {
                        Some(TransportError::InvalidState)
                    };
                    self.preparation_ack(id, refusal)?;
                    if command == Command::Quit {
                        self.closing = true;
                        if let Some(source) = &self.source {
                            source.request_stop();
                        }
                    }
                }
            }
            Admission::Refused {
                id,
                received_counter,
                reason,
                ..
            } => {
                self.track(id, received_counter)?;
                self.preparation_ack(id, Some(reason))?;
            }
        }
        Ok(())
    }
    fn track(&mut self, id: u64, receipt: i64) -> Result<()> {
        ensure!(
            self.acknowledgements.len() < 8,
            "installation worker outstanding command bound"
        );
        let received_us = self
            .clock
            .elapsed_at(receipt)
            .ok_or_else(|| anyhow::anyhow!("installation receipt clock"))?;
        self.acknowledgements.push_back(DisplayAck {
            id,
            received_us,
            reported_counter: None,
            generation: 0,
        });
        let now = self
            .clock
            .now_us()
            .ok_or_else(|| anyhow::anyhow!("installation worker clock"))?;
        if now.saturating_sub(received_us) > 100_000 {
            self.fault()?;
        }
        Ok(())
    }
    fn reported(&mut self, id: u64, generation: u64, at: i64) -> Result<()> {
        let ack = self
            .acknowledgements
            .iter_mut()
            .find(|ack| ack.id == id)
            .ok_or_else(|| anyhow::anyhow!("installation untracked actual acknowledgement"))?;
        ensure!(
            ack.reported_counter.is_none(),
            "installation repeated actual acknowledgement"
        );
        ack.reported_counter = Some(at);
        ack.generation = generation;
        Ok(())
    }
    fn preparation_ack(&mut self, id: u64, refusal: Option<TransportError>) -> Result<()> {
        let at = counter()?;
        self.reported(id, 0, at)?;
        self.send(
            0,
            Output::PreparationAck {
                id,
                phase: self.phase(),
                refusal,
                observed_counter: at,
            },
        )
    }
    fn controller_events(&mut self) -> Result<()> {
        while let Some(event) = self.controller.as_mut().and_then(Controller::event) {
            let at = counter()?;
            match event {
                Event::Ack { id, record } => {
                    self.reported(id, record.snapshot.generation, at)?;
                    self.send(
                        record.snapshot.generation,
                        Output::Ack {
                            id,
                            snapshot: record.snapshot,
                            refusal: record.refusal,
                            observed_counter: at,
                        },
                    )?;
                }
                Event::Refused {
                    id,
                    snapshot,
                    reason,
                } => {
                    self.reported(id, snapshot.generation, at)?;
                    self.send(
                        snapshot.generation,
                        Output::Ack {
                            id,
                            snapshot,
                            refusal: Some(reason),
                            observed_counter: at,
                        },
                    )?;
                }
                Event::State(snapshot) => self.send(
                    snapshot.generation,
                    Output::State {
                        snapshot,
                        observed_counter: at,
                    },
                )?,
                Event::Fault(_) => self.fault()?,
            }
        }
        Ok(())
    }
    /// Explicit blocking destruction, never call from the foreground/controller.
    /// A broken join remains inside the externally contained process. No final
    /// message is queued before these real barriers, nor is delivery/reap claimed.
    pub fn finish(mut self) -> Result<WorkerClosure> {
        ensure!(self.closing, "installation worker still active");
        self.gate.end();
        let generation = self.generation();
        let snapshot = self.observed();
        if let Some(source) = &self.source {
            source.request_stop();
        }
        drop(self.controller.take());
        drop(self.output.take());
        drop(self.source.take());
        drop(self.core.take());
        let at = counter()?;
        let report = if let Some(snapshot) = snapshot.filter(|snapshot| snapshot.session_closed) {
            Output::Ended {
                snapshot,
                observed_counter: at,
            }
        } else if snapshot.is_none() {
            Output::PreparationEnded {
                phase: if self.source_fault {
                    Preparation::Fault
                } else {
                    Preparation::Stopped
                },
                observed_counter: at,
            }
        } else {
            Output::ResourcesClosed {
                generation,
                observed_counter: at,
            }
        };
        Ok(WorkerClosure {
            channel: self.channel,
            report,
            generation,
            _runtime: self.runtime,
        })
    }
}

fn counter() -> Result<i64> {
    let mut value = 0;
    unsafe {
        QueryPerformanceCounter(&mut value)?;
    }
    ensure!(value > 0, "installation performance counter unavailable");
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::super::protocol::{Input, ParentGate};
    use super::*;
    use crate::scene_delivery::FileRef;
    use reel_assets::jobs::{QuietEvent, QuietLimits, run_quiet_observed};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    #[test]
    #[ignore = "contained worker fixture; explicitly launched by parent"]
    fn contained_worker_fixture() {
        let raw = std::fs::read("session.json").unwrap();
        let runtime = Arc::new(RuntimeBinding::bind(&raw).unwrap());
        let invitation: Invitation =
            serde_json::from_slice(&std::fs::read("invitation.json").unwrap()).unwrap();
        let (origin, frequency): (i64, i64) =
            serde_json::from_slice(&std::fs::read("clock.json").unwrap()).unwrap();
        let clock = OutputClock::from_origin(origin, frequency).unwrap();
        let channel = Channel::connect(&invitation).unwrap();
        let mut worker = WorkerSession::new(runtime, channel, &invitation, clock).unwrap();
        let mode: u8 = serde_json::from_slice(&std::fs::read("mode.json").unwrap()).unwrap();
        let mut rejected = false;
        let deadline = Instant::now() + Duration::from_secs(12);
        while !worker.closing() {
            if let Err(error) = worker.tick() {
                assert!(
                    mode != 0
                        && error
                            .to_string()
                            .contains("acknowledgement display precedes report")
                );
                rejected = true;
            }
            assert!(worker.controller.is_none() && worker.output.is_none());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(rejected, mode != 0);
        let mut closure = worker.finish().unwrap();
        assert!(matches!(
            closure.report,
            Output::PreparationEnded {
                phase: Preparation::Fault,
                ..
            }
        ));
        closure
            .channel
            .send(closure.generation, &closure.report.bytes().unwrap())
            .unwrap();
        // Test-only delivery fence; production delivery/reaping remains the
        // enclosing owner's job. No timer or queue acceptance is called delivery.
        while !std::path::Path::new("observed.marker").exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        closure.channel.shutdown().unwrap();
    }

    #[test]
    fn actual_contained_worker_authenticates_before_original_failure_and_keeps_status_quit() {
        contained(0);
    }
    #[test]
    fn actual_contained_worker_rejects_wrong_display_ack_generation() {
        contained(1);
    }
    #[test]
    fn actual_contained_worker_rejects_display_claim_before_actual_report() {
        contained(2);
    }
    fn contained(mode: u8) {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("mode.json"),
            serde_json::to_vec(&mode).unwrap(),
        )
        .unwrap();
        let mut request = super::super::session::fixture_request();
        let image = std::env::current_exe().unwrap();
        let identity = crate::media_runtime::ByteIdentity::observe_file(&image).unwrap();
        request.playback.runtime = FileRef {
            path: image,
            sha256: identity.sha256,
            bytes: identity.bytes,
        };
        request.original.declaration.path = root.path().join("missing-original.json");
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
        let clock = OutputClock::start().unwrap();
        std::fs::write(
            root.path().join("clock.json"),
            serde_json::to_vec(&clock.origin()).unwrap(),
        )
        .unwrap();
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
            let child_cancel = cancel.clone();
            let (program, directory, pid) = (binding.runtime_path(), root.path(), &actual_pid);
            let job = scope.spawn(move || run_quiet_observed(program, &[
                "--ignored".into(), "--exact".into(),
                "spoken_programme::installation::native::worker::tests::contained_worker_fixture".into(),
                "--test-threads=1".into(),
            ], directory, Duration::from_secs(16), QuietLimits { memory_bytes: 256 * 1024 * 1024, processes: 1 }, |event| {
                if child_cancel.load(Ordering::Acquire) { return Err("controlled session cancelled".into()); }
                if let QuietEvent::Process(info) = event { pid.store(info.process_id, Ordering::Release); }
                Ok(())
            }).map_err(|error| error.to_string()));
            let _cancel = Cancel(cancel.clone());
            let deadline = Instant::now() + Duration::from_secs(12);
            let mut parent = None;
            let mut status = false;
            let mut quit_ack = false;
            let mut ended = false;
            while !ended {
                assert!(Instant::now() < deadline, "contained worker test deadline");
                let pid = actual_pid.load(Ordering::Acquire);
                if pid == 0 || !channel.verify_peer(pid).unwrap() {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                while let Some(packet) = channel.receive() {
                    match Output::parse(&packet).unwrap() {
                        Output::Hello { .. } => {
                            assert!(parent.is_none());
                            let mut gate =
                                ParentGate::new(binding.context(), pid, invitation.nonce).unwrap();
                            channel
                                .send(
                                    0,
                                    &gate.authorize(&channel, &packet).unwrap().bytes().unwrap(),
                                )
                                .unwrap();
                            parent = Some(gate);
                        }
                        Output::Preparing {
                            phase: Preparation::Fault,
                            ..
                        } => {
                            assert!(parent.is_some());
                            let received_counter = counter().unwrap();
                            channel
                                .send(
                                    0,
                                    &Input::Control {
                                        id: 1,
                                        command: Command::Status,
                                        received_counter,
                                    }
                                    .bytes()
                                    .unwrap(),
                                )
                                .unwrap();
                            if mode == 2 {
                                channel
                                    .send(
                                        0,
                                        &Input::AckShown {
                                            id: 1,
                                            observed_counter: received_counter,
                                        }
                                        .bytes()
                                        .unwrap(),
                                    )
                                    .unwrap();
                            }
                        }
                        Output::Preparing {
                            phase: Preparation::Checking,
                            ..
                        } => assert!(parent.is_some()),
                        Output::PreparationAck {
                            id: 1,
                            phase: Preparation::Fault,
                            refusal: None,
                            ..
                        } => {
                            status = true;
                            if mode == 2 {
                                continue;
                            }
                            channel
                                .send(
                                    u64::from(mode == 1),
                                    &Input::AckShown {
                                        id: 1,
                                        observed_counter: counter().unwrap(),
                                    }
                                    .bytes()
                                    .unwrap(),
                                )
                                .unwrap();
                            if mode == 1 {
                                continue;
                            }
                            channel
                                .send(
                                    0,
                                    &Input::Control {
                                        id: 2,
                                        command: Command::Quit,
                                        received_counter: counter().unwrap(),
                                    }
                                    .bytes()
                                    .unwrap(),
                                )
                                .unwrap();
                        }
                        Output::PreparationAck {
                            id: 2,
                            phase: Preparation::Fault,
                            refusal: None,
                            ..
                        } => {
                            assert!(status);
                            quit_ack = true;
                        }
                        Output::PreparationEnded {
                            phase: Preparation::Fault,
                            ..
                        } => {
                            assert!(status);
                            assert_eq!(quit_ack, mode == 0);
                            std::fs::write(
                                root.path().join("observed.marker"),
                                b"observed after actual native/source teardown",
                            )
                            .unwrap();
                            ended = true;
                        }
                        _ => panic!(
                            "missing originals produced unexpected readiness/transport report"
                        ),
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            let result = job.join().unwrap();
            assert!(result.is_ok(), "{result:?}");
            channel.shutdown().unwrap();
        });
    }
}

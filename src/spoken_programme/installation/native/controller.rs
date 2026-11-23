//! Nonblocking transport/output controller inside the owned native process.
//! Original checks are requested through opaque tickets, never wire successes.
//! The outer session still owns authentication, display, process and I/O gates.
use super::{
    output::{OutputClock, OutputEvent, OutputOperation, OutputSink, OutputStage, OutputWorker},
    *,
};
use crate::spoken_programme::installation::{RestartPermit, RestartTicket};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Backend,
    Deadline,
    Watchdog,
    Queue,
    Ownership,
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Ack {
        id: u64,
        record: Trace,
    },
    Refused {
        id: u64,
        snapshot: Snapshot,
        reason: TransportError,
    },
    State(Snapshot),
    Fault(Failure),
}
struct Pending {
    id: u64,
    command: Command,
    generation: u64,
    received_us: u64,
    sent: bool,
    sequence: Option<u64>,
}
pub struct Controller {
    core: Option<Box<Transport>>,
    callback: Option<CallbackControl>,
    output: OutputWorker,
    sink: Option<OutputSink>,
    operation: Option<OutputOperation>,
    native_generation: Option<u64>,
    open: Option<u64>,
    recheck: Option<u64>,
    check_issued: bool,
    permit: Option<RestartPermit>,
    source_ticket: Option<RestartTicket>,
    close: bool,
    deadline: Option<u64>,
    pending: VecDeque<Pending>,
    events: VecDeque<Event>,
    observed: Snapshot,
    last_cursor_report_us: u64,
    seen_sequence: u64,
    last_callback_us: u64,
    last_id: u64,
    clock: OutputClock,
    failure: Option<Failure>,
    require_startup_ack: bool,
    awaiting_startup_ack: Option<(u64, u64)>,
}
impl Controller {
    /// Takes the actual prepared transport. Construction opens no stream.
    /// All native calls/drop remain on OutputWorker's backend thread. Destruction
    /// joins that worker and therefore belongs inside process containment.
    pub fn new(core: Box<Transport>, output: OutputWorker, clock: OutputClock) -> Self {
        let observed = core.snapshot();
        Self {
            core: Some(core),
            callback: None,
            output,
            sink: None,
            operation: None,
            native_generation: None,
            open: None,
            recheck: None,
            check_issued: false,
            permit: None,
            source_ticket: None,
            close: false,
            deadline: None,
            pending: VecDeque::with_capacity(8),
            events: VecDeque::with_capacity(256),
            observed,
            last_cursor_report_us: 0,
            seen_sequence: observed.command_sequence,
            last_callback_us: 0,
            last_id: 0,
            clock,
            failure: None,
            require_startup_ack: false,
            awaiting_startup_ack: None,
        }
    }
    pub fn observed(&self) -> Snapshot {
        self.observed
    }
    pub fn failed(&self) -> Option<Failure> {
        self.failure
    }
    pub fn event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }
    /// The authenticated worker uses this before accepting any commands.
    /// Startup remains silent until the foreground confirms its actual ack.
    pub fn require_startup_acknowledgement(&mut self) -> Result<()> {
        ensure!(
            self.last_id == 0,
            "installation late acknowledgement policy"
        );
        self.require_startup_ack = true;
        Ok(())
    }
    pub fn acknowledgement_shown(&mut self, id: u64) {
        self.check_ack_deadlines();
        if self.failure.is_none()
            && self
                .awaiting_startup_ack
                .is_some_and(|(waiting, _)| waiting == id)
        {
            self.awaiting_startup_ack = None;
        }
    }
    /// A protocol/lifecycle failure requests silence and physical close without
    /// inventing a command ID or a returned-core observation.
    pub fn abort(&mut self) {
        self.fail(Failure::Backend);
    }
    pub fn command(
        &mut self,
        id: u64,
        command: Command,
        generation: u64,
        received_us: u64,
    ) -> Result<()> {
        let now = self.now()?;
        ensure!(
            id > self.last_id && received_us <= now,
            "installation controller command context"
        );
        if self.pending.len() == 8 {
            self.fail(Failure::Queue);
            anyhow::bail!("installation controller command queue full");
        }
        self.last_id = id;
        if matches!(command, Command::Stop | Command::Quit) {
            self.cancel_startup();
            self.close = true;
        }
        self.pending.push_back(Pending {
            id,
            command,
            generation,
            received_us,
            sent: false,
            sequence: None,
        });
        self.check_ack_deadlines();
        self.dispatch(now);
        Ok(())
    }
    fn now(&self) -> Result<u64> {
        self.clock
            .now_us()
            .ok_or_else(|| anyhow::anyhow!("installation controller clock unavailable"))
    }
    fn emit(&mut self, event: Event) {
        if self.events.len() < 256 {
            self.events.push_back(event);
        } else {
            self.fail(Failure::Queue);
        }
    }
    fn fail(&mut self, reason: Failure) {
        if self.failure.is_none() {
            self.failure = Some(reason);
            if self.events.len() < 256 {
                self.events.push_back(Event::Fault(reason));
            }
        }
        if let Some(callback) = &self.callback {
            callback.fail();
        }
        self.cancel_startup();
        self.close = true;
    }
    fn cancel_startup(&mut self) {
        if let Some(op) = &self.operation {
            if op.stage != OutputStage::Close {
                op.cancel();
            }
        }
        self.open = None;
        self.recheck = None;
        self.check_issued = false;
        self.permit = None;
        self.source_ticket = None;
        self.deadline = None;
        self.awaiting_startup_ack = None;
    }
    fn check_ack_deadlines(&mut self) {
        // Use controller visibility, not the rounded core trace timestamp.
        // Check before consuming a late delivery/record: removing its pending
        // entry must not erase a missed foreground-receipt deadline.
        let expired = self.clock.now_us().is_none_or(|now| {
            self.pending
                .iter()
                .any(|pending| now.saturating_sub(pending.received_us) > 100_000)
                || self
                    .awaiting_startup_ack
                    .is_some_and(|(_, receipt)| now.saturating_sub(receipt) > 100_000)
        });
        if expired {
            self.fail(Failure::Deadline);
        }
    }
    fn effects(&mut self, effects: Effects) {
        if effects.close {
            self.close = true;
        }
        if self.failure.is_some() {
            return;
        }
        if let Some(generation) = effects.recheck {
            self.recheck = Some(generation);
            self.check_issued = false;
            self.open = None;
        }
        if let Some(generation) = effects.open {
            self.open = Some(generation);
        }
        if matches!(self.observed.state, State::Stopped | State::Fault)
            || self.observed.session_closed
        {
            self.cancel_startup();
        }
    }
    fn record(&mut self, record: Trace) {
        self.check_ack_deadlines();
        if record.snapshot.command_sequence <= self.seen_sequence {
            return;
        }
        self.seen_sequence = record.snapshot.command_sequence;
        let Some(index) = self
            .pending
            .iter()
            .position(|pending| pending.sequence == Some(record.snapshot.command_sequence))
        else {
            self.fail(Failure::Ownership);
            return;
        };
        let pending = self.pending.remove(index).unwrap();
        if !pending.sent {
            self.fail(Failure::Ownership);
            return;
        }
        if record.refusal.is_none() && self.failure.is_none() {
            let span = match (pending.command, record.snapshot.state) {
                (Command::Start, State::Starting) => Some(2_000_000),
                (Command::Restart, State::Restarting) => Some(35_000_000),
                _ => None,
            };
            if let Some(span) = span {
                if self.require_startup_ack {
                    self.awaiting_startup_ack = Some((pending.id, pending.received_us));
                }
                self.deadline = Some(
                    pending
                        .received_us
                        .saturating_add(span)
                        .min(MAX_SESSION_MS * 1000),
                );
            }
        }
        self.emit(Event::Ack {
            id: pending.id,
            record,
        });
    }
    fn poll_core(&mut self, now: u64) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let generation = core.snapshot().generation;
        let notice = if self.failure.is_some() {
            Notice::BackendFault { generation }
        } else {
            Notice::None
        };
        let effects = core.poll(now / 1000, notice);
        let mut records = [None; 8];
        let mut count = 0;
        let mut sequence = self.seen_sequence;
        for record in core.trace() {
            if record.snapshot.command_sequence > sequence && count < 8 {
                sequence = record.snapshot.command_sequence;
                records[count] = Some(*record);
                count += 1;
            }
        }
        let snapshot = core.snapshot();
        for record in records.into_iter().flatten() {
            self.record(record);
        }
        if snapshot != self.observed {
            self.observed = snapshot;
            // The acknowledgement already carries this exact snapshot. A
            // second display would immediately overwrite the numbered reply.
            if !matches!(self.events.back(), Some(Event::Ack { record, .. }) if record.snapshot == snapshot)
            {
                self.emit(Event::State(snapshot));
            }
        }
        self.effects(effects);
        if snapshot.session_closed {
            self.check_ack_deadlines();
            while let Some(pending) = self.pending.pop_front() {
                self.emit(Event::Refused {
                    id: pending.id,
                    snapshot,
                    reason: TransportError::InvalidState,
                });
            }
        }
    }
    fn dispatch(&mut self, now: u64) {
        // FIFO also covers commands accepted by a callback immediately before
        // actual ownership return. Never strand an accepted old-queue command.
        if self.core.is_some() {
            self.poll_core(now);
            while let Some(index) = self.pending.iter().position(|pending| !pending.sent) {
                let pending = &mut self.pending[index];
                let result = self
                    .core
                    .as_mut()
                    .unwrap()
                    .enqueue_for_generation(pending.command.as_str(), pending.generation);
                match result {
                    Ok(sequence) => {
                        pending.sent = true;
                        pending.sequence = Some(sequence);
                        self.poll_core(now);
                    }
                    Err(reason) => {
                        self.check_ack_deadlines();
                        let pending = self.pending.remove(index).unwrap();
                        let snapshot = self.core.as_ref().unwrap().snapshot();
                        self.emit(Event::Refused {
                            id: pending.id,
                            snapshot,
                            reason,
                        });
                        self.poll_core(now);
                    }
                }
            }
        } else if let Some(callback) = self.callback.as_mut() {
            for pending in &mut self.pending {
                if !pending.sent {
                    match callback.try_tagged_command(
                        pending.command,
                        pending.generation,
                        pending.id,
                    ) {
                        Ok(()) => pending.sent = true,
                        Err(CallbackCommandError::OwnershipReturned) => break,
                        Err(CallbackCommandError::QueueFull) => {
                            self.fail(Failure::Queue);
                            break;
                        }
                    }
                }
            }
        }
    }
    /// Take a ticket only after the identical core has returned AND the old
    /// physical stream has closed. The outer source worker checks it off-thread.
    pub fn restart_ticket(&mut self) -> Result<Option<RestartTicket>> {
        if self.check_issued
            || self.recheck.is_none()
            || self.core.is_none()
            || self.operation.is_some()
            || self.native_generation.is_some()
        {
            return Ok(None);
        }
        let core = self.core.as_ref().unwrap();
        if core.snapshot().state != State::Restarting {
            return Ok(None);
        }
        let ticket = core.restart_ticket()?;
        self.check_issued = true;
        Ok(Some(ticket))
    }
    pub fn restart_checked(&mut self, permit: RestartPermit) -> Result<()> {
        let core = self
            .core
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("installation recheck without actual owner"))?;
        ensure!(
            self.check_issued
                && self.recheck == Some(permit.generation)
                && core.snapshot().generation == permit.generation
                && core.snapshot().state == State::Restarting,
            "installation stale recheck completion"
        );
        self.open = Some(permit.generation);
        self.permit = Some(permit);
        self.check_issued = false;
        self.recheck = None;
        Ok(())
    }
    pub fn restart_rejected(&mut self, generation: u64) -> Result<()> {
        let now = self.now()?;
        if let Some(core) = self.core.as_mut() {
            let effects = core.poll(now / 1000, Notice::RestartRejected { generation });
            let snapshot = core.snapshot();
            if snapshot != self.observed {
                self.observed = snapshot;
                self.emit(Event::State(snapshot));
            }
            self.effects(effects);
        }
        self.check_issued = false;
        Ok(())
    }
    pub fn has_staged_restart(&self) -> bool {
        self.permit.is_some()
    }
    /// Connect the retained source-check thread without borrowing the core
    /// during blocking work. Obsolete completions are discarded, not replayed.
    pub fn drive_source(&mut self, source: &mut super::source::SourceWorker) -> Result<()> {
        use super::source::SourceEvent;
        let now = self.now()?;
        while let Some(event) = source.event() {
            match event {
                SourceEvent::RestartChecked { generation, permit } => {
                    if self.recheck == Some(generation) && self.observed.state == State::Restarting
                    {
                        self.restart_checked(permit)?;
                    }
                }
                SourceEvent::Failed { generation, .. } => {
                    if self.recheck == Some(generation) && self.observed.state == State::Restarting
                    {
                        self.restart_rejected(generation)?;
                    }
                }
                SourceEvent::Ready(_) => {
                    self.fail(Failure::Ownership);
                }
            }
        }
        if let Some(operation) = source.active_operation() {
            if self.recheck != Some(operation.generation)
                || self.observed.state != State::Restarting
            {
                operation.cancel();
            }
        }
        if self.source_ticket.is_none() {
            self.source_ticket = self.restart_ticket()?;
        }
        if source.active_operation().is_none() {
            if let Some(ticket) = self.source_ticket.take() {
                let deadline = self.deadline.unwrap_or(now).min(now + 30_000_000);
                if source.restart(ticket, deadline).is_err() {
                    self.fail(Failure::Deadline);
                }
            }
        }
        Ok(())
    }
    pub fn tick(&mut self) -> Result<()> {
        let now = self.now()?;
        self.check_ack_deadlines();
        if self.callback.is_some() {
            while let Some(report) = self.callback.as_mut().and_then(CallbackControl::report) {
                self.deliveries();
                self.last_callback_us = report.callback_ms * 1000;
                for record in report.commands.into_iter().flatten() {
                    self.record(record);
                }
                self.observe_callback(report.snapshot, now);
                self.effects(report.effects);
            }
            let returned = self.callback.as_mut().and_then(CallbackControl::returned);
            if let Some(core) = returned {
                self.deliveries();
                self.core = Some(core);
                self.callback = None;
            }
        }
        self.dispatch(now);
        if self.deadline.is_some_and(|deadline| now >= deadline)
            && matches!(self.observed.state, State::Starting | State::Restarting)
        {
            self.fail(Failure::Deadline);
        }
        if self.callback.is_some()
            && matches!(self.observed.state, State::Playing | State::LoopWait)
            && now.saturating_sub(self.last_callback_us) >= 250_000
        {
            self.fail(Failure::Watchdog);
        }
        if self.sink.as_ref().is_some_and(OutputSink::failed) {
            self.fail(Failure::Backend);
        }
        if let Some(event) = self.output.event() {
            self.operation = None;
            match event {
                OutputEvent::Probed { .. } => {
                    if self.open.is_some() && !self.close && self.failure.is_none() {
                        if let Some(permit) = self.permit.take() {
                            let accepted = self
                                .core
                                .as_mut()
                                .map(|core| core.accept_restart(permit, now / 1000));
                            match accepted {
                                Some(Ok(effects)) => self.effects(effects),
                                _ => self.fail(Failure::Ownership),
                            }
                        }
                        if self.failure.is_some() {
                            return Ok(());
                        }
                        let generation = self.open.unwrap();
                        let deadline = self
                            .deadline
                            .unwrap_or(now)
                            .min(now + 2_000_000)
                            .min(MAX_SESSION_MS * 1000);
                        match self.output.open(generation, deadline) {
                            Ok(operation) => self.operation = Some(operation),
                            Err(_) => self.fail(Failure::Deadline),
                        }
                    }
                }
                OutputEvent::Opened {
                    generation, sink, ..
                } => {
                    self.native_generation = Some(generation);
                    self.sink = Some(sink);
                    if self.open != Some(generation) || self.failure.is_some() {
                        self.close = true;
                    }
                }
                OutputEvent::Closed { .. } => {
                    self.native_generation = None;
                    self.sink.take();
                    self.close = false;
                }
                OutputEvent::Failed { stage, reason, .. } => {
                    if stage != OutputStage::Probe {
                        self.native_generation = None;
                        self.sink.take();
                    }
                    if reason != super::output::OutputFailure::Cancelled {
                        self.fail(Failure::Backend);
                    }
                }
            }
        }
        if self.close && self.operation.is_none() {
            if let Some(generation) = self.native_generation {
                let deadline = (now + 2_000_000).min(MAX_SESSION_MS * 1000 + 2_000_000);
                match self.output.close(generation, deadline) {
                    Ok(op) => self.operation = Some(op),
                    Err(_) => self.fail(Failure::Backend),
                }
            } else {
                self.close = false;
                self.sink.take();
            }
        }
        if self.operation.is_none()
            && !self.close
            && self.native_generation.is_none()
            && self.open.is_some()
            && self.failure.is_none()
            && self.awaiting_startup_ack.is_none()
        {
            let deadline = self
                .deadline
                .unwrap_or(now)
                .min(now + 2_000_000)
                .min(MAX_SESSION_MS * 1000);
            match self.output.probe(deadline) {
                Ok(op) => self.operation = Some(op),
                Err(_) => self.fail(Failure::Deadline),
            }
        }
        if !self.close
            && self.operation.is_none()
            && self.callback.is_none()
            && self.open == self.native_generation
            && self.open.is_some()
            && self.sink.as_ref().is_some_and(OutputSink::ready)
        {
            let core = self
                .core
                .take()
                .ok_or_else(|| anyhow::anyhow!("installation output handoff lost actual owner"))?;
            let (mut control, callback) = callback_pair(core);
            match self.sink.as_mut().unwrap().attach(callback) {
                Ok(()) => {
                    self.callback = Some(control);
                    self.open = None;
                    self.last_callback_us = now;
                }
                Err(callback) => {
                    drop(callback);
                    self.core = control.returned().or(self.core.take());
                    self.fail(Failure::Backend);
                }
            }
        }
        self.dispatch(now);
        Ok(())
    }

    fn observe_callback(&mut self, snapshot: Snapshot, now: u64) {
        let previous = std::mem::replace(&mut self.observed, snapshot);
        if snapshot == previous {
            return;
        }
        // The console needs the latest cursor, not one display per audio
        // callback. State, generation, pass and command changes stay immediate.
        let cursor_only = snapshot
            == (Snapshot {
                source_frame: snapshot.source_frame,
                interval_frame: snapshot.interval_frame,
                ..previous
            });
        if !cursor_only || now.saturating_sub(self.last_cursor_report_us) >= 100_000 {
            self.last_cursor_report_us = now;
            self.emit(Event::State(snapshot));
        }
    }
    fn deliveries(&mut self) {
        while let Some(delivery) = self.callback.as_mut().and_then(CallbackControl::delivery) {
            self.check_ack_deadlines();
            let Some(index) = self
                .pending
                .iter()
                .position(|pending| pending.id == delivery.tag)
            else {
                self.fail(Failure::Ownership);
                return;
            };
            if let Some(reason) = delivery.refusal {
                let pending = self.pending.remove(index).unwrap();
                self.emit(Event::Refused {
                    id: pending.id,
                    snapshot: delivery.snapshot,
                    reason,
                });
            } else {
                self.pending[index].sequence = delivery.sequence;
            }
        }
    }
}

impl Controller {
    fn shutdown_and_reclaim(&mut self) -> Result<()> {
        // Keep the return receiver alive while the native closure is actually
        // destroyed. Struct field drop order alone would drop it too early.
        let result = self.output.shutdown();
        if let Some(sink) = self.sink.as_mut() {
            sink.reclaim_closed();
        }
        if let Some(callback) = self.callback.as_mut() {
            if let Some(core) = callback.returned() {
                self.core = Some(core);
            }
        }
        result
    }

    /// Called by worker teardown, after lifetime expiry, outside the control loop.
    pub(super) fn finish_expired(&mut self) -> Result<Snapshot> {
        self.shutdown_and_reclaim()?;
        let now = self.now()?;
        ensure!(
            now >= MAX_SESSION_MS * 1000,
            "installation lifetime has not expired"
        );
        ensure!(
            self.core.is_some(),
            "installation expired transport was not returned"
        );
        self.poll_core(now);
        ensure!(
            self.observed.session_closed,
            "installation expired transport did not close"
        );
        Ok(self.observed)
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        let _ = self.shutdown_and_reclaim();
    }
}

#[cfg(test)]
mod tests {
    use super::super::output::controlled::{Harness, output};
    use super::*;
    use std::sync::atomic::Ordering;
    fn fixture() -> (Controller, Harness, OutputClock) {
        fixture_at(OutputClock::start().unwrap())
    }
    fn fixture_at(clock: OutputClock) -> (Controller, Harness, OutputClock) {
        let binding = super::super::session::fixture_request().endpoint;
        let (output, harness) = output(binding, clock);
        let mut samples = vec![0.0; 20000];
        samples[9700] = 0.125;
        let core = Box::new(
            Transport::new(samples.into(), "Engineering signal\n".into(), [7; 32], 2, 0).unwrap(),
        );
        (Controller::new(core, output, clock), harness, clock)
    }
    #[test]
    fn ready_status_keeps_its_numbered_acknowledgement_as_the_latest_report() {
        let (mut controller, _, clock) = fixture();
        controller
            .command(1, Command::Status, 0, clock.now_us().unwrap())
            .unwrap();
        controller.tick().unwrap();
        let Some(Event::Ack { id: 1, record }) = controller.event() else {
            panic!("expected numbered Status acknowledgement");
        };
        assert_eq!(record.snapshot, controller.observed());
        assert_eq!(record.snapshot.state, State::Ready);
        assert!(controller.event().is_none(), "duplicate Ready report");
    }
    #[test]
    fn expiry_reclaims_a_paused_callback_and_reports_the_real_stopped_core() {
        let (origin, frequency) = OutputClock::start().unwrap().origin();
        let clock = OutputClock::from_origin(origin - frequency * 119, frequency).unwrap();
        let (mut controller, harness, _) = fixture_at(clock);
        controller
            .command(1, Command::Start, 0, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |c| {
            c.observed().state == State::Playing
        });
        controller
            .command(2, Command::Pause, 1, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |c| {
            c.observed().state == State::Paused
        });
        assert!(controller.core.is_none());
        let paused = controller.observed();
        while clock.now_us().unwrap() < MAX_SESSION_MS * 1000 {
            std::thread::sleep(Duration::from_millis(2));
        }
        let closed = controller.finish_expired().unwrap();
        assert_eq!(closed.state, State::Stopped);
        assert!(closed.session_closed);
        assert_eq!(closed.source_frame, paused.source_frame);
        assert_eq!(closed.generation, paused.generation);
    }
    fn wait(
        controller: &mut Controller,
        harness: &Harness,
        mut done: impl FnMut(&Controller) -> bool,
    ) {
        let until = Instant::now() + Duration::from_secs(3);
        while !done(controller) {
            harness.fill(480);
            controller.tick().unwrap();
            while controller.event().is_some() {}
            assert!(Instant::now() < until, "controlled controller deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn foreground_startup_acknowledgement_gates_open_and_cannot_revive_a_timeout() {
        for timely in [true, false] {
            let (mut controller, harness, clock) = fixture();
            let _release = harness.release_guard();
            controller.require_startup_acknowledgement().unwrap();
            controller
                .command(1, Command::Start, 0, clock.now_us().unwrap())
                .unwrap();
            controller.tick().unwrap();
            assert!(controller.operation.is_none());
            assert_eq!(harness.opened.load(Ordering::Acquire), 0);
            controller.acknowledgement_shown(99);
            assert!(controller.awaiting_startup_ack.is_some());
            if !timely {
                std::thread::sleep(Duration::from_millis(120));
            }
            controller.acknowledgement_shown(1);
            if timely {
                wait(&mut controller, &harness, |controller| {
                    controller.observed.state == State::Playing
                });
                assert_eq!(harness.opened.load(Ordering::Acquire), 1);
            } else {
                controller.tick().unwrap();
                assert_eq!(controller.failed(), Some(Failure::Deadline));
                assert_eq!(harness.opened.load(Ordering::Acquire), 0);
                assert_eq!(controller.observed.state, State::Fault);
            }
        }
    }
    #[test]
    fn paused_callback_stall_faults_pending_resume_before_source_can_move() {
        let (mut controller, harness, clock) = fixture();
        let _release = harness.release_guard();
        controller
            .command(1, Command::Start, 0, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Playing
        });
        controller
            .command(2, Command::Pause, 1, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Paused
        });
        let cursor = controller.observed.source_frame;
        controller
            .command(3, Command::Resume, 1, clock.now_us().unwrap())
            .unwrap();
        std::thread::sleep(Duration::from_millis(120));
        controller.tick().unwrap();
        assert_eq!(controller.failed(), Some(Failure::Deadline));
        assert!(harness.fill(480).iter().all(|sample| *sample == 0.0));
        wait(&mut controller, &harness, |controller| {
            controller.core.is_some() && controller.observed.state == State::Fault
        });
        assert_eq!(controller.observed.source_frame, cursor);
        controller
            .command(4, Command::Status, 0, clock.now_us().unwrap())
            .unwrap();
        assert!(matches!(controller.event(), Some(Event::Ack { id: 4, .. })));
        controller
            .command(5, Command::Quit, 0, clock.now_us().unwrap())
            .unwrap();
        assert!(controller.observed.session_closed);
    }
    #[test]
    fn late_callback_ack_does_not_erase_the_receipt_deadline_failure() {
        let (mut controller, harness, clock) = fixture();
        let _release = harness.release_guard();
        controller
            .command(1, Command::Start, 0, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Playing
        });
        controller
            .command(2, Command::Pause, 1, clock.now_us().unwrap())
            .unwrap();
        harness.fill(1);
        std::thread::sleep(Duration::from_millis(120));
        controller.tick().unwrap();
        assert_eq!(controller.failed(), Some(Failure::Deadline));
        assert!(controller.pending.is_empty());
        assert!(matches!(
            controller.event(),
            Some(Event::Fault(Failure::Deadline))
        ));
        assert!(matches!(controller.event(), Some(Event::Ack { id: 2, .. })));
    }
    #[test]
    fn expired_start_receipt_cannot_open_a_stream() {
        let (mut controller, harness, _) = fixture();
        let _release = harness.release_guard();
        std::thread::sleep(Duration::from_millis(2100));
        controller.command(1, Command::Start, 0, 0).unwrap();
        controller.tick().unwrap();
        assert_eq!(controller.failed(), Some(Failure::Deadline));
        assert_eq!(controller.observed.state, State::Fault);
        assert_eq!(controller.observed.source_frame, 0);
        assert_eq!(harness.opened.load(Ordering::Acquire), 0);
    }
    #[test]
    fn faulted_enqueue_refusal_cannot_steal_the_following_status_ack() {
        let (mut controller, harness, clock) = fixture();
        let _release = harness.release_guard();
        controller
            .command(1, Command::Start, 0, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Playing
        });
        controller
            .command(2, Command::Pause, 1, clock.now_us().unwrap())
            .unwrap();
        controller.callback.as_ref().unwrap().fail();
        controller
            .command(3, Command::Status, 0, clock.now_us().unwrap())
            .unwrap();
        harness.fill(1);
        controller.tick().unwrap();
        let mut refused = false;
        let mut status = false;
        while let Some(event) = controller.event() {
            match event {
                Event::Refused {
                    id: 2,
                    reason: TransportError::InvalidState,
                    ..
                } => refused = true,
                Event::Ack { id: 3, record } => {
                    status = true;
                    assert_eq!(record.snapshot.state, State::Fault);
                }
                Event::Ack { id: 2, .. } => panic!("refused command stole another command's trace"),
                _ => {}
            }
        }
        assert!(refused && status && controller.pending.is_empty());
    }
    #[test]
    fn restart_waits_for_actual_close_and_late_source_result_cannot_revive_stop() {
        let (mut controller, harness, clock) = fixture();
        let _release = harness.release_guard();
        let address = &**controller.core.as_ref().unwrap() as *const Transport;
        controller
            .command(1, Command::Start, 0, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Playing
        });
        harness.block_close();
        controller
            .command(2, Command::Restart, 1, clock.now_us().unwrap())
            .unwrap();
        harness.fill(1);
        controller.tick().unwrap();
        assert_eq!(controller.observed.state, State::Restarting);
        assert!(controller.restart_ticket().unwrap().is_none());
        harness.release_close();
        wait(&mut controller, &harness, |controller| {
            controller.native_generation.is_none() && controller.operation.is_none()
        });
        let ticket = controller.restart_ticket().unwrap().unwrap();
        let permit = RestartPermit {
            session: ticket.session,
            generation: ticket.generation,
            seal: ticket.seal,
        };
        controller
            .command(3, Command::Stop, 0, clock.now_us().unwrap())
            .unwrap();
        assert!(controller.restart_checked(permit).is_err());
        for _ in 0..3 {
            controller.tick().unwrap();
        }
        assert_eq!(controller.observed.state, State::Stopped);
        assert_eq!(harness.opened.load(Ordering::Acquire), 1);
        assert_eq!(
            &**controller.core.as_ref().unwrap() as *const Transport,
            address
        );
    }
    #[test]
    fn fresh_restart_reopens_the_same_core_and_watchdog_faults_without_fake_state() {
        let (mut controller, harness, clock) = fixture();
        let _release = harness.release_guard();
        let address = &**controller.core.as_ref().unwrap() as *const Transport;
        controller
            .command(1, Command::Start, 0, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Playing
        });
        controller
            .command(2, Command::Restart, 1, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.core.is_some()
                && controller.native_generation.is_none()
                && controller.operation.is_none()
        });
        let ticket = controller.restart_ticket().unwrap().unwrap();
        controller
            .restart_checked(RestartPermit {
                session: ticket.session,
                generation: ticket.generation,
                seal: ticket.seal,
            })
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Playing && controller.observed.generation == 2
        });
        assert_eq!(harness.opened.load(Ordering::Acquire), 2);
        std::thread::sleep(Duration::from_millis(260));
        controller.tick().unwrap();
        assert_eq!(controller.failed(), Some(Failure::Watchdog));
        let until = Instant::now() + Duration::from_secs(2);
        while controller.core.is_none() {
            controller.tick().unwrap();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(controller.observed.state, State::Fault);
        assert_eq!(
            &**controller.core.as_ref().unwrap() as *const Transport,
            address
        );
    }
    #[test]
    fn startup_ack_precedes_open_and_blocked_open_stop_keeps_the_identical_box() {
        let (mut controller, harness, clock) = fixture();
        let _release = harness.release_guard();
        let address = &**controller.core.as_ref().unwrap() as *const Transport;
        harness.block_open();
        controller
            .command(1, Command::Start, 0, clock.now_us().unwrap())
            .unwrap();
        assert_eq!(harness.opened.load(Ordering::Acquire), 0);
        assert!(
            matches!(controller.event(),Some(Event::Ack{id:1,record}) if record.snapshot.state==State::Starting)
        );
        wait(&mut controller, &harness, |_| {
            harness.opened.load(Ordering::Acquire) == 1
        });
        controller
            .command(2, Command::Stop, 0, clock.now_us().unwrap())
            .unwrap();
        assert_eq!(controller.observed().state, State::Stopped);
        assert_eq!(
            &**controller.core.as_ref().unwrap() as *const Transport,
            address
        );
        assert!(controller.restart_ticket().unwrap().is_none());
        harness.release_open();
        wait(&mut controller, &harness, |controller| {
            controller.operation.is_none() && controller.native_generation.is_none()
        });
        assert_eq!(controller.observed().source_frame, 0);
        assert!(controller.failed().is_none());
    }
    #[test]
    fn source_and_interval_pause_stop_and_actual_close_keep_control_responsive() {
        let (mut controller, harness, clock) = fixture();
        let _release = harness.release_guard();
        controller
            .command(1, Command::Start, 0, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Playing
        });
        controller
            .command(2, Command::Pause, 1, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Paused
        });
        let cursor = controller.observed().source_frame;
        for _ in 0..3 {
            assert!(harness.fill(511).iter().all(|sample| *sample == 0.0));
            controller.tick().unwrap();
        }
        assert_eq!(controller.observed().source_frame, cursor);
        controller
            .command(3, Command::Resume, 1, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::LoopWait
        });
        controller
            .command(4, Command::Pause, 1, clock.now_us().unwrap())
            .unwrap();
        wait(&mut controller, &harness, |controller| {
            controller.observed.state == State::Paused
        });
        let cursor = controller.observed().interval_frame;
        harness.fill(2048);
        controller.tick().unwrap();
        assert_eq!(controller.observed().interval_frame, cursor);
        harness.block_close();
        controller
            .command(5, Command::Stop, 0, clock.now_us().unwrap())
            .unwrap();
        harness.fill(1);
        controller.tick().unwrap();
        assert_eq!(controller.observed().state, State::Stopped);
        controller
            .command(6, Command::Status, 0, clock.now_us().unwrap())
            .unwrap();
        assert!(controller.core.is_some());
        harness.release_close();
        wait(&mut controller, &harness, |controller| {
            controller.operation.is_none() && controller.native_generation.is_none()
        });
        assert_eq!(harness.closed.load(Ordering::Acquire), 1);
    }
}

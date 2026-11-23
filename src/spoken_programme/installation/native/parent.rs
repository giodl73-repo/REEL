//! Foreground protocol/control state. Actual console I/O and process ownership
//! remain separate: display confirmation is an integration boundary, not proof
//! that any particular console has rendered these observations.
use super::{
    console::ConsoleRenderer,
    input::{CommandLease, CommandPool, QueuedCommand},
    output::OutputClock,
    pipe::Channel,
    protocol::{Input, Output, ParentGate, Preparation, RuntimeBinding},
    wire::Packet,
    *,
};
use std::{collections::VecDeque, sync::Arc};
use windows::Win32::System::Performance::QueryPerformanceCounter;

#[derive(Clone, Copy)]
pub struct DisplayTicket(u64);
#[cfg(test)]
impl DisplayTicket {
    pub(super) fn fixture() -> Self {
        Self(1)
    }
}
pub struct DisplayItem {
    pub ticket: DisplayTicket,
    pub report: Output,
    /// A late historical observation cannot clear an outer Fault notice.
    pub fault_overlay: bool,
}
pub enum ParentEvent {
    Display(DisplayItem),
    Fault,
    LocalStatus(Option<Snapshot>),
    QuitRequested,
}
#[derive(Clone, Copy)]
pub struct LayoutObservation {
    pub columns: u16,
    pub rows: u16,
    pub input_code_page: u32,
    pub output_code_page: u32,
}
struct Credit {
    lease: CommandLease,
    id: u64,
    command: Command,
    received_us: u64,
    received_counter: i64,
    expected_generation: u64,
    reported: Option<(u64, i64)>,
    shown: Option<i64>,
}
enum DisplayKind {
    Transcript(String),
    Ack(u64, u64),
    Final,
    Observation,
}
struct PendingDisplay {
    ticket: u64,
    kind: DisplayKind,
}

/// Owns both bounded foreground queues and the actual local channel. The PID
/// supplied here must come from the existing held process owner, not a child
/// report. Construction performs no console I/O, originals check or device work.
pub struct ParentSession {
    input: CommandPool,
    channel: Channel,
    gate: ParentGate,
    _runtime: Arc<RuntimeBinding>,
    worker_pid: u32,
    nonce: [u8; 16],
    clock: OutputClock,
    authorized: bool,
    transcript: bool,
    text_shown: bool,
    active: bool,
    worker_fault: bool,
    protocol_fault: bool,
    final_seen: bool,
    final_displayed: bool,
    quit: bool,
    observed: Option<Snapshot>,
    last_counter: i64,
    last_receipt: i64,
    last_id: u64,
    last_ticket: u64,
    credits: VecDeque<Credit>,
    displays: VecDeque<PendingDisplay>,
    events: VecDeque<ParentEvent>,
}
impl ParentSession {
    pub fn new(
        runtime: Arc<RuntimeBinding>,
        channel: Channel,
        nonce: [u8; 16],
        worker_pid: u32,
        clock: OutputClock,
    ) -> Result<Self> {
        Ok(Self {
            input: CommandPool::new(),
            gate: ParentGate::new(runtime.context(), worker_pid, nonce)?,
            channel,
            _runtime: runtime,
            worker_pid,
            nonce,
            clock,
            authorized: false,
            transcript: false,
            text_shown: false,
            active: false,
            worker_fault: false,
            protocol_fault: false,
            final_seen: false,
            final_displayed: false,
            quit: false,
            observed: None,
            last_counter: 0,
            last_receipt: 0,
            last_id: 0,
            last_ticket: 0,
            credits: VecDeque::with_capacity(8),
            displays: VecDeque::with_capacity(256),
            events: VecDeque::with_capacity(256),
        })
    }
    pub fn event(&mut self) -> Option<ParentEvent> {
        let mut event = self.events.pop_front()?;
        if let ParentEvent::Display(item) = &mut event {
            item.fault_overlay = self.failed();
        }
        Some(event)
    }
    pub fn observed(&self) -> Option<Snapshot> {
        self.observed
    }
    pub fn outstanding(&self) -> usize {
        self.credits.len()
    }
    /// Give the cooked reader this same pool before creating its input queue.
    pub fn command_pool(&self) -> CommandPool {
        self.input.clone()
    }
    pub fn final_displayed(&self) -> bool {
        self.final_displayed
    }
    pub fn failed(&self) -> bool {
        self.protocol_fault || self.worker_fault
    }
    pub fn quit_requested(&self) -> bool {
        self.quit
    }
    fn emit(&mut self, event: ParentEvent) -> Result<()> {
        ensure!(self.events.len() < 256, "installation parent event bound");
        self.events.push_back(event);
        Ok(())
    }
    fn fault(&mut self) {
        if !self.protocol_fault {
            self.protocol_fault = true;
            self.input.fault();
            self.gate.end();
            self.displays.clear();
            self.events.clear();
            self.events.push_back(ParentEvent::Fault);
        }
    }
    fn send(&mut self, generation: u64, input: Input) -> Result<()> {
        self.channel.send(generation, &input.bytes()?)
    }
    /// Called after complete ASCII line receipt; its original QPC counter is
    /// preserved. Invalid syntax does not mutate protocol/transport state.
    pub fn line(&mut self, line: &str, received_counter: i64) -> Result<Option<u64>> {
        ensure!(
            line.is_ascii() && line.len() <= 32,
            "installation command line profile"
        );
        self.command(Command::parse(line)?, received_counter)
    }
    pub fn command(&mut self, command: Command, received_counter: i64) -> Result<Option<u64>> {
        if self.input.failed() {
            self.fault();
        }
        if self.local_command(command)? {
            return Ok(None);
        }
        self.command_available(command)?;
        let reserved = match self.input.command(command, received_counter) {
            Ok(reserved) => reserved,
            Err(error) => {
                self.fault();
                return Err(error.into());
            }
        };
        self.accept(reserved)
    }
    /// Adopts the exact input reservation, receipt and generation; never
    /// reserves a second slot or retags a queued line for a newer generation.
    pub fn accept(&mut self, reserved: QueuedCommand) -> Result<Option<u64>> {
        let (command, received_counter, generation, lease) = match reserved.unpack(&self.input) {
            Ok(parts) => parts,
            Err(error) => {
                self.fault();
                return Err(error.into());
            }
        };
        if self.input.failed() {
            self.fault();
        }
        if self.local_command(command)? {
            return Ok(None);
        }
        self.command_available(command)?;
        let result = self.submit(command, received_counter, generation, lease);
        if result.is_err() {
            self.fault();
        }
        result.map(Some)
    }
    fn local_command(&mut self, command: Command) -> Result<bool> {
        if command == Command::Status
            && (!self.authorized
                || self.protocol_fault
                || self.final_seen
                || self.quit
                || self.input.closed())
        {
            self.emit(ParentEvent::LocalStatus(self.observed))?;
            return Ok(true);
        }
        if command == Command::Quit
            && (!self.authorized
                || self.protocol_fault
                || self.final_seen
                || self.quit
                || self.input.closed())
        {
            self.quit = true;
            self.emit(ParentEvent::QuitRequested)?;
            return Ok(true);
        }
        Ok(false)
    }
    fn command_available(&self, command: Command) -> Result<()> {
        ensure!(
            self.authorized
                && !self.final_seen
                && !self.protocol_fault
                && !self.quit
                && !self.input.closed(),
            "installation foreground command unavailable"
        );
        ensure!(
            !self.worker_fault || matches!(command, Command::Status | Command::Quit),
            "installation foreground terminal Fault"
        );
        Ok(())
    }
    fn submit(
        &mut self,
        command: Command,
        received_counter: i64,
        generation: u64,
        lease: CommandLease,
    ) -> Result<u64> {
        let now = counter()?;
        ensure!(
            received_counter >= self.clock.origin().0
                && received_counter >= self.last_receipt
                && received_counter <= now,
            "installation foreground receipt context"
        );
        ensure!(
            self.credits.len() < 8,
            "installation foreground eight-command bound"
        );
        let received_us = self
            .clock
            .elapsed_at(received_counter)
            .ok_or_else(|| anyhow::anyhow!("installation foreground receipt clock"))?;
        ensure!(
            self.clock
                .now_us()
                .is_some_and(|now| now.saturating_sub(received_us) <= 100_000),
            "installation foreground receipt expired"
        );
        let id = self
            .last_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("installation command ID exhausted"))?;
        self.send(
            generation,
            Input::Control {
                id,
                command,
                received_counter,
            },
        )?;
        self.last_id = id;
        self.last_receipt = received_counter;
        self.credits.push_back(Credit {
            lease,
            id,
            command,
            received_us,
            received_counter,
            expected_generation: generation,
            reported: None,
            shown: None,
        });
        if command == Command::Quit {
            self.quit = true;
        }
        Ok(id)
    }
    /// Poll at the foreground's <=20 ms cadence. No blocking reads/writes,
    /// hashing of files, rendering or cleanup joins occur on this path.
    pub fn tick(&mut self) -> Result<()> {
        if self.protocol_fault || self.final_seen {
            return Ok(());
        }
        let result = self.poll();
        if result.is_err() {
            self.fault();
        }
        result
    }
    fn poll(&mut self) -> Result<()> {
        ensure!(!self.input.failed(), "installation foreground input failed");
        self.deadlines()?;
        ensure!(
            !self.channel.failed() && !self.channel.stopped(),
            "installation foreground channel ended"
        );
        if !self.channel.verify_peer(self.worker_pid)? {
            ensure!(
                self.clock.now_us().is_some_and(|now| now < 30_000_000),
                "installation foreground peer deadline"
            );
            return Ok(());
        }
        for _ in 0..8 {
            ensure!(
                !self.channel.failed() && !self.channel.stopped(),
                "installation foreground channel ended"
            );
            let Some(packet) = self.channel.receive() else {
                break;
            };
            if !self.authorized {
                let input = self.gate.authorize(&self.channel, &packet)?;
                self.send(0, input)?;
                self.authorized = true;
            } else {
                self.report(&packet)?;
            }
            if self.final_seen {
                break;
            }
        }
        self.deadlines()
    }
    fn deadlines(&self) -> Result<()> {
        let receipts = self.input.unshown_receipts();
        let now_counter = counter()?;
        let frequency = self.clock.origin().1 as u128;
        ensure!(
            receipts.into_iter().all(|receipt| receipt == 0
                || (receipt <= now_counter as u64
                    && (now_counter as u128 - receipt as u128) * 1_000_000 <= frequency * 100_000)),
            "installation queued input acknowledgement deadline"
        );
        let now = self
            .clock
            .now_us()
            .ok_or_else(|| anyhow::anyhow!("installation foreground clock"))?;
        ensure!(
            !self
                .credits
                .iter()
                .any(|credit| credit.shown.is_none()
                    && now.saturating_sub(credit.received_us) > 100_000),
            "installation foreground acknowledgement deadline"
        );
        ensure!(
            now < MAX_SESSION_MS * 1000 || self.quit,
            "installation foreground lifetime reached"
        );
        Ok(())
    }
    fn observation_counter(&mut self, value: i64) -> Result<()> {
        ensure!(
            value >= self.clock.origin().0 && value >= self.last_counter && value <= counter()?,
            "installation worker observation counter differs"
        );
        self.last_counter = value;
        Ok(())
    }
    fn snapshot(&mut self, snapshot: Snapshot) -> Result<()> {
        let previous_generation = self.observed.map_or(0, |value| value.generation);
        ensure!(
            snapshot.pass <= self._runtime.request().passes
                && snapshot.generation >= previous_generation
                && snapshot.generation <= previous_generation.saturating_add(1),
            "installation foreground snapshot context"
        );
        if snapshot.generation > previous_generation {
            ensure!(
                self.credits.iter().any(|credit| matches!(
                    credit.command,
                    Command::Start | Command::Restart
                ) && credit.expected_generation
                    == previous_generation),
                "installation unsolicited stream generation"
            );
        }
        if let Some(previous) = self.observed {
            ensure!(
                snapshot.command_sequence >= previous.command_sequence,
                "installation worker command sequence reversed"
            );
        }
        ensure!(
            snapshot.state != State::Ready
                || (snapshot.generation == 0
                    && snapshot.source_frame == 0
                    && snapshot.interval_frame == 0
                    && !snapshot.session_closed),
            "installation spurious Ready snapshot"
        );
        if snapshot.state == State::Fault {
            self.worker_fault = true;
        }
        self.observed = Some(snapshot);
        self.input.publish_generation(snapshot.generation);
        Ok(())
    }
    fn report(&mut self, packet: &Packet) -> Result<()> {
        ensure!(
            packet.as_bytes()[8..24] == self.nonce,
            "installation foreground run context differs"
        );
        let report = Output::parse(packet)?;
        let kind = match &report {
            Output::Hello { .. } => anyhow::bail!("installation repeated worker Hello"),
            Output::Transcript { sha256, .. } => {
                ensure!(
                    !self.transcript && !self.active && !self.worker_fault,
                    "installation transcript order"
                );
                self.transcript = true;
                DisplayKind::Transcript(sha256.clone())
            }
            Output::Preparing {
                phase,
                observed_counter,
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(
                    !self.active && !matches!(phase, Preparation::Stopped),
                    "installation preparation order"
                );
                if matches!(phase, Preparation::AwaitingText) {
                    ensure!(self.transcript, "installation missing transcript");
                }
                if matches!(phase, Preparation::Fault) {
                    self.worker_fault = true;
                } else {
                    ensure!(
                        !self.worker_fault,
                        "installation preparation revived after Fault"
                    );
                }
                DisplayKind::Observation
            }
            Output::Ready {
                snapshot,
                observed_counter,
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(
                    !self.active && self.text_shown && !self.worker_fault,
                    "installation Ready before display or after Fault"
                );
                self.snapshot(*snapshot)?;
                self.active = true;
                DisplayKind::Observation
            }
            Output::State {
                snapshot,
                observed_counter,
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(self.active, "installation state before Ready");
                self.snapshot(*snapshot)?;
                DisplayKind::Observation
            }
            Output::Ack {
                id,
                snapshot,
                observed_counter,
                ..
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(
                    self.active,
                    "installation core acknowledgement before Ready"
                );
                self.check_ack(*id, *observed_counter)?;
                self.snapshot(*snapshot)?;
                self.ack(*id, snapshot.generation, *observed_counter)?;
                DisplayKind::Ack(*id, snapshot.generation)
            }
            Output::PreparationAck {
                id,
                phase,
                observed_counter,
                ..
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(
                    !self.active,
                    "installation preparation acknowledgement after Ready"
                );
                if matches!(phase, Preparation::Fault) {
                    self.worker_fault = true;
                }
                self.ack(*id, 0, *observed_counter)?;
                DisplayKind::Ack(*id, 0)
            }
            Output::CreditReleased {
                id,
                generation,
                observed_counter,
            } => {
                self.observation_counter(*observed_counter)?;
                let index = self
                    .credits
                    .iter()
                    .position(|credit| credit.id == *id)
                    .ok_or_else(|| anyhow::anyhow!("installation unknown command credit"))?;
                let credit = &self.credits[index];
                ensure!(
                    credit
                        .reported
                        .is_some_and(|(reported_generation, _)| reported_generation == *generation)
                        && credit.shown.is_some_and(|shown| shown <= *observed_counter),
                    "installation command credit before display"
                );
                self.credits.remove(index);
                return Ok(());
            }
            Output::SessionFault {
                generation,
                observed_counter,
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(
                    self.active
                        && *generation >= self.observed.map_or(0, |s| s.generation)
                        && *generation
                            <= self.observed.map_or(0, |s| s.generation).saturating_add(1),
                    "installation Fault generation context"
                );
                let current = self.observed.map_or(0, |snapshot| snapshot.generation);
                ensure!(
                    *generation == current
                        || self.credits.iter().any(|credit| matches!(
                            credit.command,
                            Command::Start | Command::Restart
                        ) && credit.expected_generation
                            == current),
                    "installation unsolicited Fault generation"
                );
                self.worker_fault = true;
                DisplayKind::Observation
            }
            Output::Ended {
                snapshot,
                observed_counter,
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(self.active, "installation core end before Ready");
                self.snapshot(*snapshot)?;
                self.final_report()?;
                DisplayKind::Final
            }
            Output::PreparationEnded {
                phase,
                observed_counter,
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(
                    !self.active && (matches!(phase, Preparation::Fault) || !self.worker_fault),
                    "installation preparation end context"
                );
                self.final_report()?;
                DisplayKind::Final
            }
            Output::ResourcesClosed {
                generation,
                observed_counter,
            } => {
                self.observation_counter(*observed_counter)?;
                ensure!(
                    self.active && Some(*generation) == self.observed.map(|s| s.generation),
                    "installation resource closure context"
                );
                self.final_report()?;
                DisplayKind::Final
            }
        };
        self.last_ticket = self
            .last_ticket
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("installation display ticket exhausted"))?;
        ensure!(
            self.displays.len() < 256,
            "installation foreground display bound"
        );
        self.displays.push_back(PendingDisplay {
            ticket: self.last_ticket,
            kind,
        });
        self.emit(ParentEvent::Display(DisplayItem {
            ticket: DisplayTicket(self.last_ticket),
            report,
            fault_overlay: self.failed(),
        }))
    }
    fn ack(&mut self, id: u64, generation: u64, at: i64) -> Result<()> {
        self.check_ack(id, at)?;
        let credit = self
            .credits
            .iter_mut()
            .find(|credit| credit.id == id)
            .ok_or_else(|| anyhow::anyhow!("installation unsolicited command acknowledgement"))?;
        credit.reported = Some((generation, at));
        Ok(())
    }
    fn check_ack(&self, id: u64, at: i64) -> Result<()> {
        let credit = self
            .credits
            .iter()
            .find(|credit| credit.id == id)
            .ok_or_else(|| anyhow::anyhow!("installation unsolicited command acknowledgement"))?;
        ensure!(
            credit.reported.is_none() && at >= credit.received_counter,
            "installation duplicate or pre-receipt command acknowledgement"
        );
        Ok(())
    }
    fn final_report(&mut self) -> Result<()> {
        ensure!(
            self.quit
                || self
                    .clock
                    .now_us()
                    .is_some_and(|now| now >= MAX_SESSION_MS * 1000),
            "installation unsolicited worker closure"
        );
        self.final_seen = true;
        Ok(())
    }
    /// Integration callback AFTER actual rendering, in event order. Transcript
    /// requires the real layout/code-page observation. No I/O is performed here.
    pub fn displayed(
        &mut self,
        ticket: DisplayTicket,
        layout: Option<LayoutObservation>,
    ) -> Result<()> {
        let result = self.confirm_display(ticket, layout);
        if result.is_err() {
            self.fault();
        }
        result
    }
    /// Drive protocol and console output without blocking the foreground loop.
    /// Local Fault, Status and Quit events remain available to the owner.
    pub fn tick_with_renderer(&mut self, renderer: &mut ConsoleRenderer) -> Result<()> {
        self.tick()?;
        if self.protocol_fault {
            return Ok(());
        }
        let result = self.advance_renderer(renderer);
        if result.is_err() {
            self.fault();
        }
        result
    }
    fn advance_renderer(&mut self, renderer: &mut ConsoleRenderer) -> Result<()> {
        if let Some((ticket, layout)) = renderer.completion()? {
            self.displayed(ticket, layout)?;
        }
        if !renderer.pending()
            && matches!(self.events.front(), Some(ParentEvent::Display(_)))
            && let Some(ParentEvent::Display(item)) = self.event()
        {
            renderer.submit(item)?;
        }
        Ok(())
    }
    /// Blocking IPC destruction only, after the enclosing owner has arranged
    /// final delivery/reaping or explicit cancellation. Not a process receipt.
    pub fn shutdown_channel(&mut self) -> Result<()> {
        ensure!(
            self.final_displayed || (self.protocol_fault && self.quit),
            "installation foreground still active"
        );
        self.channel.shutdown()
    }
    fn confirm_display(
        &mut self,
        ticket: DisplayTicket,
        layout: Option<LayoutObservation>,
    ) -> Result<()> {
        ensure!(
            !self.protocol_fault,
            "installation display after protocol failure"
        );
        self.deadlines()?;
        ensure!(
            self.displays
                .front()
                .is_some_and(|display| display.ticket == ticket.0),
            "installation display receipt order/replay"
        );
        let display = self.displays.pop_front().unwrap();
        match display.kind {
            DisplayKind::Transcript(sha256) => {
                let layout = layout
                    .ok_or_else(|| anyhow::anyhow!("installation transcript layout missing"))?;
                self.send(
                    0,
                    Input::TextShown {
                        transcript_sha256: sha256,
                        columns: layout.columns,
                        rows: layout.rows,
                        input_code_page: layout.input_code_page,
                        output_code_page: layout.output_code_page,
                    },
                )?;
                self.text_shown = true;
            }
            DisplayKind::Ack(id, generation) => {
                let at = counter()?;
                self.send(
                    generation,
                    Input::AckShown {
                        id,
                        observed_counter: at,
                    },
                )?;
                let credit = self
                    .credits
                    .iter_mut()
                    .find(|credit| credit.id == id)
                    .ok_or_else(|| anyhow::anyhow!("installation lost display command credit"))?;
                credit.shown = Some(at);
                credit.lease.mark_shown();
            }
            DisplayKind::Final => {
                self.final_displayed = true;
                self.input.close();
                self.credits.clear();
            }
            DisplayKind::Observation => {}
        }
        Ok(())
    }
}
fn counter() -> Result<i64> {
    let mut at = 0;
    unsafe {
        QueryPerformanceCounter(&mut at)?;
    }
    ensure!(at > 0, "installation foreground counter unavailable");
    Ok(at)
}
impl Drop for ParentSession {
    fn drop(&mut self) {
        self.input.close();
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::{protocol::WorkerGate, session::fixture_request};
    use super::*;
    use crate::scene_delivery::FileRef;
    pub(crate) struct Fixture {
        pub(crate) parent: ParentSession,
        pub(crate) peer: Channel,
        _root: tempfile::TempDir,
    }
    pub(crate) fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let mut request = fixture_request();
        let path = std::env::current_exe().unwrap();
        let identity = crate::media_runtime::ByteIdentity::observe_file(&path).unwrap();
        request.playback.runtime = FileRef {
            path,
            sha256: identity.sha256,
            bytes: identity.bytes,
        };
        let lock = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"));
        let path = root.path().join("retained.lock");
        std::fs::write(&path, lock).unwrap();
        request.playback.cargo_lock = FileRef {
            path,
            sha256: crate::production::sha256_bytes(lock),
            bytes: lock.len() as u64,
        };
        let runtime =
            Arc::new(RuntimeBinding::bind(&serde_json::to_vec(&request).unwrap()).unwrap());
        let (invitation, channel) = Channel::server().unwrap();
        let mut peer = Channel::connect(&invitation).unwrap();
        let hello = WorkerGate::new(runtime.context(), std::process::id(), invitation.nonce)
            .unwrap()
            .hello();
        let mut parent = ParentSession::new(
            runtime,
            channel,
            invitation.nonce,
            std::process::id(),
            OutputClock::start().unwrap(),
        )
        .unwrap();
        peer.send(0, &hello.bytes().unwrap()).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while !parent.authorized {
            parent.tick().unwrap();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(matches!(
            Input::parse(&receive(&mut peer)).unwrap(),
            Input::Authorize { .. }
        ));
        Fixture {
            parent,
            peer,
            _root: root,
        }
    }
    pub(crate) fn receive(peer: &mut Channel) -> Packet {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(packet) = peer.receive() {
                return packet;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn deliver(f: &mut Fixture, generation: u64, report: Output) -> Result<()> {
        let next = f.parent.last_ticket;
        let count = f.parent.outstanding();
        f.peer.send(generation, &report.bytes()?).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            f.parent.tick()?;
            if f.parent.last_ticket != next || f.parent.outstanding() != count {
                return Ok(());
            }
            ensure!(Instant::now() < until, "parent test report deadline");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn item(f: &mut Fixture) -> DisplayItem {
        match f.parent.event().unwrap() {
            ParentEvent::Display(item) => item,
            _ => panic!("missing display item"),
        }
    }
    fn ready_snapshot() -> Snapshot {
        Snapshot {
            state: State::Ready,
            generation: 0,
            source_frame: 0,
            interval_frame: 0,
            pass: 1,
            command_sequence: 0,
            session_closed: false,
            fault: None,
        }
    }
    fn ready(f: &mut Fixture) {
        let text = "Señal de ingeniería\n";
        deliver(
            f,
            0,
            Output::Transcript {
                text: text.into(),
                sha256: crate::production::sha256_bytes(text.as_bytes()),
            },
        )
        .unwrap();
        let display = item(f);
        f.parent
            .displayed(
                display.ticket,
                Some(LayoutObservation {
                    columns: 80,
                    rows: 30,
                    input_code_page: 65001,
                    output_code_page: 65001,
                }),
            )
            .unwrap();
        assert!(matches!(
            Input::parse(&receive(&mut f.peer)).unwrap(),
            Input::TextShown { .. }
        ));
        deliver(
            f,
            0,
            Output::Ready {
                snapshot: ready_snapshot(),
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        let display = item(f);
        f.parent.displayed(display.ticket, None).unwrap();
    }
    fn status_ack(f: &mut Fixture, id: u64) -> DisplayItem {
        deliver(
            f,
            0,
            Output::PreparationAck {
                id,
                phase: Preparation::Checking,
                refusal: None,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        item(f)
    }
    #[test]
    fn an_envelope_taken_before_input_closure_cannot_be_forwarded_afterward() {
        let mut f = fixture();
        let pool = f.parent.command_pool();
        let (mut input, mut queued) = pool.channel();
        input.line("start", counter().unwrap()).unwrap();
        let reserved = queued.receive().unwrap();
        queued.close();
        assert!(f.parent.accept(reserved).is_err());
        assert_eq!((pool.occupied(), f.parent.last_id), (0, 0));
        assert!(f.peer.receive().is_none());
        assert_eq!(
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap(),
            None
        );
        assert_eq!(
            f.parent.command(Command::Quit, counter().unwrap()).unwrap(),
            None
        );
    }
    #[test]
    fn the_watchdog_covers_a_queued_line_even_when_no_consumer_adopts_it() {
        let mut f = fixture();
        let pool = f.parent.command_pool();
        let (mut input, mut queued) = pool.channel();
        input.line("stop", counter().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert!(f.parent.tick().is_err());
        assert!(queued.receive().is_none());
        assert_eq!((pool.occupied(), f.parent.last_id), (0, 0));
        assert!(f.parent.protocol_fault && f.peer.receive().is_none());
    }
    #[test]
    fn displayed_reservations_keep_capacity_without_restarting_the_display_deadline() {
        let mut f = fixture();
        let pool = f.parent.command_pool();
        f.parent
            .command(Command::Status, counter().unwrap())
            .unwrap();
        receive(&mut f.peer);
        let display = status_ack(&mut f, 1);
        f.parent.displayed(display.ticket, None).unwrap();
        receive(&mut f.peer);
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(pool.occupied(), 1);
        deliver(
            &mut f,
            0,
            Output::CreditReleased {
                id: 1,
                generation: 0,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        assert_eq!(pool.occupied(), 0);
        assert!(!f.parent.failed());
    }
    #[test]
    fn queued_and_forwarded_lines_share_the_exact_same_eight_reservations() {
        let mut f = fixture();
        let pool = f.parent.command_pool();
        let (mut input, mut queued) = pool.channel();
        for _ in 0..4 {
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap();
            receive(&mut f.peer);
        }
        for _ in 0..4 {
            input.line("status", counter().unwrap()).unwrap();
        }
        assert_eq!((pool.occupied(), f.parent.outstanding()), (8, 4));
        for _ in 0..4 {
            f.parent.accept(queued.receive().unwrap()).unwrap();
            receive(&mut f.peer);
            assert_eq!(pool.occupied(), 8);
        }
        let display = status_ack(&mut f, 1);
        f.parent.displayed(display.ticket, None).unwrap();
        receive(&mut f.peer);
        assert_eq!(pool.occupied(), 8);
        deliver(
            &mut f,
            0,
            Output::CreditReleased {
                id: 1,
                generation: 0,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        assert_eq!(pool.occupied(), 7);
        input.line("status", counter().unwrap()).unwrap();
        f.parent.accept(queued.receive().unwrap()).unwrap();
        assert_eq!((pool.occupied(), f.parent.outstanding()), (8, 8));
    }
    #[test]
    fn input_overflow_faults_the_parent_before_queued_commands_are_forwarded() {
        let mut f = fixture();
        let pool = f.parent.command_pool();
        let (mut input, mut queued) = pool.channel();
        for _ in 0..4 {
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap();
            receive(&mut f.peer);
        }
        for _ in 0..4 {
            input.line("start", counter().unwrap()).unwrap();
        }
        assert!(input.line("start", counter().unwrap()).is_err());
        assert!(f.parent.tick().is_err());
        assert!(queued.receive().is_none());
        assert_eq!((pool.occupied(), f.parent.last_id), (4, 4));
        assert!(matches!(f.parent.event(), Some(ParentEvent::Fault)));
        assert_eq!(
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap(),
            None
        );
        assert_eq!(
            f.parent.command(Command::Quit, counter().unwrap()).unwrap(),
            None
        );
    }
    #[test]
    fn queued_generation_and_receipt_are_not_retagged_for_new_parent_observation() {
        let mut f = fixture();
        ready(&mut f);
        f.parent
            .command(Command::Start, counter().unwrap())
            .unwrap();
        receive(&mut f.peer);
        let pool = f.parent.command_pool();
        let (mut input, mut queued) = pool.channel();
        let receipt = counter().unwrap();
        input.line("pause", receipt).unwrap();
        let mut snapshot = ready_snapshot();
        snapshot.state = State::Starting;
        snapshot.generation = 1;
        snapshot.command_sequence = 1;
        deliver(
            &mut f,
            1,
            Output::Ack {
                id: 1,
                snapshot,
                refusal: None,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        f.parent.accept(queued.receive().unwrap()).unwrap();
        let packet = receive(&mut f.peer);
        assert_eq!(packet.generation(), 0);
        assert!(
            matches!(Input::parse(&packet).unwrap(), Input::Control { id: 2, command: Command::Pause, received_counter } if received_counter == receipt)
        );
        assert_eq!(pool.occupied(), 2);
    }
    #[test]
    fn foreign_input_reservations_cannot_enter_a_parent_session() {
        let mut f = fixture();
        let foreign = CommandPool::new();
        let reserved = foreign.command(Command::Start, counter().unwrap()).unwrap();
        assert!(f.parent.accept(reserved).is_err());
        assert_eq!(
            (
                foreign.occupied(),
                f.parent.command_pool().occupied(),
                f.parent.last_id
            ),
            (0, 0, 0)
        );
        assert!(f.parent.protocol_fault);
        assert!(f.peer.receive().is_none());
    }
    #[test]
    fn queued_time_is_included_in_the_original_receipt_budget() {
        let mut f = fixture();
        let pool = f.parent.command_pool();
        let (mut input, mut queued) = pool.channel();
        input.line("start", counter().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert!(f.parent.accept(queued.receive().unwrap()).is_err());
        assert_eq!((pool.occupied(), f.parent.last_id), (0, 0));
        assert!(f.parent.protocol_fault);
        assert!(f.peer.receive().is_none());
    }
    #[test]
    fn parent_destruction_closes_admission_while_external_input_leases_return_once() {
        let f = fixture();
        let pool = f.parent.command_pool();
        let (mut input, mut queued) = pool.channel();
        input.line("status", counter().unwrap()).unwrap();
        let reserved = queued.receive().unwrap();
        drop(f.parent);
        assert!(pool.closed());
        assert_eq!(pool.occupied(), 1);
        drop(reserved);
        assert_eq!(pool.occupied(), 0);
        assert!(input.line("start", counter().unwrap()).is_err());
    }
    #[test]
    fn invalid_ack_does_not_replace_the_last_admitted_snapshot() {
        for unknown in [true, false] {
            let mut f = fixture();
            ready(&mut f);
            let before = counter().unwrap();
            let received = loop {
                let value = counter().unwrap();
                if value > before {
                    break value;
                }
            };
            f.parent.command(Command::Start, received).unwrap();
            receive(&mut f.peer);
            let mut snapshot = ready_snapshot();
            snapshot.state = State::Starting;
            snapshot.generation = 1;
            snapshot.command_sequence = 1;
            let report = Output::Ack {
                id: if unknown { 2 } else { 1 },
                snapshot,
                refusal: None,
                observed_counter: if unknown { counter().unwrap() } else { before },
            };
            assert!(deliver(&mut f, 1, report).is_err());
            assert_eq!(f.parent.observed(), Some(ready_snapshot()));
        }
    }
    #[test]
    fn future_fault_generation_requires_a_pending_start_or_restart() {
        let mut f = fixture();
        ready(&mut f);
        assert!(
            deliver(
                &mut f,
                1,
                Output::SessionFault {
                    generation: 1,
                    observed_counter: counter().unwrap()
                }
            )
            .is_err()
        );
        assert_eq!(f.parent.observed(), Some(ready_snapshot()));
    }
    #[test]
    fn reversed_counters_and_replayed_ack_ids_end_admission_before_old_display() {
        let mut f = fixture();
        ready(&mut f);
        let old = f.parent.clock.origin().0;
        assert!(
            deliver(
                &mut f,
                0,
                Output::State {
                    snapshot: ready_snapshot(),
                    observed_counter: old
                }
            )
            .is_err()
        );
        let mut f = fixture();
        ready(&mut f);
        f.parent
            .command(Command::Status, counter().unwrap())
            .unwrap();
        receive(&mut f.peer);
        let mut snapshot = ready_snapshot();
        snapshot.command_sequence = 1;
        deliver(
            &mut f,
            0,
            Output::Ack {
                id: 1,
                snapshot,
                refusal: None,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        assert!(!f.parent.displays.is_empty());
        assert!(
            deliver(
                &mut f,
                0,
                Output::Ack {
                    id: 1,
                    snapshot,
                    refusal: None,
                    observed_counter: counter().unwrap()
                }
            )
            .is_err()
        );
        assert!(matches!(f.parent.event(), Some(ParentEvent::Fault)));
        assert!(f.parent.displays.is_empty() && f.parent.event().is_none());
    }
    #[test]
    fn quitting_refuses_new_transport_commands_and_keeps_status_local() {
        let mut f = fixture();
        assert_eq!(
            f.parent.command(Command::Quit, counter().unwrap()).unwrap(),
            Some(1)
        );
        receive(&mut f.peer);
        assert!(
            f.parent
                .command(Command::Start, counter().unwrap())
                .is_err()
        );
        assert_eq!(
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap(),
            None
        );
        assert_eq!(
            f.parent.command(Command::Quit, counter().unwrap()).unwrap(),
            None
        );
        assert_eq!(f.parent.outstanding(), 1);
        assert_eq!(f.parent.last_id, 1);
        assert!(!f.parent.protocol_fault);
    }
    #[test]
    fn foreground_credit_stays_owned_until_the_worker_confirms_display() {
        let mut f = fixture();
        for _ in 0..8 {
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap();
            assert!(matches!(
                Input::parse(&receive(&mut f.peer)).unwrap(),
                Input::Control { .. }
            ));
        }
        let display = status_ack(&mut f, 1);
        f.parent.displayed(display.ticket, None).unwrap();
        assert!(matches!(
            Input::parse(&receive(&mut f.peer)).unwrap(),
            Input::AckShown { id: 1, .. }
        ));
        assert_eq!(f.parent.outstanding(), 8);
        deliver(
            &mut f,
            0,
            Output::CreditReleased {
                id: 1,
                generation: 0,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        assert_eq!(f.parent.outstanding(), 7);
        assert_eq!(
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap(),
            Some(9)
        );
    }
    #[test]
    fn ninth_command_faults_without_stealing_credit_and_local_status_quit_survive() {
        let mut f = fixture();
        for _ in 0..8 {
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap();
            receive(&mut f.peer);
        }
        let display = status_ack(&mut f, 1);
        f.parent.displayed(display.ticket, None).unwrap();
        receive(&mut f.peer);
        assert!(
            f.parent
                .command(Command::Status, counter().unwrap())
                .is_err()
        );
        assert_eq!(f.parent.outstanding(), 8);
        assert!(matches!(f.parent.event(), Some(ParentEvent::Fault)));
        assert_eq!(
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap(),
            None
        );
        assert!(matches!(
            f.parent.event(),
            Some(ParentEvent::LocalStatus(None))
        ));
        assert_eq!(
            f.parent.command(Command::Quit, counter().unwrap()).unwrap(),
            None
        );
        assert!(f.parent.quit_requested());
    }
    #[test]
    fn late_display_cannot_erase_the_original_foreground_receipt_deadline() {
        let mut f = fixture();
        f.parent
            .command(Command::Status, counter().unwrap())
            .unwrap();
        receive(&mut f.peer);
        let display = status_ack(&mut f, 1);
        std::thread::sleep(Duration::from_millis(120));
        assert!(f.parent.displayed(display.ticket, None).is_err());
        assert!(f.parent.protocol_fault);
        assert!(f.parent.credits[0].shown.is_none());
        assert!(matches!(f.parent.event(), Some(ParentEvent::Fault)));
    }
    #[test]
    fn report_order_rejects_ready_before_text_and_unsolicited_new_generation() {
        let mut f = fixture();
        assert!(
            deliver(
                &mut f,
                0,
                Output::Ready {
                    snapshot: ready_snapshot(),
                    observed_counter: counter().unwrap()
                }
            )
            .is_err()
        );
        assert!(f.parent.failed());
        let mut f = fixture();
        ready(&mut f);
        let mut snapshot = ready_snapshot();
        snapshot.generation = 1;
        snapshot.state = State::Playing;
        assert!(
            deliver(
                &mut f,
                1,
                Output::State {
                    snapshot,
                    observed_counter: counter().unwrap()
                }
            )
            .is_err()
        );
        assert!(f.parent.failed());
    }
    #[test]
    fn credit_and_acknowledgements_require_a_real_pending_command_and_display() {
        for unknown in [true, false] {
            let mut f = fixture();
            f.parent
                .command(Command::Status, counter().unwrap())
                .unwrap();
            receive(&mut f.peer);
            let report = if unknown {
                Output::PreparationAck {
                    id: 2,
                    phase: Preparation::Checking,
                    refusal: None,
                    observed_counter: counter().unwrap(),
                }
            } else {
                Output::CreditReleased {
                    id: 1,
                    generation: 0,
                    observed_counter: counter().unwrap(),
                }
            };
            assert!(deliver(&mut f, 0, report).is_err());
            assert!(f.parent.failed() && f.parent.outstanding() == 1);
        }
    }
    #[test]
    fn display_receipts_are_ordered_once_and_require_the_actual_layout_observation() {
        for missing in [true, false] {
            let mut f = fixture();
            let text = "Señal\n";
            deliver(
                &mut f,
                0,
                Output::Transcript {
                    text: text.into(),
                    sha256: crate::production::sha256_bytes(text.as_bytes()),
                },
            )
            .unwrap();
            let display = item(&mut f);
            if missing {
                assert!(f.parent.displayed(display.ticket, None).is_err());
            } else {
                f.parent
                    .displayed(
                        display.ticket,
                        Some(LayoutObservation {
                            columns: 80,
                            rows: 30,
                            input_code_page: 65001,
                            output_code_page: 65001,
                        }),
                    )
                    .unwrap();
                assert!(f.parent.displayed(display.ticket, None).is_err());
            }
            assert!(f.parent.failed());
        }
    }
    #[test]
    fn worker_fault_remains_visible_when_a_late_actual_acknowledgement_arrives() {
        let mut f = fixture();
        ready(&mut f);
        f.parent
            .command(Command::Start, counter().unwrap())
            .unwrap();
        receive(&mut f.peer);
        deliver(
            &mut f,
            1,
            Output::SessionFault {
                generation: 1,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        let mut snapshot = ready_snapshot();
        snapshot.generation = 1;
        snapshot.command_sequence = 1;
        snapshot.state = State::Starting;
        deliver(
            &mut f,
            1,
            Output::Ack {
                id: 1,
                snapshot,
                refusal: None,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        assert!(item(&mut f).fault_overlay);
        assert!(item(&mut f).fault_overlay);
        assert!(f.parent.failed());
        assert!(
            f.parent
                .command(Command::Start, counter().unwrap())
                .is_err()
        );
    }
    #[test]
    fn final_observation_requires_quit_and_is_not_displayed_by_queue_acceptance() {
        let mut f = fixture();
        assert!(
            deliver(
                &mut f,
                0,
                Output::PreparationEnded {
                    phase: Preparation::Stopped,
                    observed_counter: counter().unwrap()
                }
            )
            .is_err()
        );
        let mut f = fixture();
        f.parent.command(Command::Quit, counter().unwrap()).unwrap();
        receive(&mut f.peer);
        let display = status_ack(&mut f, 1);
        f.parent.displayed(display.ticket, None).unwrap();
        receive(&mut f.peer);
        deliver(
            &mut f,
            0,
            Output::PreparationEnded {
                phase: Preparation::Stopped,
                observed_counter: counter().unwrap(),
            },
        )
        .unwrap();
        assert!(!f.parent.final_displayed());
        let display = item(&mut f);
        f.parent.displayed(display.ticket, None).unwrap();
        assert!(f.parent.final_displayed() && f.parent.outstanding() == 0);
    }
}

//! Serialized, allocation-free sample callback core. The host owns clocks,
//! actual endpoint observations, worker containment and asynchronous rechecks.
use super::*;
use std::{
    collections::VecDeque,
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

fn next_session() -> Outcome<u64> {
    // Compatible with the declared MSRV and newer toolchains; neither wrap the
    // identity counter nor rely on a renamed/deprecated update convenience API.
    let mut current = NEXT_SESSION.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .ok_or(TransportError::SessionExpired)?;
        match NEXT_SESSION.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Ok(current),
            Err(observed) => current = observed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum State {
    Ready,
    Starting,
    Playing,
    LoopWait,
    Paused,
    Restarting,
    Completed,
    Stopped,
    Fault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Start,
    Pause,
    Resume,
    Restart,
    Stop,
    Status,
    Quit,
}
impl Command {
    pub fn parse(line: &str) -> std::result::Result<Self, TransportError> {
        if line.len() > 32 || !line.is_ascii() {
            return Err(TransportError::InvalidCommand);
        }
        let s = line.trim_matches([' ', '\r', '\n']);
        [
            Self::Start,
            Self::Pause,
            Self::Resume,
            Self::Restart,
            Self::Stop,
            Self::Status,
            Self::Quit,
        ]
        .into_iter()
        .zip([
            "start", "pause", "resume", "restart", "stop", "status", "quit",
        ])
        .find(|(_, name)| s.eq_ignore_ascii_case(name))
        .map(|(c, _)| c)
        .ok_or(TransportError::InvalidCommand)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TransportError {
    InvalidCommand,
    InvalidState,
    QueueFull,
    TraceFull,
    ClockReversed,
    SessionExpired,
    StartupExpired,
    RestartExpired,
    CallbackStalled,
    BackendFault,
    InvalidBuffer,
    StaleGeneration,
    UnverifiedRestart,
}
impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "installation transport {self:?}")
    }
}
impl std::error::Error for TransportError {}
type Outcome<T> = std::result::Result<T, TransportError>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub open: Option<u64>,
    pub recheck: Option<u64>,
    pub close: bool,
    pub quit: bool,
}
#[derive(Clone, Copy, Debug)]
pub enum Notice {
    None,
    Opened { generation: u64 },
    BackendFault { generation: u64 },
    RestartRejected { generation: u64 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub state: State,
    pub generation: u64,
    pub source_frame: u64,
    pub interval_frame: u64,
    pub pass: u8,
    pub command_sequence: u64,
    pub session_closed: bool,
    pub fault: Option<TransportError>,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Trace {
    pub at_ms: u64,
    pub snapshot: Snapshot,
    pub refusal: Option<TransportError>,
}
pub struct Transport {
    session: u64,
    samples: Arc<[f32]>,
    transcript: Arc<str>,
    seal: [u8; 32],
    passes: u8,
    snapshot: Snapshot,
    paused_from: State,
    begin: u64,
    last: u64,
    last_callback: u64,
    stage_deadline: u64,
    restart_verified: bool,
    queue: VecDeque<(u64, Command)>,
    next_sequence: u64,
    trace: Vec<Trace>,
    effects: Effects,
}
impl Transport {
    pub(super) fn new(
        samples: Arc<[f32]>,
        transcript: Arc<str>,
        seal: [u8; 32],
        passes: u8,
        now: u64,
    ) -> Outcome<Self> {
        if now > u64::MAX - MAX_SESSION_MS {
            return Err(TransportError::SessionExpired);
        }
        Ok(Self {
            session: next_session()?,
            samples,
            transcript,
            seal,
            passes,
            snapshot: Snapshot {
                state: State::Ready,
                generation: 0,
                source_frame: 0,
                interval_frame: 0,
                pass: 1,
                command_sequence: 0,
                session_closed: false,
                fault: None,
            },
            paused_from: State::Playing,
            begin: now,
            last: now,
            last_callback: now,
            stage_deadline: 0,
            restart_verified: false,
            queue: VecDeque::with_capacity(8),
            next_sequence: 0,
            trace: Vec::with_capacity(256),
            effects: Effects::default(),
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot
    }
    pub fn trace(&self) -> &[Trace] {
        &self.trace
    }
    pub fn transcript(&self) -> &str {
        &self.transcript
    }
    pub fn source_frames(&self) -> u64 {
        (self.samples.len() / 2) as u64
    }
    pub fn enqueue(&mut self, line: &str) -> Outcome<u64> {
        let command = Command::parse(line)?;
        if self.snapshot.session_closed
            || (self.snapshot.state == State::Fault
                && !matches!(command, Command::Status | Command::Quit))
        {
            return Err(TransportError::InvalidState);
        }
        if self.queue.len() == 8 {
            self.fail(TransportError::QueueFull);
            return Err(TransportError::QueueFull);
        }
        self.next_sequence += 1;
        self.queue.push_back((self.next_sequence, command));
        Ok(self.next_sequence)
    }
    fn record(&mut self, refusal: Option<TransportError>) {
        if self.trace.len() == 256 {
            self.fail(TransportError::TraceFull);
        } else {
            self.trace.push(Trace {
                at_ms: self.last - self.begin,
                snapshot: self.snapshot,
                refusal,
            })
        }
    }
    fn fail(&mut self, error: TransportError) {
        self.snapshot.state = State::Fault;
        self.snapshot.fault = Some(error);
        self.effects.open = None;
        self.effects.recheck = None;
        self.effects.close = true;
    }
    fn close(&mut self) {
        self.effects.open = None;
        self.effects.recheck = None;
        self.effects.close = true;
    }
    fn time(&mut self, now: u64) -> bool {
        if now < self.last {
            self.fail(TransportError::ClockReversed);
            return false;
        }
        self.last = now;
        if now - self.begin >= MAX_SESSION_MS {
            if self.snapshot.state != State::Fault {
                self.snapshot.state = State::Stopped
            }
            self.snapshot.session_closed = true;
            self.queue.clear();
            self.close();
            self.effects.quit = true;
            return false;
        }
        true
    }
    /// Faults dominate commands; commands precede opened/media boundaries.
    /// Returned effects must be performed by the host off the audio callback.
    pub fn poll(&mut self, now: u64, notice: Notice) -> Effects {
        if let Notice::BackendFault { generation } = notice {
            if generation == self.snapshot.generation {
                self.fail(TransportError::BackendFault)
            }
        }
        if let Notice::RestartRejected { generation } = notice {
            if generation == self.snapshot.generation && self.snapshot.state == State::Restarting {
                self.fail(TransportError::UnverifiedRestart);
            }
        }
        if self.time(now) && !self.snapshot.session_closed {
            // Already elapsed transport failures cannot be erased by Pause,
            // Resume or Restart in the same tick. Quit still closes a Fault.
            match self.snapshot.state {
                State::Starting if now >= self.stage_deadline => {
                    self.fail(TransportError::StartupExpired)
                }
                State::Restarting if now >= self.stage_deadline => {
                    self.fail(TransportError::RestartExpired)
                }
                State::Playing | State::LoopWait if now - self.last_callback >= 250 => {
                    self.fail(TransportError::CallbackStalled)
                }
                _ => {}
            }
            while let Some((sequence, command)) = self.queue.pop_front() {
                self.snapshot.command_sequence = sequence;
                let valid = self.command(command);
                self.record(if valid {
                    None
                } else {
                    Some(TransportError::InvalidState)
                });
                if self.snapshot.session_closed {
                    self.queue.clear();
                    break;
                }
            }
            if let Notice::Opened { generation } = notice {
                if generation == self.snapshot.generation
                    && matches!(self.snapshot.state, State::Starting | State::Restarting)
                {
                    if self.snapshot.state == State::Restarting && !self.restart_verified {
                        self.fail(TransportError::UnverifiedRestart)
                    } else if now >= self.stage_deadline {
                        self.fail(if self.snapshot.state == State::Starting {
                            TransportError::StartupExpired
                        } else {
                            TransportError::RestartExpired
                        })
                    } else {
                        self.snapshot.state = State::Playing;
                        self.snapshot.source_frame = 0;
                        self.snapshot.interval_frame = 0;
                        self.snapshot.pass = 1;
                        self.last_callback = now;
                        self.record(None)
                    }
                }
            }
            match self.snapshot.state {
                State::Starting if now >= self.stage_deadline => {
                    self.fail(TransportError::StartupExpired)
                }
                State::Restarting if now >= self.stage_deadline => {
                    self.fail(TransportError::RestartExpired)
                }
                State::Playing | State::LoopWait if now - self.last_callback >= 250 => {
                    self.fail(TransportError::CallbackStalled)
                }
                _ => {}
            }
        }
        std::mem::take(&mut self.effects)
    }
    fn command(&mut self, command: Command) -> bool {
        let state = self.snapshot.state;
        if command == Command::Quit {
            self.snapshot.session_closed = true;
            if state != State::Fault {
                self.snapshot.state = State::Stopped
            }
            self.close();
            self.effects.quit = true;
            return true;
        }
        if command == Command::Status {
            return true;
        }
        if state == State::Fault {
            return false;
        }
        match command {
            Command::Start if state == State::Ready => {
                self.snapshot.generation += 1;
                self.snapshot.state = State::Starting;
                self.stage_deadline = self.last.saturating_add(2000);
                self.effects.open = Some(self.snapshot.generation);
            }
            Command::Pause if matches!(state, State::Playing | State::LoopWait) => {
                self.paused_from = state;
                self.snapshot.state = State::Paused;
            }
            Command::Resume if state == State::Paused => {
                self.snapshot.state = self.paused_from;
                self.last_callback = self.last;
            }
            Command::Stop
                if !matches!(
                    state,
                    State::Restarting
                        | State::Starting
                        | State::Ready
                        | State::Playing
                        | State::LoopWait
                        | State::Paused
                ) =>
            {
                return false;
            }
            Command::Stop => {
                self.snapshot.state = State::Stopped;
                self.close();
            }
            Command::Restart
                if matches!(
                    state,
                    State::Playing
                        | State::LoopWait
                        | State::Paused
                        | State::Completed
                        | State::Stopped
                ) =>
            {
                self.snapshot.generation += 1;
                self.snapshot.state = State::Restarting;
                self.restart_verified = false;
                self.stage_deadline = self.last.saturating_add(35000);
                self.close();
                self.effects.recheck = Some(self.snapshot.generation);
            }
            _ => return false,
        }
        true
    }
    /// The caller serializes access. No allocation/locks/I/O in this routine;
    /// trace/queue storage was preallocated and cannot grow beyond its cap.
    pub fn fill(&mut self, generation: u64, now: u64, out: &mut [f32]) -> Outcome<Effects> {
        out.fill(0.0);
        if generation != self.snapshot.generation {
            return Err(TransportError::StaleGeneration);
        }
        if out.is_empty() || out.len() % 2 != 0 || out.len() > MAX_CALLBACK_FRAMES * 2 {
            self.fail(TransportError::InvalidBuffer);
            return Err(TransportError::InvalidBuffer);
        }
        let mut effects = self.poll(now, Notice::None);
        self.last_callback = self.last;
        for frame in out.chunks_exact_mut(2) {
            match self.snapshot.state {
                State::Playing => {
                    let at = self.snapshot.source_frame as usize * 2;
                    frame.copy_from_slice(&self.samples[at..at + 2]);
                    self.snapshot.source_frame += 1;
                    if self.snapshot.source_frame == self.source_frames() {
                        self.snapshot.state = if self.snapshot.pass == self.passes {
                            State::Completed
                        } else {
                            State::LoopWait
                        };
                        self.snapshot.interval_frame = 0;
                        self.record(None);
                    }
                }
                State::LoopWait => {
                    self.snapshot.interval_frame += 1;
                    if self.snapshot.interval_frame == INTERVAL_FRAMES {
                        self.snapshot.interval_frame = 0;
                        self.snapshot.source_frame = 0;
                        self.snapshot.pass += 1;
                        self.snapshot.state = State::Playing;
                        self.record(None);
                    }
                }
                _ => {}
            }
        }
        if self.snapshot.state == State::Fault {
            effects.close = true;
            effects.open = None;
            effects.recheck = None;
            self.effects = Effects::default()
        }
        Ok(effects)
    }
    pub fn restart_ticket(&self) -> Outcome<RestartTicket> {
        if self.snapshot.state != State::Restarting || self.restart_verified {
            return Err(TransportError::InvalidState);
        }
        Ok(RestartTicket {
            session: self.session,
            generation: self.snapshot.generation,
            seal: self.seal,
        })
    }
    pub fn accept_restart(&mut self, permit: RestartPermit, now: u64) -> Outcome<Effects> {
        let effects = self.poll(now, Notice::None);
        if permit.session != self.session
            || permit.generation != self.snapshot.generation
            || self.snapshot.state != State::Restarting
            || self.restart_verified
        {
            self.effects = effects;
            return Err(TransportError::StaleGeneration);
        }
        if permit.seal != self.seal {
            self.fail(TransportError::UnverifiedRestart);
            self.effects.quit |= effects.quit;
            return Err(TransportError::UnverifiedRestart);
        }
        self.restart_verified = true;
        // The aggregate restart deadline remains authoritative; opening cannot
        // gain another 2 seconds past that boundary.
        self.stage_deadline = self.stage_deadline.min(now.saturating_add(2000));
        Ok(Effects {
            open: Some(permit.generation),
            recheck: None,
            close: effects.close,
            quit: effects.quit,
        })
    }
}

#[cfg(test)]
mod tests;

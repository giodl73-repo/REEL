//! One transport owner, returned intact before stream close/restart. Channels
//! are allocated before activation; callback operations only move fixed values.
use super::*;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

const OWNED: u8 = 0;
const WRITING: u8 = 1;
const RETURNING: u8 = 2;
const RETURNED: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackCommandError {
    QueueFull,
    OwnershipReturned,
}
impl std::fmt::Display for CallbackCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::QueueFull => "installation native command queue full",
            Self::OwnershipReturned => "installation callback ownership returned",
        })
    }
}
impl std::error::Error for CallbackCommandError {}

#[derive(Clone, Copy, Debug)]
pub struct CallbackReport {
    pub snapshot: Snapshot,
    pub callback_ms: u64,
    pub predicted_playback_us: Option<u64>,
    pub effects: Effects,
    /// Actual command records, including refusals, from this callback. Media
    /// boundaries sharing a sequence do not masquerade as another command.
    pub commands: [Option<Trace>; 8],
}
pub struct CallbackControl {
    commands: Producer<Command>,
    reports: Consumer<CallbackReport>,
    returned: Consumer<Box<Transport>>,
    fatal: Arc<AtomicBool>,
    phase: Arc<AtomicU8>,
}
pub struct Callback {
    transport: Option<Box<Transport>>,
    commands: Consumer<Command>,
    reports: Producer<CallbackReport>,
    returned: Producer<Box<Transport>>,
    fatal: Arc<AtomicBool>,
    opened: bool,
    phase: Arc<AtomicU8>,
    pending_return: bool,
}
pub fn callback_pair(transport: Box<Transport>) -> (CallbackControl, Callback) {
    let (commands, receive) = RingBuffer::new(8);
    let (reports, read) = RingBuffer::new(256);
    let (returned, take) = RingBuffer::new(1);
    let fatal = Arc::new(AtomicBool::new(false));
    let phase = Arc::new(AtomicU8::new(OWNED));
    (
        CallbackControl {
            commands,
            reports: read,
            returned: take,
            fatal: fatal.clone(),
            phase: phase.clone(),
        },
        Callback {
            transport: Some(transport),
            commands: receive,
            reports,
            returned,
            fatal,
            opened: false,
            phase,
            pending_return: false,
        },
    )
}
impl CallbackControl {
    pub fn command(&mut self, command: Command) -> Result<()> {
        self.try_command(command).map_err(anyhow::Error::new)
    }
    /// A returned-owner refusal is nonfatal: the controller must apply that
    /// command to the returned transport, not leave it in an abandoned channel.
    pub fn try_command(
        &mut self,
        command: Command,
    ) -> std::result::Result<(), CallbackCommandError> {
        if self
            .phase
            .compare_exchange(OWNED, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(CallbackCommandError::OwnershipReturned);
        }
        let pushed = self.commands.push(command).is_ok();
        if !pushed {
            self.fail();
        }
        self.phase.store(OWNED, Ordering::Release);
        if pushed {
            Ok(())
        } else {
            Err(CallbackCommandError::QueueFull)
        }
    }
    pub fn report(&mut self) -> Option<CallbackReport> {
        self.reports.pop().ok()
    }
    pub fn returned(&mut self) -> Option<Box<Transport>> {
        self.returned.pop().ok()
    }
    pub fn fail(&self) {
        self.fatal.store(true, Ordering::Release)
    }
    pub fn failed(&self) -> bool {
        self.fatal.load(Ordering::Acquire)
    }
}
fn enqueue_pending(
    commands: &mut Consumer<Command>,
    transport: &mut Transport,
    fatal: &AtomicBool,
) {
    for _ in 0..8 {
        let Ok(command) = commands.pop() else {
            break;
        };
        match transport.enqueue(command.as_str()) {
            Ok(_) => {}
            // Terminal/closed cores refuse late controls without manufacturing
            // a new backend failure. Their owner reports the actual terminal
            // state, and Status/Quit behind that refusal remain processable.
            Err(TransportError::InvalidState) => {}
            Err(_) => {
                fatal.store(true, Ordering::Release);
                break;
            }
        }
    }
}
fn records(transport: &Transport, before: u64) -> [Option<Trace>; 8] {
    let mut result = [None; 8];
    let mut last = before;
    let mut count = 0;
    for event in transport.trace() {
        if event.snapshot.command_sequence > last && count < result.len() {
            result[count] = Some(*event);
            last = event.snapshot.command_sequence;
            count += 1;
        }
    }
    result
}
impl Callback {
    fn begin_return(&self) -> bool {
        match self
            .phase
            .compare_exchange(OWNED, RETURNING, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => true,
            Err(value) => value == RETURNING,
        }
    }
    fn return_transport(&mut self) {
        if let Some(owned) = self.transport.take() {
            match self.returned.push(owned) {
                Ok(()) => self.phase.store(RETURNED, Ordering::Release),
                Err(rtrb::PushError::Full(owned)) => {
                    self.transport = Some(owned);
                    self.fatal.store(true, Ordering::Release);
                }
            }
        }
    }
    /// The backend calls this after its stream actually starts delivering data.
    /// No checker, allocation, serialization, locking or I/O occurs here.
    pub fn fill(&mut self, now_ms: u64, predicted_playback_us: Option<u64>, out: &mut [f32]) {
        out.fill(0.0);
        let Some(before) = self
            .transport
            .as_ref()
            .map(|t| t.snapshot().command_sequence)
        else {
            return;
        };
        // Reserve report capacity before advancing the source. On overflow the
        // output is silence and the submitted-source cursor must not advance.
        if self.reports.slots() == 0 {
            self.fatal.store(true, Ordering::Release);
        }
        let mut effects = Effects::default();
        {
            let transport = self.transport.as_mut().unwrap();
            let generation = transport.snapshot().generation;
            if self.fatal.load(Ordering::Acquire) {
                effects = transport.poll(now_ms, Notice::BackendFault { generation });
            } else {
                enqueue_pending(&mut self.commands, transport, &self.fatal);
                if self.pending_return {
                    effects = transport.poll(now_ms, Notice::None);
                } else {
                    if !self.opened {
                        // Commands precede the Opened notice: Stop at this boundary
                        // cannot be undone by an already completed native open.
                        effects = transport.poll(now_ms, Notice::Opened { generation });
                        self.opened = true;
                    }
                    match transport.fill(generation, now_ms, out) {
                        Ok(next) => {
                            effects.close |= next.close;
                            effects.quit |= next.quit;
                            effects.open = next.open.or(effects.open);
                            effects.recheck = next.recheck.or(effects.recheck);
                        }
                        Err(TransportError::StaleGeneration) => {
                            // Restart changed generation while draining commands.
                            // No samples are emitted; its recheck effect is below.
                            let next = transport.poll(now_ms, Notice::None);
                            effects.close |= next.close;
                            effects.recheck = next.recheck.or(effects.recheck);
                            effects.quit |= next.quit;
                        }
                        Err(_) => {
                            effects.close = true;
                            self.fatal.store(true, Ordering::Release);
                        }
                    }
                }
            }
            self.pending_return |= effects.close
                || effects.recheck.is_some()
                || effects.quit
                || matches!(
                    transport.snapshot().state,
                    State::Fault | State::Completed | State::Stopped
                );
        }
        let transport = self.transport.as_mut().unwrap();
        effects.close |= self.pending_return;
        let snapshot = transport.snapshot();
        if self
            .reports
            .push(CallbackReport {
                snapshot,
                callback_ms: now_ms,
                predicted_playback_us,
                effects,
                commands: records(transport, before),
            })
            .is_err()
        {
            self.fatal.store(true, Ordering::Release);
            transport.poll(
                now_ms,
                Notice::BackendFault {
                    generation: transport.snapshot().generation,
                },
            );
            out.fill(0.0);
            self.pending_return = true;
        }
        if self.pending_return && self.begin_return() {
            // No producer can enter after RETURNING. Commands accepted just
            // before that transition travel in the real core queue; applying
            // them happens on the controller's next poll, after this buffer.
            enqueue_pending(
                &mut self.commands,
                self.transport.as_mut().unwrap(),
                &self.fatal,
            );
            self.return_transport();
        }
    }
}
impl Drop for Callback {
    fn drop(&mut self) {
        if self.transport.is_none() {
            return;
        }
        // Destruction belongs to the native teardown thread, never fill(). A
        // preempted writer can finish its bounded send before ownership moves.
        while !self.begin_return() {
            std::thread::yield_now();
        }
        enqueue_pending(
            &mut self.commands,
            self.transport.as_mut().unwrap(),
            &self.fatal,
        );
        self.return_transport();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::AtomicUsize;
    static ARMED: AtomicBool = AtomicBool::new(false);
    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static FREES: AtomicUsize = AtomicUsize::new(0);
    struct CountAllocator;
    // Instrumentation is enabled only inside a single isolated fixture child;
    // parallel tests in the parent never arm the global counters.
    unsafe impl GlobalAlloc for CountAllocator {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            if ARMED.load(Ordering::Relaxed) {
                ALLOCS.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            if ARMED.load(Ordering::Relaxed) {
                FREES.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            if ARMED.load(Ordering::Relaxed) {
                ALLOCS.fetch_add(1, Ordering::Relaxed);
                FREES.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.realloc(p, l, n) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: CountAllocator = CountAllocator;
    fn engine() -> Box<Transport> {
        let mut pcm = vec![0.0; 20000];
        pcm[9700] = 0.125;
        let mut t = Transport::new(pcm.into(), "Texto español\n".into(), [7; 32], 2, 0).unwrap();
        t.enqueue("start").unwrap();
        assert_eq!(t.poll(0, Notice::None).open, Some(1));
        Box::new(t)
    }
    #[test]
    fn callback_preserves_pcm_and_returns_the_same_restart_context() {
        let (mut control, mut callback) = callback_pair(engine());
        let mut out = [0.0; 2000];
        for n in 0..5 {
            callback.fill(1 + n * 10, Some(1000 + n * 10000), &mut out);
            assert!(control.report().is_some());
        }
        assert_eq!(out[1700], 0.125);
        control.command(Command::Pause).unwrap();
        callback.fill(52, Some(52000), &mut out);
        let paused = control.report().unwrap().snapshot;
        assert_eq!(paused.state, State::Paused);
        callback.fill(5000, None, &mut out);
        assert_eq!(control.report().unwrap().snapshot, paused);
        assert!(out.iter().all(|v| *v == 0.0));
        control.command(Command::Restart).unwrap();
        callback.fill(5001, None, &mut out);
        let report = control.report().unwrap();
        assert_eq!(report.effects.recheck, Some(2));
        let transport = control.returned().unwrap();
        assert_eq!(transport.snapshot().state, State::Restarting);
        assert!(transport.restart_ticket().is_ok());
        assert_eq!(transport.transcript(), "Texto español\n");
        callback.fill(5002, None, &mut out);
        assert!(out.iter().all(|v| *v == 0.0));
    }
    #[test]
    fn callback_stop_precedes_first_opened_and_fatal_queues_return_ownership() {
        let (mut control, mut callback) = callback_pair(engine());
        control.command(Command::Stop).unwrap();
        let mut out = [1.0; 960];
        callback.fill(1, None, &mut out);
        assert!(out.iter().all(|v| *v == 0.0));
        assert_eq!(control.returned().unwrap().snapshot().state, State::Stopped);
        let (mut control, mut callback) = callback_pair(engine());
        for _ in 0..8 {
            control.command(Command::Status).unwrap();
        }
        assert!(control.command(Command::Status).is_err());
        callback.fill(1, None, &mut out);
        assert_eq!(control.returned().unwrap().snapshot().state, State::Fault);
        let (mut control, mut callback) = callback_pair(engine());
        let mut tiny = [0.0; 2];
        for n in 0..257 {
            callback.fill(n, None, &mut tiny);
        }
        assert!(control.failed());
        let actual = control.returned().unwrap().snapshot();
        assert_eq!(actual.state, State::Fault);
        assert_eq!(
            actual.source_frame, 256,
            "overflow silence is not a submitted source frame"
        );
    }
    #[test]
    fn completion_returns_the_same_box_and_rejects_abandoned_commands() {
        let transport = engine();
        let identity = (&*transport as *const Transport) as usize;
        let (mut control, mut callback) = callback_pair(transport);
        let mut out = [0.0; 4096];
        let mut nonzero = 0;
        let mut actual = None;
        for n in 1..=32 {
            callback.fill(n, None, &mut out);
            nonzero += out.iter().filter(|v| **v != 0.0).count();
            let _ = control.report();
            if let Some(owned) = control.returned() {
                actual = Some(owned);
                break;
            }
        }
        let actual = actual.unwrap();
        assert_eq!((&*actual as *const Transport) as usize, identity);
        assert_eq!(actual.snapshot().state, State::Completed);
        assert_eq!(
            (actual.snapshot().pass, actual.snapshot().source_frame),
            (2, 10000)
        );
        assert_eq!(nonzero, 2);
        assert_eq!(
            control.try_command(Command::Status),
            Err(CallbackCommandError::OwnershipReturned)
        );
        assert!(!control.failed());
    }
    #[test]
    fn drop_before_first_fill_returns_the_box_with_pending_stop() {
        let transport = engine();
        let identity = (&*transport as *const Transport) as usize;
        let (mut control, callback) = callback_pair(transport);
        control.command(Command::Stop).unwrap();
        drop(callback);
        let mut actual = control.returned().unwrap();
        assert_eq!((&*actual as *const Transport) as usize, identity);
        actual.poll(1, Notice::None);
        assert_eq!(actual.snapshot().state, State::Stopped);
        assert_eq!(actual.snapshot().source_frame, 0);
        actual.enqueue("restart").unwrap();
        assert_eq!(actual.poll(2, Notice::None).recheck, Some(2));
        assert!(actual.restart_ticket().is_ok());
    }
    #[test]
    fn a_preempted_writer_defers_return_without_blocking_fill() {
        let (mut control, mut callback) = callback_pair(engine());
        control.command(Command::Stop).unwrap();
        struct Lease(Arc<AtomicU8>);
        impl Drop for Lease {
            fn drop(&mut self) {
                self.0.store(OWNED, Ordering::Release);
            }
        }
        let lease = Lease(control.phase.clone());
        lease.0.store(WRITING, Ordering::Release);
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let task = std::thread::spawn(move || {
            let mut out = [1.0; 2];
            callback.fill(1, None, &mut out);
            send.send((callback, out))
                .unwrap_or_else(|_| panic!("callback fixture receiver closed"));
        });
        // The only writer release happens after fill returns (or the bounded
        // harness timeout). A callback waiting for that writer cannot pass.
        let completed = receive.recv_timeout(Duration::from_secs(5));
        assert!(control.returned().is_none());
        drop(lease);
        task.join().unwrap();
        let (mut callback, out) = completed.expect("fill waited for a preempted writer");
        assert_eq!(out, [0.0; 2]);
        callback.fill(2, None, &mut [0.0; 2]);
        assert_eq!(control.returned().unwrap().snapshot().state, State::Stopped);
    }
    #[test]
    fn racing_return_never_accepts_a_command_into_an_abandoned_queue() {
        for _ in 0..64 {
            let (mut control, mut callback) = callback_pair(engine());
            control.command(Command::Stop).unwrap();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let child = barrier.clone();
            let task = std::thread::spawn(move || {
                child.wait();
                callback.fill(1, None, &mut [0.0; 2]);
            });
            barrier.wait();
            let accepted = control.try_command(Command::Status);
            task.join().unwrap();
            let mut actual = control.returned().unwrap();
            actual.poll(2, Notice::None);
            match accepted {
                Ok(()) => assert_eq!(actual.snapshot().command_sequence, 3),
                Err(CallbackCommandError::OwnershipReturned) => {
                    assert_eq!(actual.snapshot().command_sequence, 2);
                    actual.enqueue("status").unwrap();
                    actual.poll(3, Notice::None);
                    assert_eq!(actual.snapshot().command_sequence, 3);
                }
                other => panic!("unexpected bounded command result: {other:?}"),
            }
            assert!(!control.failed());
        }
    }
    #[test]
    fn command_records_keep_refusals_before_opened_and_media_events() {
        let (mut control, mut callback) = callback_pair(engine());
        control.command(Command::Pause).unwrap(); // Starting: invalid.
        control.command(Command::Status).unwrap();
        callback.fill(1, None, &mut [0.0; 960]);
        let report = control.report().unwrap();
        assert_eq!(report.snapshot.state, State::Playing);
        assert_eq!(report.commands.iter().flatten().count(), 2);
        let refused = report.commands[0].unwrap();
        assert_eq!(refused.snapshot.command_sequence, 2);
        assert_eq!(refused.refusal, Some(TransportError::InvalidState));
        assert_eq!(refused.snapshot.state, State::Starting);
        assert_eq!(report.commands[1].unwrap().snapshot.state, State::Starting);
    }
    #[test]
    fn controls_refused_after_quit_do_not_invent_a_backend_fault() {
        let mut transport = engine();
        transport.enqueue("quit").unwrap();
        transport.poll(1, Notice::None);
        let (mut control, mut callback) = callback_pair(transport);
        control.command(Command::Status).unwrap();
        callback.fill(2, None, &mut [1.0; 2]);
        let actual = control.returned().unwrap();
        assert!(actual.snapshot().session_closed);
        assert_eq!(actual.snapshot().state, State::Stopped);
        assert!(!control.failed());
    }
    #[test]
    #[ignore = "isolated allocator fixture only; explicitly launched by parent"]
    fn allocation_fixture() {
        let (_control, mut callback) = callback_pair(engine());
        let mut out = [0.0; 2];
        ALLOCS.store(0, Ordering::Relaxed);
        FREES.store(0, Ordering::Relaxed);
        ARMED.store(true, Ordering::Relaxed);
        for n in 0..260 {
            callback.fill(n, Some(n * 1000), &mut out);
        }
        ARMED.store(false, Ordering::Relaxed);
        assert_eq!(ALLOCS.load(Ordering::Relaxed), 0);
        assert_eq!(FREES.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn callback_handoff_and_overflow_do_not_allocate_or_free() {
        use std::{
            process::{Command as ChildCommand, Stdio},
            time::{Duration, Instant},
        };
        let mut command = ChildCommand::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "spoken_programme::installation::native::callback::tests::allocation_fixture",
                "--test-threads=1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        struct Owned(std::process::Child);
        impl Drop for Owned {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Owned(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "isolated callback allocation fixture failed"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "isolated callback allocation fixture timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

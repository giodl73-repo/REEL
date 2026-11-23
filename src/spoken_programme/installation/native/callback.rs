//! One transport owner, returned intact before stream close/restart. Channels
//! are allocated before capture; callback operations only move fixed values.
use super::*;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug)]
pub struct CallbackReport {
    pub snapshot: Snapshot,
    pub callback_ms: u64,
    pub predicted_playback_us: Option<u64>,
    pub effects: Effects,
}
pub struct CallbackControl {
    commands: Producer<Command>,
    reports: Consumer<CallbackReport>,
    returned: Consumer<Box<Transport>>,
    fatal: Arc<AtomicBool>,
}
pub struct Callback {
    transport: Option<Box<Transport>>,
    commands: Consumer<Command>,
    reports: Producer<CallbackReport>,
    returned: Producer<Box<Transport>>,
    fatal: Arc<AtomicBool>,
    opened: bool,
}
pub fn callback_pair(transport: Box<Transport>) -> (CallbackControl, Callback) {
    let (commands, receive) = RingBuffer::new(8);
    let (reports, read) = RingBuffer::new(256);
    let (returned, take) = RingBuffer::new(1);
    let fatal = Arc::new(AtomicBool::new(false));
    (
        CallbackControl {
            commands,
            reports: read,
            returned: take,
            fatal: fatal.clone(),
        },
        Callback {
            transport: Some(transport),
            commands: receive,
            reports,
            returned,
            fatal,
            opened: false,
        },
    )
}
impl CallbackControl {
    pub fn command(&mut self, command: Command) -> Result<()> {
        if self.commands.push(command).is_err() {
            self.fail();
            anyhow::bail!("installation native command queue full")
        }
        Ok(())
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
fn command_name(command: Command) -> &'static str {
    match command {
        Command::Start => "start",
        Command::Pause => "pause",
        Command::Resume => "resume",
        Command::Restart => "restart",
        Command::Stop => "stop",
        Command::Status => "status",
        Command::Quit => "quit",
    }
}
impl Callback {
    /// The backend calls this after its stream actually starts delivering data.
    /// No checker, allocation, serialization, locking or I/O occurs here.
    pub fn fill(&mut self, now_ms: u64, predicted_playback_us: Option<u64>, out: &mut [f32]) {
        out.fill(0.0);
        let Some(transport) = self.transport.as_mut() else {
            return;
        };
        let generation = transport.snapshot().generation;
        let mut effects = Effects::default();
        if self.fatal.load(Ordering::Acquire) {
            effects = transport.poll(now_ms, Notice::BackendFault { generation });
        } else {
            for _ in 0..8 {
                let Ok(command) = self.commands.pop() else {
                    break;
                };
                if transport.enqueue(command_name(command)).is_err() {
                    self.fatal.store(true, Ordering::Release);
                    break;
                }
            }
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
                    effects.recheck = next.recheck;
                    effects.quit |= next.quit;
                }
                Err(_) => {
                    effects.close = true;
                    self.fatal.store(true, Ordering::Release);
                }
            }
        }
        let snapshot = transport.snapshot();
        if self
            .reports
            .push(CallbackReport {
                snapshot,
                callback_ms: now_ms,
                predicted_playback_us,
                effects,
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
            effects.close = true;
        }
        if effects.close
            || effects.recheck.is_some()
            || effects.quit
            || snapshot.state == State::Fault
        {
            // Moving a Box does not allocate/free its storage. Keep ownership
            // here if the return queue unexpectedly refuses it.
            if let Some(owned) = self.transport.take() {
                if let Err(rtrb::PushError::Full(owned)) = self.returned.push(owned) {
                    self.transport = Some(owned);
                    self.fatal.store(true, Ordering::Release);
                }
            }
        }
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
        for n in 0..257 {
            callback.fill(n, None, &mut out);
        }
        assert!(control.failed());
        assert!(control.returned().is_some());
    }
    #[test]
    #[ignore = "isolated allocator fixture only; explicitly launched by parent"]
    fn allocation_fixture() {
        let (_control, mut callback) = callback_pair(engine());
        let mut out = [0.0; 960];
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

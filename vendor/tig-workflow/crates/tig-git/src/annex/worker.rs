//! A named Windows job is the recoverable worker identity. This is not a daemon.
//! One explicit effect command enrolls its own process before creating children.
use std::io;
#[cfg(windows)]
use std::time::{Duration, Instant};
use tig_core::Digest;
use tig_store::source_claims::protection::effects::{self, Request, Terminated};
#[cfg(windows)]
static ENTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub struct Worker {
    effect: Digest,
    pid: u32,
    issued: bool,
}
pub struct Stopped {
    effect: Digest,
}
impl Terminated for Stopped {
    fn effect(&self) -> &Digest {
        &self.effect
    }
}
fn unavailable() -> io::Error {
    io::Error::other("effect worker termination is unknown")
}
fn identity(index: &Digest, request: &Request) -> io::Result<Digest> {
    effects::fingerprint(index, request).map_err(|_| unavailable())
}
#[cfg(windows)]
fn name(effect: &Digest) -> String {
    format!("Local\\tig-effect-{}", effect.as_str())
}

impl Worker {
    /// Explicit one-effect CLI worker enrollment. The handle deliberately lives
    /// until process exit: closing a kill-on-close job containing this process
    /// would kill the controller before it can save/report its terminal result.
    /// There is exactly one retained handle per short-lived command process.
    #[cfg(windows)]
    pub fn enter_current_process(index: &Digest, request: &Request) -> io::Result<Self> {
        Self::enter_scope(identity(index, request)?)
    }
    #[cfg(windows)]
    fn enter_scope(effect: Digest) -> io::Result<Self> {
        use firehazard::{
            self as win,
            job::object::{ExtendedLimitInformation, limit},
        };
        let job_name = name(&effect);
        match win::open_job_object_a(win::access::GENERIC_ALL, false, job_name.as_str()) {
            Ok(_) => return Err(unavailable()),
            Err(error) if error == 2u32 => (),
            Err(_) => return Err(unavailable()),
        }
        let job = win::create_job_object_a(None, job_name.as_str()).map_err(|_| unavailable())?;
        let mut limits = ExtendedLimitInformation::default();
        limits.basic_limit_information.limit_flags = limit::KILL_ON_JOB_CLOSE;
        win::set_information_job_object(&job, limits).map_err(|_| unavailable())?;
        if ENTERED
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return Err(unavailable());
        }
        if win::assign_process_to_job_object(&job, win::get_current_process()).is_err() {
            ENTERED.store(false, std::sync::atomic::Ordering::SeqCst);
            return Err(unavailable());
        }
        // No fallible work between assignment and retention. OS process exit
        // closes this sole handle, terminating all descendants including nested jobs.
        std::mem::forget(job);
        Ok(Self {
            effect,
            pid: std::process::id(),
            issued: false,
        })
    }
    #[cfg(not(windows))]
    pub fn enter_current_process(_: &Digest, _: &Request) -> io::Result<Self> {
        Err(unavailable())
    }
    pub(crate) fn admit(&mut self, index: &Digest, request: &Request) -> io::Result<()> {
        if self.issued || self.pid != std::process::id() || self.effect != identity(index, request)?
        {
            return Err(unavailable());
        }
        self.issued = true;
        Ok(())
    }
    #[cfg(windows)]
    pub fn children_stopped(&self, millis: u64) -> io::Result<()> {
        use firehazard::{self as win, job::object::BasicAccountingInformation};
        if !(1..=30_000).contains(&millis) || self.pid != std::process::id() {
            return Err(unavailable());
        }
        let job =
            win::open_job_object_a(win::access::GENERIC_ALL, false, name(&self.effect).as_str())
                .map_err(|_| unavailable())?;
        let start = Instant::now();
        loop {
            let accounting: BasicAccountingInformation =
                win::query_information_job_object(&job).map_err(|_| unavailable())?;
            if accounting.active_processes == 1 {
                return Ok(());
            }
            if start.elapsed() > Duration::from_millis(millis) {
                return Err(unavailable());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(not(windows))]
    pub fn children_stopped(&self, _: u64) -> io::Result<()> {
        Err(unavailable())
    }
}

/// Explicit recovery/cancellation stops the exact named process tree. A missing
/// job proves no associated old processes remain; access/unknown errors refuse.
/// No key-level drop, PID reuse assumption, lease expiry or blind retry is used.
#[cfg(windows)]
pub fn stop(index: &Digest, request: &Request, millis: u64) -> io::Result<Stopped> {
    stop_scope(identity(index, request)?, millis)
}
#[cfg(windows)]
fn stop_scope(effect: Digest, millis: u64) -> io::Result<Stopped> {
    use firehazard::{self as win, job::object::BasicAccountingInformation};
    if !(1..=30_000).contains(&millis) {
        return Err(unavailable());
    }
    let job = match win::open_job_object_a(win::access::GENERIC_ALL, false, name(&effect).as_str())
    {
        Ok(job) => job,
        Err(error) if error == 2u32 => return Ok(Stopped { effect }),
        Err(_) => return Err(unavailable()),
    };
    // Recovery must run in a different command process from the effect.
    if win::is_process_in_job(win::get_current_process(), Some(&job)).map_err(|_| unavailable())? {
        return Err(unavailable());
    }
    win::terminate_job_object(&job, 1).map_err(|_| unavailable())?;
    let start = Instant::now();
    loop {
        let accounting: BasicAccountingInformation =
            win::query_information_job_object(&job).map_err(|_| unavailable())?;
        if accounting.active_processes == 0 {
            return Ok(Stopped { effect });
        }
        if start.elapsed() > Duration::from_millis(millis) {
            return Err(unavailable());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(not(windows))]
pub fn stop(_: &Digest, _: &Request, _: u64) -> io::Result<Stopped> {
    Err(unavailable())
}

/// Short-lived orchestrator scope. It grants no per-object execution lease and
/// cannot be converted to a Terminated proof for a payload effect.
pub struct Controller(Worker);
#[cfg(windows)]
fn controller_scope(scope: &Digest) -> Digest {
    Digest::blob(format!("tig.annex-controller.design-1\n{}", scope.as_str()).as_bytes())
}
impl Controller {
    #[cfg(windows)]
    pub fn enter(scope: &Digest) -> io::Result<Self> {
        Worker::enter_scope(controller_scope(scope)).map(Self)
    }
    #[cfg(not(windows))]
    pub fn enter(_: &Digest) -> io::Result<Self> {
        Err(unavailable())
    }
    pub fn children_stopped(&self, millis: u64) -> io::Result<()> {
        self.0.children_stopped(millis)
    }
    #[cfg(windows)]
    pub fn stop(scope: &Digest, millis: u64) -> io::Result<()> {
        stop_scope(controller_scope(scope), millis).map(drop)
    }
    #[cfg(not(windows))]
    pub fn stop(_: &Digest, _: u64) -> io::Result<()> {
        Err(unavailable())
    }
}

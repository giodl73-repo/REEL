//! Guarded originals and opaque restart capabilities stay in one worker process.
//! Explicit construction starts checking, never an endpoint. Blocking checks and
//! guard destruction run off the controller; process containment is still the
//! caller's responsibility, including a checker that defeats cooperative stop.
use super::{output::OutputClock, session::LoadedProgramme, *};
use crate::spoken_programme::installation::{RestartPermit, RestartTicket};
use rtrb::{Consumer, Producer, RingBuffer};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFailure {
    Cancelled,
    Deadline,
    Clock,
    Original,
}

#[derive(Clone)]
pub struct SourceOperation {
    pub generation: u64,
    deadline_us: u64,
    clock: OutputClock,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}
impl SourceOperation {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    fn tick(&self) -> std::result::Result<(), SourceFailure> {
        if self.cancel.load(Ordering::Acquire) || self.stop.load(Ordering::Acquire) {
            return Err(SourceFailure::Cancelled);
        }
        let now = self.clock.now_us().ok_or(SourceFailure::Clock)?;
        if now >= self.deadline_us || now >= MAX_SESSION_MS * 1000 {
            return Err(SourceFailure::Deadline);
        }
        Ok(())
    }
    fn control(&self) -> Result<producer::ProductionControl<impl FnMut() -> Result<()>>> {
        self.tick()
            .map_err(|_| anyhow::anyhow!("installation source operation ended"))?;
        Ok(producer::ProductionControl {
            timeout: Duration::from_micros(
                self.deadline_us
                    .saturating_sub(self.clock.now_us().unwrap_or(self.deadline_us)),
            ),
            cancellation: || self.check(),
        })
    }
    fn check(&self) -> Result<()> {
        self.tick()
            .map_err(|_| anyhow::anyhow!("installation source operation ended"))
    }
    fn failure(&self) -> SourceFailure {
        self.tick().err().unwrap_or(SourceFailure::Original)
    }
}

/// In-process ownership transfer only: neither transport nor permit is a wire
/// DTO. Callers must still apply the permit to the actual same-generation core.
pub enum SourceEvent {
    Ready(Box<Transport>),
    RestartChecked {
        generation: u64,
        permit: RestartPermit,
    },
    Failed {
        generation: u64,
        reason: SourceFailure,
    },
}
struct Task {
    operation: SourceOperation,
    ticket: RestartTicket,
}

/// One original-check thread inside the owned native worker. Guards and Prepared
/// remain alive after Ready and through every recheck. Drop requests cancellation
/// and joins actual destruction; it does not manufacture bounded process cleanup.
pub struct SourceWorker {
    tasks: Producer<Task>,
    events: Consumer<SourceEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    active: Option<SourceOperation>,
    initialized: bool,
    clock: OutputClock,
}
impl SourceWorker {
    pub fn new(request: SessionRequest, clock: OutputClock, deadline_us: u64) -> Result<Self> {
        request.validate()?;
        let stop = Arc::new(AtomicBool::new(false));
        let initial = operation(clock, 0, deadline_us, &stop)?;
        let initial_task = initial.clone();
        let (tasks, receive) = RingBuffer::new(1);
        let (mut send, events) = RingBuffer::new(1);
        let stopped = stop.clone();
        let thread = std::thread::Builder::new()
            .name("reel-installation-original-check".into())
            .spawn(move || {
                if let Err(reason) = run(request, &initial_task, receive, &mut send, &stopped) {
                    let _ = send.push(SourceEvent::Failed {
                        generation: 0,
                        reason,
                    });
                }
            })?;
        Ok(Self {
            tasks,
            events,
            stop,
            thread: Some(thread),
            active: Some(initial),
            initialized: false,
            clock,
        })
    }
    pub fn active_operation(&self) -> Option<SourceOperation> {
        self.active.clone()
    }
    pub fn restart(&mut self, ticket: RestartTicket, deadline_us: u64) -> Result<SourceOperation> {
        ensure!(
            self.initialized
                && self.active.is_none()
                && !self.finished()
                && !self.stop.load(Ordering::Acquire),
            "installation source busy/stopped/uninitialized"
        );
        let operation = operation(self.clock, ticket.generation, deadline_us, &self.stop)?;
        ensure!(
            self.tasks
                .push(Task {
                    operation: operation.clone(),
                    ticket
                })
                .is_ok(),
            "installation source task queue full"
        );
        self.active = Some(operation.clone());
        Ok(operation)
    }
    pub fn event(&mut self) -> Option<SourceEvent> {
        let mut event = self.events.pop().ok()?;
        // Cancellation/deadline may race publication. A queued success cannot
        // revive that operation when the controller finally receives it.
        if !matches!(event, SourceEvent::Failed { .. }) {
            if let Some((generation, reason)) = self
                .active
                .as_ref()
                .and_then(|op| op.tick().err().map(|reason| (op.generation, reason)))
            {
                event = SourceEvent::Failed { generation, reason };
            }
        }
        self.initialized |= matches!(event, SourceEvent::Ready(_));
        self.active = None;
        Some(event)
    }
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(operation) = &self.active {
            operation.cancel();
        }
    }
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}
impl Drop for SourceWorker {
    fn drop(&mut self) {
        self.request_stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn operation(
    clock: OutputClock,
    generation: u64,
    deadline_us: u64,
    stop: &Arc<AtomicBool>,
) -> Result<SourceOperation> {
    let now = clock
        .now_us()
        .ok_or_else(|| anyhow::anyhow!("installation source clock unavailable"))?;
    ensure!(
        deadline_us > now
            && deadline_us <= now.saturating_add(30_000_000)
            && deadline_us <= MAX_SESSION_MS * 1000,
        "installation source deadline profile"
    );
    Ok(SourceOperation {
        generation,
        deadline_us,
        clock,
        cancel: Arc::new(AtomicBool::new(false)),
        stop: stop.clone(),
    })
}
fn run(
    request: SessionRequest,
    initial: &SourceOperation,
    mut tasks: Consumer<Task>,
    events: &mut Producer<SourceEvent>,
    stopped: &AtomicBool,
) -> std::result::Result<(), SourceFailure> {
    let loaded = LoadedProgramme::load(&request.original, || initial.check())
        .map_err(|_| initial.failure())?;
    loaded
        .recheck_controls(|| initial.check())
        .map_err(|_| initial.failure())?;
    let prepared = super::super::prepare(
        &request.selection(),
        loaded.inputs(),
        initial.control().map_err(|_| initial.failure())?,
    )
    .map_err(|_| initial.failure())?;
    // Reuse one absolute deadline across load, initial prepare and fresh seal
    // verification. Birth at Run origin includes all setup in the 120 s limit.
    let mut transport = prepared
        .transport(0, initial.control().map_err(|_| initial.failure())?)
        .map_err(|_| initial.failure())?;
    initial.tick()?;
    transport.poll(
        initial.clock.now_us().ok_or(SourceFailure::Clock)? / 1000,
        Notice::None,
    );
    if events
        .push(SourceEvent::Ready(Box::new(transport)))
        .is_err()
    {
        return Ok(());
    }
    while !stopped.load(Ordering::Acquire) {
        let Ok(task) = tasks.pop() else {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        };
        let op = &task.operation;
        let checked = (|| -> Result<RestartPermit> {
            op.check()?;
            loaded.recheck_controls(|| op.check())?;
            let permit = prepared.restart_permit(task.ticket, op.control()?)?;
            op.check()?;
            Ok(permit)
        })();
        let event = match checked {
            Ok(permit) => SourceEvent::RestartChecked {
                generation: op.generation,
                permit,
            },
            Err(_) => SourceEvent::Failed {
                generation: op.generation,
                reason: op.failure(),
            },
        };
        if events.push(event).is_err() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending() -> (SourceWorker, Producer<SourceEvent>) {
        let clock = OutputClock::start().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let active =
            Some(operation(clock, 0, clock.now_us().unwrap() + 30_000_000, &stop).unwrap());
        let (tasks, _) = RingBuffer::new(1);
        let (send, events) = RingBuffer::new(1);
        (
            SourceWorker {
                tasks,
                events,
                stop,
                thread: None,
                active,
                initialized: false,
                clock,
            },
            send,
        )
    }
    fn transport() -> Box<Transport> {
        Box::new(
            Transport::new(
                vec![0.0; 20000].into(),
                "Engineering signal\n".into(),
                [7; 32],
                1,
                0,
            )
            .unwrap(),
        )
    }
    #[test]
    fn cancelled_or_expired_queued_ready_never_initializes_the_port() {
        for cancelled in [false, true] {
            let (mut worker, mut send) = pending();
            assert!(send.push(SourceEvent::Ready(transport())).is_ok());
            let op = worker.active.as_mut().unwrap();
            if cancelled {
                op.cancel();
            } else {
                op.deadline_us = 0;
            }
            assert!(
                matches!(worker.event(), Some(SourceEvent::Failed { generation: 0, reason }) if reason == if cancelled { SourceFailure::Cancelled } else { SourceFailure::Deadline })
            );
            assert!(!worker.initialized && worker.active_operation().is_none());
        }
    }
    #[test]
    fn cancelled_queued_permit_is_not_exposed_as_a_fresh_capability() {
        let (mut worker, mut send) = pending();
        let mut actual = transport();
        actual.enqueue("stop").unwrap();
        actual.poll(0, Notice::None);
        actual.enqueue("restart").unwrap();
        actual.poll(1, Notice::None);
        let ticket = actual.restart_ticket().unwrap();
        worker.active.as_mut().unwrap().generation = ticket.generation;
        assert!(
            send.push(SourceEvent::RestartChecked {
                generation: ticket.generation,
                permit: RestartPermit {
                    session: ticket.session,
                    generation: ticket.generation,
                    seal: ticket.seal
                }
            })
            .is_ok()
        );
        actual.enqueue("stop").unwrap();
        actual.poll(2, Notice::None);
        worker.active_operation().unwrap().cancel();
        assert!(matches!(
            worker.event(),
            Some(SourceEvent::Failed {
                generation: 1,
                reason: SourceFailure::Cancelled
            })
        ));
        assert_eq!(actual.snapshot().state, State::Stopped);
    }
    #[test]
    fn source_deadline_cannot_reset_or_extend_the_profile() {
        let clock = OutputClock::start().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        assert!(operation(clock, 0, 0, &stop).is_err());
        assert!(operation(clock, 0, clock.now_us().unwrap() + 31_000_000, &stop).is_err());
        let (counter, frequency) = clock.origin();
        let old = OutputClock::from_origin(counter - frequency * 119, frequency).unwrap();
        assert!(operation(old, 1, 121_000_000, &stop).is_err());
        let op = operation(clock, 1, clock.now_us().unwrap() + 30_000_000, &stop).unwrap();
        assert!(op.control().unwrap().timeout <= Duration::from_secs(30));
        stop.store(true, Ordering::Release);
        assert_eq!(op.tick(), Err(SourceFailure::Cancelled));
    }
}

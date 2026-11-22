//! Explicit output port, not a foreground installation session. Call only from
//! the owned native worker: blocking probe/open/drop run on its backend thread.
//! Creating this port opens no endpoint. Only explicit operations query/open it.
//! The complete session/terminal/process profile remains a separate caller.
use super::*;
use cpal::traits::{DeviceTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread::JoinHandle,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

const NONE: u64 = u64::MAX;
const SESSION_US: u64 = MAX_SESSION_MS * 1000;
const CLEANUP_US: u64 = 2_000_000;

/// One system-wide monotonic domain, preserved across parent/worker/restart.
/// This observes clocks only, never endpoints. A new invocation chooses origin.
#[derive(Clone, Copy)]
pub struct OutputClock {
    counter: i64,
    frequency: i64,
}
impl OutputClock {
    pub fn start() -> Result<Self> {
        let (mut counter, mut frequency) = (0, 0);
        unsafe {
            QueryPerformanceFrequency(&mut frequency)?;
            QueryPerformanceCounter(&mut counter)?;
        }
        Self::from_origin(counter, frequency)
    }
    pub fn from_origin(counter: i64, frequency: i64) -> Result<Self> {
        let (mut actual, mut now) = (0, 0);
        unsafe {
            QueryPerformanceFrequency(&mut actual)?;
            QueryPerformanceCounter(&mut now)?;
        }
        ensure!(
            frequency > 0 && frequency == actual && counter > 0 && now >= counter,
            "installation monotonic origin profile"
        );
        let clock = Self { counter, frequency };
        ensure!(
            clock.elapsed_at(now).is_some_and(|us| us < SESSION_US),
            "installation expired origin"
        );
        Ok(clock)
    }
    pub fn origin(&self) -> (i64, i64) {
        (self.counter, self.frequency)
    }
    pub(super) fn elapsed_at(&self, counter: i64) -> Option<u64> {
        let ticks = counter.checked_sub(self.counter)?;
        if ticks < 0 {
            return None;
        }
        u64::try_from(ticks as u128 * 1_000_000 / self.frequency as u128).ok()
    }
    pub fn now_us(&self) -> Option<u64> {
        let mut now = 0;
        unsafe { QueryPerformanceCounter(&mut now) }.ok()?;
        self.elapsed_at(now)
    }
    fn native_us(&self, nanos: u128) -> Option<u64> {
        // WASAPI/CPAL timestamps share the QPC 100 ns grid.
        let origin = self.counter as u128 * 10_000_000 / self.frequency as u128 * 100;
        u64::try_from(nanos.checked_sub(origin)? / 1000).ok()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStage {
    Probe,
    Open,
    Close,
}
impl OutputStage {
    fn limit_us(self) -> u64 {
        SESSION_US + if self == Self::Close { CLEANUP_US } else { 0 }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFailure {
    Cancelled,
    Deadline,
    Backend,
    Binding,
    Buffer,
    Clock,
    Busy,
}

#[derive(Clone)]
pub struct OutputOperation {
    pub generation: u64,
    pub stage: OutputStage,
    deadline_us: u64,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    clock: OutputClock,
}
impl OutputOperation {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    fn tick(&self) -> std::result::Result<(), OutputFailure> {
        if self.cancel.load(Ordering::Acquire) || self.stop.load(Ordering::Acquire) {
            return Err(OutputFailure::Cancelled);
        }
        let now = self.clock.now_us().ok_or(OutputFailure::Clock)?;
        if now >= self.deadline_us || now >= self.stage.limit_us() {
            return Err(OutputFailure::Deadline);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct OutputStats {
    pub callbacks: u64,
    pub owned_callbacks: u64,
    pub submitted_source_frames: u64,
    pub largest_callback_frames: u32,
    pub first_ready_observed_us: Option<u64>,
    pub first_source_predicted_us: Option<u64>,
    pub last_callback_native_us: Option<u64>,
    pub last_observed_us: Option<u64>,
    pub fault_observed_us: Option<u64>,
}
struct Facts {
    failed: AtomicBool,
    callbacks: AtomicU64,
    owned_callbacks: AtomicU64,
    submitted: AtomicU64,
    largest: AtomicU32,
    ready: AtomicU64,
    first_source_prediction: AtomicU64,
    native: AtomicU64,
    observed: AtomicU64,
    fault: AtomicU64,
}
fn value(n: u64) -> Option<u64> {
    (n != NONE).then_some(n)
}
impl Facts {
    fn new() -> Self {
        Self {
            failed: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            owned_callbacks: AtomicU64::new(0),
            submitted: AtomicU64::new(0),
            largest: AtomicU32::new(0),
            ready: AtomicU64::new(NONE),
            first_source_prediction: AtomicU64::new(NONE),
            native: AtomicU64::new(NONE),
            observed: AtomicU64::new(NONE),
            fault: AtomicU64::new(NONE),
        }
    }
    fn fail(&self, now: Option<u64>) {
        if let Some(now) = now {
            let _ = self
                .fault
                .compare_exchange(NONE, now, Ordering::AcqRel, Ordering::Acquire);
        }
        self.failed.store(true, Ordering::Release);
    }
    fn stats(&self) -> OutputStats {
        OutputStats {
            callbacks: self.callbacks.load(Ordering::Acquire),
            owned_callbacks: self.owned_callbacks.load(Ordering::Acquire),
            submitted_source_frames: self.submitted.load(Ordering::Acquire),
            largest_callback_frames: self.largest.load(Ordering::Acquire),
            first_ready_observed_us: value(self.ready.load(Ordering::Acquire)),
            first_source_predicted_us: value(self.first_source_prediction.load(Ordering::Acquire)),
            last_callback_native_us: value(self.native.load(Ordering::Acquire)),
            last_observed_us: value(self.observed.load(Ordering::Acquire)),
            fault_observed_us: value(self.fault.load(Ordering::Acquire)),
        }
    }
}

/// Off-callback sending end. Native readiness is independent of opening/play().
/// A returned Callback on refusal still owns its actual transport.
pub struct OutputSink {
    sender: Option<Producer<Callback>>,
    facts: Arc<Facts>,
    generation: u64,
    attached: bool,
    operation: OutputOperation,
}
impl OutputSink {
    pub fn ready(&self) -> bool {
        self.facts.ready.load(Ordering::Acquire) != NONE
            && !self.failed()
            && self.operation.tick().is_ok()
    }
    pub fn failed(&self) -> bool {
        self.facts.failed.load(Ordering::Acquire)
            || self.operation.cancel.load(Ordering::Acquire)
            || self.operation.stop.load(Ordering::Acquire)
    }
    pub fn stats(&self) -> OutputStats {
        self.facts.stats()
    }
    pub fn attach(&mut self, callback: Callback) -> std::result::Result<(), Box<Callback>> {
        if !self.ready() || self.attached || callback.generation() != Some(self.generation) {
            return Err(Box::new(callback));
        }
        self.reclaim_closed();
        let Some(sender) = self.sender.as_mut() else {
            return Err(Box::new(callback));
        };
        match sender.push(callback) {
            Ok(()) => {
                self.attached = true;
                Ok(())
            }
            Err(rtrb::PushError::Full(callback)) => Err(Box::new(callback)),
        }
    }
    /// Closing a receiver can race its final pop. Dropping an abandoned sender
    /// releases any unconsumed Callback on this control/teardown thread.
    pub fn reclaim_closed(&mut self) -> bool {
        if self.sender.as_ref().is_some_and(Producer::is_abandoned) {
            self.sender.take();
        }
        self.sender.is_none()
    }
}
struct Slot {
    incoming: Consumer<Callback>,
    owner: Option<Callback>,
    facts: Arc<Facts>,
    clock: OutputClock,
    operation: OutputOperation,
}
fn slot(operation: OutputOperation) -> (OutputSink, Slot) {
    let (sender, incoming) = RingBuffer::new(1);
    let facts = Arc::new(Facts::new());
    (
        OutputSink {
            sender: Some(sender),
            facts: facts.clone(),
            generation: operation.generation,
            attached: false,
            operation: operation.clone(),
        },
        Slot {
            incoming,
            owner: None,
            facts,
            clock: operation.clock,
            operation,
        },
    )
}
impl Slot {
    fn fill(&mut self, out: &mut [f32], timestamps: Option<(u128, u128)>) {
        out.fill(0.0);
        let observed = self.clock.now_us();
        let last = self.facts.observed.load(Ordering::Acquire);
        let valid = observed.is_some_and(|now| last == NONE || now >= last)
            && !out.is_empty()
            && out.len() % 2 == 0
            && out.len() <= MAX_CALLBACK_FRAMES * 2;
        let (native, predicted) = match timestamps {
            Some((callback, playback)) if playback >= callback => (
                self.clock.native_us(callback),
                self.clock.native_us(playback),
            ),
            Some(_) => (None, None),
            None => (observed, None), // Controlled fake backend only; real WASAPI supplies both.
        };
        let last_native = self.facts.native.load(Ordering::Acquire);
        if !valid
            || !native.is_some_and(|now| last_native == NONE || now >= last_native)
            || (timestamps.is_some() && predicted.is_none())
        {
            self.facts.fail(observed);
        }
        let waiting_for_source = self.facts.submitted.load(Ordering::Acquire) == 0;
        if self.operation.cancel.load(Ordering::Acquire)
            || self.operation.stop.load(Ordering::Acquire)
            || (waiting_for_source
                && (self.operation.tick().is_err()
                    || predicted.is_some_and(|us| us >= self.operation.deadline_us)))
        {
            self.facts.fail(observed);
        }
        if let Some(now) = observed {
            self.facts.observed.store(now, Ordering::Release);
        }
        if let Some(now) = native {
            self.facts.native.store(now, Ordering::Release);
        }
        self.facts.callbacks.fetch_add(1, Ordering::Relaxed);
        self.facts.largest.fetch_max(
            (out.len() / 2).min(u32::MAX as usize) as u32,
            Ordering::Relaxed,
        );
        if valid && !self.facts.failed.load(Ordering::Acquire) {
            let _ = self.facts.ready.compare_exchange(
                NONE,
                observed.unwrap(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        if self.owner.is_none() {
            self.owner = self.incoming.pop().ok();
        }
        if let Some(owner) = self.owner.as_mut() {
            if self.facts.failed.load(Ordering::Acquire) {
                owner.fail();
            }
            let emission = owner.fill_with_report(
                observed.unwrap_or_else(|| value(last).unwrap_or(0)) / 1000,
                predicted,
                out,
            );
            if let Some(emission) = emission {
                self.facts.owned_callbacks.fetch_add(1, Ordering::Relaxed);
                self.facts
                    .submitted
                    .fetch_add(emission.submitted_source_frames as u64, Ordering::Relaxed);
                if let Some(predicted) = predicted.filter(|_| emission.submitted_source_frames > 0)
                {
                    let _ = self.facts.first_source_prediction.compare_exchange(
                        NONE,
                        predicted,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
            }
            if owner.failed() || emission.is_some_and(|e| e.snapshot.state == State::Fault) {
                self.facts.fail(observed);
            }
        }
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        // Not a data callback. Drain before destroying the receive endpoint;
        // OutputSink::reclaim_closed handles a sender racing this final pop.
        while let Ok(pending) = self.incoming.pop() {
            drop(pending);
        }
        self.owner.take();
    }
}

pub enum OutputEvent {
    Probed {
        generation: u64,
        observation: EndpointObservation,
    },
    Opened {
        generation: u64,
        reported_period_frames: u32,
        sink: OutputSink,
    },
    Closed {
        generation: u64,
    },
    Failed {
        generation: u64,
        stage: OutputStage,
        reason: OutputFailure,
    },
}
struct Task {
    operation: OutputOperation,
}
trait Driver: Send + 'static {
    type Stream;
    fn probe(
        &mut self,
        binding: &EndpointBinding,
        operation: &OutputOperation,
    ) -> std::result::Result<EndpointObservation, OutputFailure>;
    fn open(
        &mut self,
        binding: &EndpointBinding,
        slot: Slot,
        operation: &OutputOperation,
    ) -> std::result::Result<(Self::Stream, u32), OutputFailure>;
}
struct Wasapi;
impl Driver for Wasapi {
    type Stream = cpal::Stream;
    fn probe(
        &mut self,
        binding: &EndpointBinding,
        operation: &OutputOperation,
    ) -> std::result::Result<EndpointObservation, OutputFailure> {
        operation.tick()?;
        let (_, observed) =
            device::observe(&binding.device_id).map_err(|_| OutputFailure::Backend)?;
        operation.tick()?;
        if &observed.binding != binding {
            return Err(OutputFailure::Binding);
        }
        Ok(observed)
    }
    fn open(
        &mut self,
        binding: &EndpointBinding,
        mut slot: Slot,
        operation: &OutputOperation,
    ) -> std::result::Result<(cpal::Stream, u32), OutputFailure> {
        operation.tick()?;
        let (device, observed) =
            device::observe(&binding.device_id).map_err(|_| OutputFailure::Backend)?;
        operation.tick()?;
        if &observed.binding != binding {
            return Err(OutputFailure::Binding);
        }
        let facts = slot.facts.clone();
        let clock = slot.clock;
        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: 48000,
            buffer_size: cpal::BufferSize::Fixed(480),
        };
        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                move |out, info| {
                    let stamp = info.timestamp();
                    slot.fill(
                        out,
                        Some((stamp.callback.as_nanos(), stamp.playback.as_nanos())),
                    );
                },
                move |_| facts.fail(clock.now_us()),
                Some(Duration::from_secs(2)),
            )
            .map_err(|_| OutputFailure::Backend)?;
        operation.tick()?;
        let period = stream.buffer_size().map_err(|_| OutputFailure::Backend)?;
        if period == 0 || period > MAX_CALLBACK_FRAMES as u32 {
            return Err(OutputFailure::Buffer);
        }
        operation.tick()?;
        stream.play().map_err(|_| OutputFailure::Backend)?;
        operation.tick()?;
        Ok((stream, period))
    }
}

/// One serial backend thread, one physical stream at most. This is a port used
/// inside the contained worker, not a substitute for foreground lifecycle rules.
/// `Drop` joins native teardown and can block on a driver; the parent must own
/// that complete worker process and enforce the profile's cleanup deadline.
pub struct OutputWorker {
    tasks: Producer<Task>,
    events: Consumer<OutputEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    active: Option<OutputOperation>,
    open_generation: Option<u64>,
    clock: OutputClock,
}
impl OutputWorker {
    pub fn new(binding: EndpointBinding, clock: OutputClock) -> Result<Self> {
        Self::spawn(Wasapi, binding, clock)
    }
    fn spawn<B: Driver>(
        mut backend: B,
        binding: EndpointBinding,
        clock: OutputClock,
    ) -> Result<Self> {
        binding.validate()?;
        let (tasks, mut receive) = RingBuffer::<Task>::new(1);
        let (mut send, events) = RingBuffer::new(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::Builder::new()
            .name("reel-installation-native-output".into())
            .spawn(move || {
                let mut stream: Option<B::Stream> = None;
                while !stopped.load(Ordering::Acquire) {
                    let Ok(task) = receive.pop() else {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    let op = task.operation;
                    let result = match op.stage {
                        OutputStage::Probe => op
                            .tick()
                            .and_then(|_| backend.probe(&binding, &op))
                            .and_then(|observation| {
                                op.tick()?;
                                Ok(OutputEvent::Probed {
                                    generation: op.generation,
                                    observation,
                                })
                            }),
                        OutputStage::Open => {
                            let (sink, slot) = slot(op.clone());
                            op.tick()
                                .and_then(|_| backend.open(&binding, slot, &op))
                                .and_then(|(opened, period)| {
                                    op.tick()?;
                                    if period == 0 || period > MAX_CALLBACK_FRAMES as u32 {
                                        return Err(OutputFailure::Buffer);
                                    }
                                    stream = Some(opened);
                                    Ok(OutputEvent::Opened {
                                        generation: op.generation,
                                        reported_period_frames: period,
                                        sink,
                                    })
                                })
                        }
                        OutputStage::Close => {
                            drop(stream.take());
                            op.tick().map(|_| OutputEvent::Closed {
                                generation: op.generation,
                            })
                        }
                    };
                    let event = match result {
                        Ok(event) => event,
                        Err(reason) => OutputEvent::Failed {
                            generation: op.generation,
                            stage: op.stage,
                            reason,
                        },
                    };
                    if send.push(event).is_err() {
                        break;
                    }
                }
                drop(stream);
            })?;
        Ok(Self {
            tasks,
            events,
            stop,
            thread: Some(thread),
            active: None,
            open_generation: None,
            clock,
        })
    }
    pub fn probe(&mut self, deadline_us: u64) -> Result<OutputOperation> {
        self.submit(OutputStage::Probe, 0, deadline_us)
    }
    pub fn open(&mut self, generation: u64, deadline_us: u64) -> Result<OutputOperation> {
        ensure!(generation > 0, "installation zero output generation");
        self.submit(OutputStage::Open, generation, deadline_us)
    }
    pub fn close(&mut self, generation: u64, deadline_us: u64) -> Result<OutputOperation> {
        self.submit(OutputStage::Close, generation, deadline_us)
    }
    fn submit(
        &mut self,
        stage: OutputStage,
        generation: u64,
        deadline_us: u64,
    ) -> Result<OutputOperation> {
        ensure!(
            self.active.is_none() && !self.stop.load(Ordering::Acquire),
            "installation output busy/stopped"
        );
        match stage {
            OutputStage::Close => ensure!(
                self.open_generation == Some(generation),
                "installation stale close"
            ),
            _ => ensure!(
                self.open_generation.is_none(),
                "installation stream already open"
            ),
        }
        let now = self
            .clock
            .now_us()
            .ok_or_else(|| anyhow::anyhow!("installation output clock unavailable"))?;
        ensure!(
            deadline_us > now
                && deadline_us <= now.saturating_add(CLEANUP_US)
                && deadline_us <= stage.limit_us(),
            "installation output stage deadline profile"
        );
        let op = OutputOperation {
            generation,
            stage,
            deadline_us,
            cancel: Arc::new(AtomicBool::new(false)),
            stop: self.stop.clone(),
            clock: self.clock,
        };
        ensure!(
            self.tasks
                .push(Task {
                    operation: op.clone()
                })
                .is_ok(),
            "installation output task queue full"
        );
        self.active = Some(op.clone());
        Ok(op)
    }
    pub fn event(&mut self) -> Option<OutputEvent> {
        let event = self.events.pop().ok()?;
        match &event {
            OutputEvent::Opened { generation, .. } => self.open_generation = Some(*generation),
            OutputEvent::Closed { .. }
            | OutputEvent::Failed {
                stage: OutputStage::Open | OutputStage::Close,
                ..
            } => self.open_generation = None,
            _ => {}
        }
        self.active = None;
        Some(event)
    }
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(op) = &self.active {
            op.cancel();
        }
    }
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
    /// Actual backend/stream teardown barrier. May block on a broken driver;
    /// the enclosing process owner, not a timer flag, bounds that failure.
    pub fn shutdown(&mut self) -> Result<()> {
        self.request_stop();
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("installation output owner panicked"))?;
        }
        Ok(())
    }
}
impl Drop for OutputWorker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
pub(super) mod controlled {
    use super::*;
    use std::sync::Mutex;
    pub struct Harness {
        slot: Arc<Mutex<Option<Slot>>>,
        open: Arc<AtomicBool>,
        close: Arc<AtomicBool>,
        pub opened: Arc<AtomicU32>,
        pub closed: Arc<AtomicU32>,
    }
    pub struct Release(Arc<AtomicBool>, Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
            self.1.store(true, Ordering::Release);
        }
    }
    impl Harness {
        pub fn release_guard(&self) -> Release {
            Release(self.open.clone(), self.close.clone())
        }
        pub fn block_open(&self) {
            self.open.store(false, Ordering::Release);
        }
        pub fn release_open(&self) {
            self.open.store(true, Ordering::Release);
        }
        pub fn block_close(&self) {
            self.close.store(false, Ordering::Release);
        }
        pub fn release_close(&self) {
            self.close.store(true, Ordering::Release);
        }
        pub fn fill(&self, frames: usize) -> Vec<f32> {
            let mut out = vec![0.0; frames * 2];
            if let Some(slot) = self.slot.lock().unwrap().as_mut() {
                slot.fill(&mut out, None);
            }
            out
        }
    }
    struct Backend {
        harness: Harness,
    }
    struct Stream {
        slot: Arc<Mutex<Option<Slot>>>,
        release: Arc<AtomicBool>,
        closed: Arc<AtomicU32>,
    }
    impl Drop for Stream {
        fn drop(&mut self) {
            while !self.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.slot.lock().unwrap().take();
            self.closed.fetch_add(1, Ordering::Release);
        }
    }
    impl Driver for Backend {
        type Stream = Stream;
        fn probe(
            &mut self,
            binding: &EndpointBinding,
            _: &OutputOperation,
        ) -> std::result::Result<EndpointObservation, OutputFailure> {
            Ok(EndpointObservation {
                binding: binding.clone(),
                sample_rate: 48000,
                channels: 2,
                format: "f32".into(),
                requested_buffer_frames: 480,
                effective_buffer_checked: false,
                device_playback_checked: false,
            })
        }
        fn open(
            &mut self,
            _: &EndpointBinding,
            slot: Slot,
            _: &OutputOperation,
        ) -> std::result::Result<(Stream, u32), OutputFailure> {
            self.harness.opened.fetch_add(1, Ordering::Release);
            while !self.harness.open.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            *self.harness.slot.lock().unwrap() = Some(slot);
            Ok((
                Stream {
                    slot: self.harness.slot.clone(),
                    release: self.harness.close.clone(),
                    closed: self.harness.closed.clone(),
                },
                480,
            ))
        }
    }
    pub fn output(binding: EndpointBinding, clock: OutputClock) -> (OutputWorker, Harness) {
        let slot = Arc::new(Mutex::new(None));
        let open = Arc::new(AtomicBool::new(true));
        let close = Arc::new(AtomicBool::new(true));
        let opened = Arc::new(AtomicU32::new(0));
        let closed = Arc::new(AtomicU32::new(0));
        let clone = Harness {
            slot: slot.clone(),
            open: open.clone(),
            close: close.clone(),
            opened: opened.clone(),
            closed: closed.clone(),
        };
        let worker = OutputWorker::spawn(Backend { harness: clone }, binding, clock).unwrap();
        (
            worker,
            Harness {
                slot,
                open,
                close,
                opened,
                closed,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    fn binding() -> EndpointBinding {
        EndpointBinding {
            profile: PROFILE.into(),
            device_id: "wasapi:{0.0.0.00000000}.{11111111-1111-1111-1111-111111111111}".into(),
            os_build: 26300,
            driver_version: "test-only".into(),
            configuration_sha256: "a".repeat(64),
        }
    }
    fn operation(clock: OutputClock, generation: u64) -> OutputOperation {
        OutputOperation {
            generation,
            stage: OutputStage::Open,
            deadline_us: clock.now_us().unwrap() + 2_000_000,
            cancel: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            clock,
        }
    }
    fn engine() -> Box<Transport> {
        let mut values = vec![0.0; 20000];
        values[9700] = 0.125;
        let mut t = Transport::new(values.into(), "Texto español\n".into(), [5; 32], 2, 0).unwrap();
        t.enqueue("start").unwrap();
        t.poll(0, Notice::None);
        Box::new(t)
    }
    #[test]
    fn actual_callback_readiness_precedes_source_attachment_and_stop() {
        let clock = OutputClock::start().unwrap();
        let (mut sink, mut slot) = slot(operation(clock, 1));
        let (mut control, callback) = callback_pair(engine());
        let callback = match sink.attach(callback) {
            Err(owned) => *owned,
            Ok(()) => panic!("source attached before callback readiness"),
        };
        let mut out = [1.0; 960];
        slot.fill(&mut out, None);
        assert!(sink.ready() && out.iter().all(|v| *v == 0.0));
        assert_eq!(sink.stats().submitted_source_frames, 0);
        control.command(Command::Stop).unwrap();
        assert!(sink.attach(callback).is_ok());
        slot.fill(&mut out, None);
        let actual = control.returned().unwrap();
        assert_eq!(actual.snapshot().state, State::Stopped);
        assert_eq!(actual.snapshot().source_frame, 0);
        assert_eq!(sink.stats().submitted_source_frames, 0);
        assert_eq!(sink.stats().first_source_predicted_us, None);
        assert!(out.iter().all(|v| *v == 0.0));
    }
    #[test]
    fn teardown_reclaims_callback_that_never_reached_a_native_fill() {
        let clock = OutputClock::start().unwrap();
        let (mut sink, mut slot) = slot(operation(clock, 1));
        slot.fill(&mut [0.0; 2], None);
        let transport = engine();
        let pointer = (&*transport as *const Transport) as usize;
        let (mut control, callback) = callback_pair(transport);
        control.command(Command::Stop).unwrap();
        assert!(sink.attach(callback).is_ok());
        drop(slot);
        assert!(sink.reclaim_closed());
        let mut actual = control.returned().unwrap();
        actual.poll(clock.now_us().unwrap() / 1000, Notice::None);
        assert_eq!((&*actual as *const Transport) as usize, pointer);
        assert_eq!(actual.snapshot().state, State::Stopped);
        assert_eq!(actual.snapshot().source_frame, 0);
    }
    #[test]
    fn cancelled_handoff_and_oversize_buffer_emit_no_source_frames() {
        for cancel in [true, false] {
            let clock = OutputClock::start().unwrap();
            let op = operation(clock, 1);
            let (mut sink, mut slot) = slot(op.clone());
            slot.fill(&mut [0.0; 2], None);
            let (mut control, callback) = callback_pair(engine());
            assert!(sink.attach(callback).is_ok());
            if cancel {
                op.cancel();
            }
            let mut out = vec![1.0; if cancel { 960 } else { 4098 }];
            slot.fill(&mut out, None);
            let actual = control.returned().unwrap();
            assert_eq!(actual.snapshot().state, State::Fault);
            assert_eq!(actual.snapshot().source_frame, 0);
            assert_eq!(sink.stats().submitted_source_frames, 0);
            assert!(sink.failed() && out.iter().all(|v| *v == 0.0));
        }
    }
    #[test]
    fn native_slot_preserves_every_frame_and_pause_in_source_and_interval() {
        let clock = OutputClock::start().unwrap();
        let (mut sink, mut slot) = slot(operation(clock, 1));
        slot.fill(&mut [0.0; 2], None);
        let (mut control, callback) = callback_pair(engine());
        assert!(sink.attach(callback).is_ok());
        let mut captured = Vec::new();
        let mut source_paused = false;
        let mut interval_paused = false;
        let mut returned = None;
        for n in 0..256 {
            let frames = [1, 511, 2048, 32, 480][n % 5];
            let mut out = vec![0.0; frames * 2];
            slot.fill(&mut out, None);
            captured.extend(out);
            let report = control.report().unwrap();
            if (!source_paused
                && report.snapshot.state == State::Playing
                && report.snapshot.source_frame >= 1000)
                || (!interval_paused && report.snapshot.state == State::LoopWait)
            {
                if report.snapshot.state == State::Playing {
                    source_paused = true;
                } else {
                    interval_paused = true;
                }
                let before = report.snapshot;
                control.command(Command::Pause).unwrap();
                for _ in 0..3 {
                    let mut silent = [1.0; 960];
                    slot.fill(&mut silent, None);
                    let actual = control.report().unwrap().snapshot;
                    assert_eq!(actual.state, State::Paused);
                    assert_eq!(
                        (actual.source_frame, actual.interval_frame),
                        (before.source_frame, before.interval_frame)
                    );
                    assert!(silent.iter().all(|v| *v == 0.0));
                }
                control.command(Command::Resume).unwrap();
            }
            if let Some(actual) = control.returned() {
                returned = Some(actual);
                break;
            }
        }
        let returned = returned.unwrap();
        assert_eq!(returned.snapshot().state, State::Completed);
        assert!(source_paused && interval_paused);
        let mut expected = vec![0.0; 64000];
        expected[9700] = 0.125;
        expected[44000 + 9700] = 0.125;
        assert_eq!(&captured[..expected.len()], expected);
        assert!(captured[expected.len()..].iter().all(|v| *v == 0.0));
        assert_eq!(sink.stats().submitted_source_frames, 20000);
        assert_eq!(sink.stats().first_source_predicted_us, None); // fake sink is not WASAPI/capture evidence.
    }
    struct Fake {
        current: Arc<Mutex<Option<Slot>>>,
        entered: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
        oversized: bool,
    }
    struct FakeStream(Arc<Mutex<Option<Slot>>>);
    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.0.lock().unwrap().take();
        }
    }
    impl Driver for Fake {
        type Stream = FakeStream;
        fn probe(
            &mut self,
            bound: &EndpointBinding,
            _: &OutputOperation,
        ) -> std::result::Result<EndpointObservation, OutputFailure> {
            Ok(EndpointObservation {
                binding: bound.clone(),
                sample_rate: 48000,
                channels: 2,
                format: "f32".into(),
                requested_buffer_frames: 480,
                effective_buffer_checked: false,
                device_playback_checked: false,
            })
        }
        fn open(
            &mut self,
            _: &EndpointBinding,
            slot: Slot,
            _: &OutputOperation,
        ) -> std::result::Result<(FakeStream, u32), OutputFailure> {
            self.entered.store(true, Ordering::Release);
            while !self.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            *self.current.lock().unwrap() = Some(slot);
            Ok((
                FakeStream(self.current.clone()),
                if self.oversized { 4096 } else { 480 },
            ))
        }
    }
    type FakeFixture = (
        Fake,
        Arc<Mutex<Option<Slot>>>,
        Arc<AtomicBool>,
        Arc<AtomicBool>,
    );
    fn fake(oversized: bool) -> FakeFixture {
        let current = Arc::new(Mutex::new(None));
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        (
            Fake {
                current: current.clone(),
                entered: entered.clone(),
                release: release.clone(),
                oversized,
            },
            current,
            entered,
            release,
        )
    }
    fn event(worker: &mut OutputWorker) -> OutputEvent {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(event) = worker.event() {
                return event;
            }
            assert!(
                Instant::now() < deadline,
                "native fixture operation did not finish"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    struct Release(Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    #[test]
    fn blocked_open_keeps_transport_in_controller_and_cancels_late_native_completion() {
        let clock = OutputClock::start().unwrap();
        let (driver, current, entered, release) = fake(false);
        let mut worker = OutputWorker::spawn(driver, binding(), clock).unwrap();
        let release = Release(release);
        let mut transport = engine();
        let pointer = (&*transport as *const Transport) as usize;
        let op = worker.open(1, clock.now_us().unwrap() + 2_000_000).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        transport.enqueue("stop").unwrap();
        transport.poll(clock.now_us().unwrap() / 1000, Notice::None);
        op.cancel();
        assert_eq!(transport.snapshot().state, State::Stopped);
        assert_eq!((&*transport as *const Transport) as usize, pointer);
        assert!(current.lock().unwrap().is_none());
        drop(release);
        assert!(matches!(
            event(&mut worker),
            OutputEvent::Failed {
                reason: OutputFailure::Cancelled,
                ..
            }
        ));
        assert!(current.lock().unwrap().is_none());
        transport.enqueue("restart").unwrap();
        transport.poll(clock.now_us().unwrap() / 1000, Notice::None);
        assert!(transport.restart_ticket().is_ok());
    }
    #[test]
    fn output_worker_closes_actual_sink_and_refuses_bad_period() {
        for oversized in [false, true] {
            let clock = OutputClock::start().unwrap();
            let (driver, current, _, release) = fake(oversized);
            release.store(true, Ordering::Release);
            let mut worker = OutputWorker::spawn(driver, binding(), clock).unwrap();
            worker.open(1, clock.now_us().unwrap() + 2_000_000).unwrap();
            match event(&mut worker) {
                OutputEvent::Opened {
                    mut sink,
                    reported_period_frames,
                    ..
                } => {
                    assert!(!oversized && reported_period_frames == 480);
                    current
                        .lock()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .fill(&mut [0.0; 2], None);
                    assert!(sink.ready());
                    assert!(worker.open(2, clock.now_us().unwrap() + 2_000_000).is_err());
                    worker
                        .close(1, clock.now_us().unwrap() + 2_000_000)
                        .unwrap();
                    assert!(matches!(
                        event(&mut worker),
                        OutputEvent::Closed { generation: 1 }
                    ));
                    assert!(current.lock().unwrap().is_none() && sink.reclaim_closed());
                }
                OutputEvent::Failed {
                    reason: OutputFailure::Buffer,
                    ..
                } => assert!(oversized && current.lock().unwrap().is_none()),
                _ => panic!("unexpected fake native operation"),
            }
        }
    }
    #[test]
    fn reversed_native_clock_faults_without_an_extra_source_submission() {
        let clock = OutputClock::start().unwrap();
        let (mut sink, mut slot) = slot(operation(clock, 1));
        let (mut control, callback) = callback_pair(engine());
        let (counter, frequency) = clock.origin();
        let origin = counter as u128 * 10_000_000 / frequency as u128 * 100;
        slot.fill(&mut [0.0; 2], Some((origin, origin)));
        assert!(sink.attach(callback).is_ok());
        slot.fill(&mut [0.0; 2], Some((origin + 1000, origin + 2000)));
        assert_eq!(sink.stats().submitted_source_frames, 1);
        assert_eq!(sink.stats().first_source_predicted_us, Some(2));
        let mut out = [1.0; 2];
        slot.fill(&mut out, Some((origin, origin + 2000)));
        assert!(sink.failed() && out == [0.0; 2]);
        assert_eq!(sink.stats().submitted_source_frames, 1);
        assert_eq!(control.returned().unwrap().snapshot().state, State::Fault);
    }
    #[test]
    fn malformed_native_timestamps_cannot_establish_readiness() {
        let clock = OutputClock::start().unwrap();
        let (counter, frequency) = clock.origin();
        let origin = counter as u128 * 10_000_000 / frequency as u128 * 100;
        for stamps in [(origin - 1000, origin), (origin + 1000, origin)] {
            let (sink, mut slot) = slot(operation(clock, 1));
            let mut out = [1.0; 2];
            slot.fill(&mut out, Some(stamps));
            assert!(!sink.ready() && sink.failed() && out == [0.0; 2]);
            assert_eq!(sink.stats().submitted_source_frames, 0);
            assert_eq!(sink.stats().first_ready_observed_us, None);
        }
    }
    #[test]
    fn expired_session_stops_the_actual_core_without_a_backend_fault() {
        let clock = OutputClock::start().unwrap();
        let (mut sink, mut slot) = slot(operation(clock, 1));
        let (mut control, callback) = callback_pair(engine());
        slot.fill(&mut [0.0; 2], None);
        assert!(sink.attach(callback).is_ok());
        slot.fill(&mut [0.0; 2], None);
        assert_eq!(sink.stats().submitted_source_frames, 1);
        slot.clock.counter -= slot.clock.frequency * 120;
        let mut out = [1.0; 2];
        slot.fill(&mut out, None);
        let actual = control.returned().unwrap();
        assert_eq!(actual.snapshot().state, State::Stopped);
        assert!(actual.snapshot().session_closed);
        assert_eq!(sink.stats().submitted_source_frames, 1);
        assert!(!sink.failed() && out == [0.0; 2]);
    }
    #[test]
    fn session_expiry_allows_only_the_cleanup_tail() {
        let clock = OutputClock::start().unwrap();
        let (driver, current, _, release) = fake(false);
        release.store(true, Ordering::Release);
        let mut worker = OutputWorker::spawn(driver, binding(), clock).unwrap();
        worker.open(1, clock.now_us().unwrap() + 2_000_000).unwrap();
        assert!(matches!(event(&mut worker), OutputEvent::Opened { .. }));
        worker.clock.counter -= worker.clock.frequency * 121;
        worker.close(1, SESSION_US + CLEANUP_US).unwrap();
        assert!(matches!(
            event(&mut worker),
            OutputEvent::Closed { generation: 1 }
        ));
        assert!(current.lock().unwrap().is_none());
        assert!(worker.open(2, SESSION_US + CLEANUP_US).is_err());
        assert!(worker.probe(SESSION_US + CLEANUP_US).is_err());
        let mut op = operation(worker.clock, 1);
        op.stage = OutputStage::Close;
        op.deadline_us = SESSION_US + CLEANUP_US;
        assert_eq!(op.tick(), Ok(()));
        op.clock.counter -= op.clock.frequency;
        assert_eq!(op.tick(), Err(OutputFailure::Deadline));
    }
    #[test]
    #[ignore = "isolated allocator fixture only; explicitly launched by parent"]
    fn slot_allocation_fixture() {
        let clock = OutputClock::start().unwrap();
        let (mut sink, mut slot) = slot(operation(clock, 1));
        let (mut control, callback) = callback_pair(engine());
        slot.fill(&mut [0.0; 2], None);
        assert!(sink.attach(callback).is_ok());
        let mut out = [0.0; 2];
        super::super::callback::assert_no_allocations(|| {
            for _ in 0..260 {
                slot.fill(&mut out, None);
            }
        });
        assert_eq!(control.returned().unwrap().snapshot().state, State::Fault);
        assert_eq!(sink.stats().submitted_source_frames, 256);
        assert!(sink.failed());
    }
    #[test]
    fn native_data_fill_handoff_and_overflow_do_not_allocate_or_free() {
        super::super::callback::run_fixture(
            "spoken_programme::installation::native::output::tests::slot_allocation_fixture",
        );
    }
    #[test]
    fn restored_clock_requires_the_same_system_frequency_and_nonfuture_origin() {
        let clock = OutputClock::start().unwrap();
        let (counter, frequency) = clock.origin();
        assert!(OutputClock::from_origin(counter, frequency).is_ok());
        assert!(OutputClock::from_origin(counter, frequency + 1).is_err());
        assert!(OutputClock::from_origin(i64::MAX, frequency).is_err());
    }
}

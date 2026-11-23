//! One eight-command pool shared by completed input lines and foreground IPC.
//! No console/device/file operations, serialized leases or unbounded queues.
use super::Command;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU64, Ordering},
};

const OPEN: u8 = 0;
const CLOSED: u8 = 1;
const FAULT: u8 = 2;
const SHOWN: u64 = 1 << 63;
struct Pool {
    // Zero is free; otherwise the original positive i64 QPC counter and one
    // display bit are published in the SAME atomic operation as ownership.
    slots: [AtomicU64; 8],
    state: AtomicU8,
    generation: AtomicU64,
}
#[derive(Clone)]
pub struct CommandPool(Arc<Pool>);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    InvalidLine,
    InvalidCounter,
    Closed,
    Overflow,
    Foreign,
    Queue,
}
impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "installation input {self:?}")
    }
}
impl std::error::Error for InputError {}

/// A runtime-only reservation. Exactly one input envelope or foreground credit
/// owns it until refusal, actual worker return, final close or destruction.
pub(super) struct CommandLease {
    pool: CommandPool,
    slot: usize,
}
impl Drop for CommandLease {
    fn drop(&mut self) {
        self.pool.0.slots[self.slot].store(0, Ordering::Release);
    }
}
impl CommandLease {
    pub(super) fn mark_shown(&self) {
        self.pool.0.slots[self.slot].fetch_or(SHOWN, Ordering::AcqRel);
    }
}
/// Captured at complete line receipt; never Clone/Serde. Parsing does not retain
/// an operator's raw line. Queueing/forwarding cannot replace receipt or scope.
pub struct QueuedCommand {
    command: Command,
    received_counter: i64,
    generation: u64,
    lease: CommandLease,
}
impl QueuedCommand {
    pub fn command(&self) -> Command {
        self.command
    }
    pub fn received_counter(&self) -> i64 {
        self.received_counter
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub(super) fn unpack(
        self,
        pool: &CommandPool,
    ) -> Result<(Command, i64, u64, CommandLease), InputError> {
        if !Arc::ptr_eq(&self.lease.pool.0, &pool.0) {
            return Err(InputError::Foreign);
        }
        Ok((
            self.command,
            self.received_counter,
            self.generation,
            self.lease,
        ))
    }
}
impl Default for CommandPool {
    fn default() -> Self {
        Self::new()
    }
}
impl CommandPool {
    pub fn new() -> Self {
        Self(Arc::new(Pool {
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
            state: AtomicU8::new(OPEN),
            generation: AtomicU64::new(0),
        }))
    }
    pub fn occupied(&self) -> usize {
        self.0
            .slots
            .iter()
            .filter(|slot| slot.load(Ordering::Acquire) != 0)
            .count()
    }
    /// Snapshot first, then the parent samples QPC. A legitimate reservation
    /// published during scanning cannot be mistaken for a future receipt.
    pub(super) fn unshown_receipts(&self) -> [u64; 8] {
        std::array::from_fn(|index| {
            let value = self.0.slots[index].load(Ordering::Acquire);
            if value & SHOWN == 0 { value } else { 0 }
        })
    }
    pub fn failed(&self) -> bool {
        self.0.state.load(Ordering::Acquire) == FAULT
    }
    pub fn closed(&self) -> bool {
        self.0.state.load(Ordering::Acquire) != OPEN
    }
    pub fn close(&self) {
        let _ = self
            .0
            .state
            .compare_exchange(OPEN, CLOSED, Ordering::AcqRel, Ordering::Acquire);
    }
    pub fn fault(&self) {
        self.0.state.store(FAULT, Ordering::Release);
    }
    pub(super) fn publish_generation(&self, generation: u64) {
        self.0.generation.store(generation, Ordering::Release);
    }
    pub fn line(&self, line: &str, received_counter: i64) -> Result<QueuedCommand, InputError> {
        if !line.is_ascii() || line.len() > 32 {
            return Err(InputError::InvalidLine);
        }
        let command = Command::parse(line).map_err(|_| InputError::InvalidLine)?;
        self.command(command, received_counter)
    }
    pub fn command(
        &self,
        command: Command,
        received_counter: i64,
    ) -> Result<QueuedCommand, InputError> {
        if received_counter <= 0 {
            self.fault();
            return Err(InputError::InvalidCounter);
        }
        // At most eight CAS attempts; no wait/lock in the control path. Receipt
        // publication cannot be stranded behind a preempted slot owner.
        for slot in 0..8 {
            if self.closed() {
                return Err(InputError::Closed);
            }
            if self.0.slots[slot]
                .compare_exchange(
                    0,
                    received_counter as u64,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                let lease = CommandLease {
                    pool: self.clone(),
                    slot,
                };
                if self.closed() {
                    return Err(InputError::Closed);
                }
                return Ok(QueuedCommand {
                    command,
                    received_counter,
                    generation: self.0.generation.load(Ordering::Acquire),
                    lease,
                });
            }
        }
        self.fault();
        Err(InputError::Overflow)
    }
    pub fn channel(&self) -> (InputSender, InputReceiver) {
        let (send, receive) = RingBuffer::new(8);
        (
            InputSender {
                pool: self.clone(),
                send,
            },
            InputReceiver {
                pool: self.clone(),
                receive,
            },
        )
    }
}
pub struct InputSender {
    pool: CommandPool,
    send: Producer<QueuedCommand>,
}
pub struct InputReceiver {
    pool: CommandPool,
    receive: Consumer<QueuedCommand>,
}
impl InputSender {
    /// The cooked reader calls this with the QPC sampled at complete receipt.
    /// Reservation precedes enqueue and remains owned through parent adoption.
    pub fn line(&mut self, line: &str, received_counter: i64) -> Result<(), InputError> {
        let command = self.pool.line(line, received_counter)?;
        if self.send.push(command).is_err() {
            self.pool.fault();
            return Err(InputError::Queue);
        }
        Ok(())
    }
}
impl InputReceiver {
    pub fn receive(&mut self) -> Option<QueuedCommand> {
        if self.pool.closed() {
            self.discard();
            return None;
        }
        self.receive.pop().ok()
    }
    pub fn discard(&mut self) {
        for _ in 0..8 {
            if self.receive.pop().is_err() {
                break;
            }
        }
    }
    pub fn close(&mut self) {
        self.pool.close();
        self.discard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_pool_bounds_queued_and_adopted_commands_and_returns_each_slot_once() {
        let pool = CommandPool::new();
        let (mut sender, mut receiver) = pool.channel();
        for _ in 0..4 {
            sender.line("PAUSE", 10).unwrap();
        }
        let mut adopted = Vec::new();
        for _ in 0..4 {
            adopted.push(pool.command(Command::Status, 11).unwrap());
        }
        assert_eq!(pool.occupied(), 8);
        drop(receiver.receive().unwrap());
        assert_eq!(pool.occupied(), 7);
        adopted.push(pool.command(Command::Quit, 12).unwrap());
        assert_eq!(pool.occupied(), 8);
        assert!(matches!(
            pool.command(Command::Start, 13),
            Err(InputError::Overflow)
        ));
        assert!(pool.failed());
        assert!(receiver.receive().is_none());
        drop(adopted);
        assert_eq!(pool.occupied(), 0);
        assert!(pool.command(Command::Status, 14).is_err());
    }
    #[test]
    fn malformed_lines_do_not_reserve_or_fault_and_scope_is_captured_before_queueing() {
        let pool = CommandPool::new();
        for line in [
            "bad command",
            "pausé",
            "start\0",
            "                                start",
        ] {
            assert!(matches!(pool.line(line, 1), Err(InputError::InvalidLine)));
        }
        assert_eq!(pool.occupied(), 0);
        assert!(!pool.failed());
        let (mut sender, mut receiver) = pool.channel();
        pool.publish_generation(4);
        sender.line("pause", 19).unwrap();
        pool.publish_generation(5);
        let command = receiver.receive().unwrap();
        assert_eq!(
            (
                command.command(),
                command.received_counter(),
                command.generation()
            ),
            (Command::Pause, 19, 4)
        );
        assert!(matches!(
            command.unpack(&CommandPool::new()),
            Err(InputError::Foreign)
        ));
        assert_eq!(pool.occupied(), 0);
    }
    #[test]
    fn concurrent_reservations_never_exceed_eight_and_overflow_is_terminal() {
        let pool = CommandPool::new();
        let barrier = Arc::new(std::sync::Barrier::new(17));
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let (pool, barrier) = (pool.clone(), barrier.clone());
                scope.spawn(move || {
                    let reservation = pool.command(Command::Status, 1);
                    assert!(pool.occupied() <= 8);
                    barrier.wait();
                    drop(reservation);
                });
            }
            barrier.wait();
            assert!(pool.failed() && pool.occupied() <= 8);
        });
        assert_eq!(pool.occupied(), 0);
    }
}

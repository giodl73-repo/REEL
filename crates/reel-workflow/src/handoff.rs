//! Private, append-only task recovery controls. This is not a storage engine or
//! a native protection implementation. Only a qualified adapter can supply one.
use crate::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const SCHEMA: &str = "reel.workflow-handoff.v1";
const MAX_EVENTS: usize = 128;

/// Exact immutable native admission packet. Its payload must preserve the
/// original operation, WorkRef, activation generation, object pins, source/Index
/// checkpoints and retained-owner binding. The adapter interprets and reopens it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub packet: Packet,
    pub attempt_id: String,
    pub native_binding: Artifact,
    pub lifecycle_tool: Artifact,
}
impl Ticket {
    fn check(&self) -> Result<()> {
        check_packet(&self.packet)?;
        artifact(&self.native_binding)?;
        artifact(&self.lifecycle_tool)?;
        if !token(&self.attempt_id) {
            return fail(ErrorKind::Invalid, "handoff-attempt");
        }
        Ok(())
    }
}

/// A guard must hold original admitted resources and fence the original actor
/// generation. None of these methods may substitute current/latest or allocate
/// a replacement operation. Implementations own payload validation, publication,
/// logical idempotency and exact retained-owner readback, not this journal.
pub trait ProtectedJob {
    fn register(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact>;
    /// Reconcile or finish the ORIGINAL operation through its qualified recovery
    /// protocol. Never allocate a replacement operation or blindly repeat effects.
    fn reconcile(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact>;
    fn verify_retained(
        &mut self,
        ticket: &Ticket,
        result: &ReturnedResult,
        acknowledgement: &Artifact,
    ) -> Result<()>;
}
pub trait Lifecycle {
    type Guard<'a>: ProtectedJob
    where
        Self: 'a;
    /// Verify task/attempt/closure correspondence, exact native binding and tool
    /// bytes, active original generation and source/Index freshness. Reopening
    /// after native completion must also support verifying its retained result.
    /// This method must not publish, enroll actors or replace protection.
    fn reopen<'a>(&'a mut self, ticket: &Ticket) -> Result<Self::Guard<'a>>;
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Event {
    Waiting { ticket: Box<Ticket> },
    ImportPending { result: Box<ReturnedResult> },
    Retained { acknowledgement: Artifact },
    Cancelled,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    sequence: usize,
    previous: Option<String>,
    event: Event,
}

/// Observed local journal state, never independent proof of native custody.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Waiting,
    ImportPending,
    Retained,
    Cancelled,
}

pub struct Journal {
    directory: PathBuf,
    ticket: Ticket,
    result: Option<ReturnedResult>,
    acknowledgement: Option<Artifact>,
    state: State,
    sequence: usize,
    last: String,
}
fn io_error(_: std::io::Error) -> Error {
    Error {
        kind: ErrorKind::NeedsRecovery,
        code: "handoff-journal-io-preserve".into(),
    }
}
fn ordinary(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return fail(ErrorKind::Denied, "handoff-journal-file-kind");
    }
    Ok(())
}
fn ancestors(path: &Path) -> Result<()> {
    for parent in path.ancestors() {
        ordinary(parent, true)?;
    }
    Ok(())
}
fn read(path: &Path) -> Result<Vec<u8>> {
    ordinary(path, false)?;
    let mut raw = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(io_error)?;
    if raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "handoff-record-bytes");
    }
    Ok(raw)
}
fn append(
    directory: &Path,
    sequence: usize,
    previous: Option<String>,
    event: Event,
) -> Result<String> {
    if sequence >= MAX_EVENTS {
        return fail(ErrorKind::Budget, "handoff-event-count");
    }
    let bytes = serde_json::to_vec(&Record {
        schema: SCHEMA.into(),
        sequence,
        previous,
        event,
    })
    .map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "handoff-record-encode".into(),
    })?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "handoff-record-bytes");
    }
    let path = directory.join(format!("{sequence:05}.json"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error {
                    kind: ErrorKind::Conflict,
                    code: "handoff-concurrent-transition".into(),
                }
            } else {
                io_error(e)
            }
        })?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(io_error)?;
    Ok(digest(&bytes))
}
impl Journal {
    /// Creates only a NEW control directory, after the adapter reopens existing
    /// protection. No stores, actor enrollment, source claims or payloads created.
    pub fn create(path: &Path, ticket: Ticket, lifecycle: &mut impl Lifecycle) -> Result<Self> {
        ticket.check()?;
        if !path.is_absolute() {
            return fail(ErrorKind::Invalid, "handoff-absolute-path");
        }
        ancestors(path.parent().ok_or(Error {
            kind: ErrorKind::Invalid,
            code: "handoff-parent".into(),
        })?)?;
        let _guard = lifecycle.reopen(&ticket)?;
        fs::create_dir(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error {
                    kind: ErrorKind::Conflict,
                    code: "handoff-create-new".into(),
                }
            } else {
                io_error(e)
            }
        })?;
        append(
            path,
            0,
            None,
            Event::Waiting {
                ticket: Box::new(ticket),
            },
        )?;
        Self::open(path)
    }
    /// Bounded local replay only. A corrupt/partial tail is preserved and stops
    /// recovery; no truncation, file replacement or inferred rollback is allowed.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return fail(ErrorKind::Invalid, "handoff-absolute-path");
        }
        ancestors(path)?;
        let mut names = Vec::new();
        for entry in fs::read_dir(path).map_err(io_error)? {
            names.push(entry.map_err(io_error)?.file_name());
            if names.len() > MAX_EVENTS {
                return fail(ErrorKind::Budget, "handoff-event-count");
            }
        }
        names.sort();
        let mut records = Vec::new();
        let mut last = None;
        for (sequence, name) in names.iter().enumerate() {
            if name != &std::ffi::OsString::from(format!("{sequence:05}.json")) {
                return fail(ErrorKind::NeedsRecovery, "handoff-journal-entry-set");
            }
            let raw = read(&path.join(name))?;
            let record: Record = decode(&raw)?;
            if record.schema != SCHEMA || record.sequence != sequence || record.previous != last {
                return fail(ErrorKind::Integrity, "handoff-record-chain");
            }
            records.push(record.event);
            last = Some(digest(&raw));
        }
        let Some(Event::Waiting { ticket }) = records.first() else {
            return fail(ErrorKind::NeedsRecovery, "handoff-original-ticket");
        };
        ticket.check()?;
        let mut journal = Self {
            directory: path.into(),
            ticket: *ticket.clone(),
            result: None,
            acknowledgement: None,
            state: State::Waiting,
            sequence: records.len(),
            last: last.unwrap(),
        };
        for event in records.into_iter().skip(1) {
            journal.apply(event)?;
        }
        Ok(journal)
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn ticket(&self) -> &Ticket {
        &self.ticket
    }
    fn apply(&mut self, event: Event) -> Result<()> {
        match (self.state, event) {
            (State::Waiting, Event::ImportPending { result }) => {
                check_result(&self.ticket.packet, &result)?;
                if result.attempt_id != self.ticket.attempt_id {
                    return fail(ErrorKind::Conflict, "handoff-attempt-binding");
                }
                self.result = Some(*result);
                self.state = State::ImportPending;
            }
            (State::ImportPending, Event::Retained { acknowledgement }) => {
                artifact(&acknowledgement)?;
                self.acknowledgement = Some(acknowledgement);
                self.state = State::Retained;
            }
            (State::Waiting, Event::Cancelled) => self.state = State::Cancelled,
            _ => return fail(ErrorKind::Conflict, "handoff-state-transition"),
        }
        Ok(())
    }
    fn record(&mut self, event: Event) -> Result<()> {
        // Every effecting entry point refreshes the disk before this method.
        let last = append(
            &self.directory,
            self.sequence,
            Some(self.last.clone()),
            event.clone(),
        )?;
        self.apply(event)?;
        self.sequence += 1;
        self.last = last;
        Ok(())
    }
    fn refresh(&mut self) -> Result<()> {
        let fresh = Self::open(&self.directory)?;
        if fresh.ticket != self.ticket {
            return fail(ErrorKind::Conflict, "handoff-ticket-changed");
        }
        *self = fresh;
        Ok(())
    }
    /// Cancellation changes task state only. There is intentionally no close,
    /// delete, expiry or release hook. An uncertain import cannot be cancelled.
    pub fn cancel(&mut self) -> Result<()> {
        self.refresh()?;
        if self.state == State::Cancelled {
            return Ok(());
        }
        if self.state != State::Waiting {
            return fail(ErrorKind::NeedsRecovery, "handoff-cancel-pending-import");
        }
        self.record(Event::Cancelled)
    }
    pub fn import(&mut self, result: ReturnedResult, lifecycle: &mut impl Lifecycle) -> Result<()> {
        self.refresh()?;
        if self.state != State::Waiting {
            return fail(
                ErrorKind::NeedsRecovery,
                "handoff-reconcile-original-import",
            );
        }
        check_result(&self.ticket.packet, &result)?;
        if result.attempt_id != self.ticket.attempt_id {
            return fail(ErrorKind::Conflict, "handoff-attempt-binding");
        }
        self.record(Event::ImportPending {
            result: Box::new(result),
        })?;
        // Intent is flushed BEFORE any call that can publish. Failure at any
        // later boundary leaves ImportPending, including an uncertain journal write.
        self.finish(lifecycle, false)
    }
    pub fn resume(&mut self, lifecycle: &mut impl Lifecycle) -> Result<()> {
        self.refresh()?;
        match self.state {
            State::ImportPending => self.finish(lifecycle, true),
            State::Retained => {
                let mut guard = lifecycle.reopen(&self.ticket)?;
                guard.verify_retained(
                    &self.ticket,
                    self.result.as_ref().unwrap(),
                    self.acknowledgement.as_ref().unwrap(),
                )
            }
            State::Waiting => {
                let _guard = lifecycle.reopen(&self.ticket)?;
                Ok(())
            }
            State::Cancelled => Ok(()),
        }
    }
    fn finish(&mut self, lifecycle: &mut impl Lifecycle, recovery: bool) -> Result<()> {
        let mut guard = lifecycle.reopen(&self.ticket)?;
        let result = self.result.as_ref().unwrap();
        let acknowledgement = if recovery {
            guard.reconcile(&self.ticket, result)?
        } else {
            guard.register(&self.ticket, result)?
        };
        artifact(&acknowledgement)?;
        guard.verify_retained(&self.ticket, result, &acknowledgement)?;
        self.record(Event::Retained { acknowledgement })
    }
}

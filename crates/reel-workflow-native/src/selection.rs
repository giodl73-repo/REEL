//! Explicit owned-ref transaction primitive, not asset validation or approval.
//! Callers must check candidate custody, exact native binding and project decision
//! authority before calling. No working-tree/index/branch or remote changes.
use crate::{Tool, error, git_error, hold_tool, io_error};
use reel_workflow::{Artifact, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tig_core::{LogicalPath, Object};
use tig_git::source::{GitId, Limits, Repository, proposal};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub path: String,
    pub expected: Object,
    pub bytes: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub name: String,
    pub email: String,
    pub unix_seconds: u64,
}
/// Private operational request. A decision pin is lineage, never proof of its
/// authority. This lower-level primitive deliberately has no film CLI command.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub git: Tool,
    pub repository: PathBuf,
    pub operation: String,
    pub selection: String,
    pub base: String,
    /// None creates a new owned lane. Some must equal the exact proposal base.
    pub expected: Option<String>,
    pub decision: Artifact,
    pub identity: Identity,
    pub changes: Vec<Change>,
}
#[derive(Debug, Serialize)]
pub struct Outcome {
    pub proposal: String,
    pub operation_reference: String,
    pub applied: bool,
    /// False can mean another explicit operation has since advanced the lane.
    pub currently_selected: bool,
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn validate(request: &Request) -> Result<()> {
    if request.schema != "reel.workflow-owned-selection.v1"
        || !token(&request.operation)
        || !request
            .selection
            .strip_prefix("refs/reel-workflow/selections/")
            .is_some_and(token)
        || request
            .expected
            .as_ref()
            .is_some_and(|old| old != &request.base)
        || request.decision.bytes == 0
        || request.decision.logical_id.is_empty()
        || request.decision.sha256.len() != 64
        || !request
            .decision
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(error(ErrorKind::Invalid, "selection-request"));
    }
    Ok(())
}

// Git runs without hooks, a shell, inherited Git redirection, lazy fetch or
// stderr parsing. Both elapsed time and collected stdout are bounded. The only
// mutation command is a local ref transaction; errors require exact ref readback.
fn git(request: &Request, args: &[&str], input: &[u8]) -> Result<(bool, Vec<u8>)> {
    let mut command = Command::new(&request.git.path);
    for (name, _) in std::env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(name);
        }
    }
    let mut child = command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["--no-optional-locks", "--no-replace-objects"])
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .current_dir(&request.repository)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(io_error)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error(ErrorKind::NeedsRecovery, "selection-stdout"))?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    // Inputs are <= 512 bytes; no pipe-sized write can block on a child which
    // has not started reading. Always close stdin before waiting for Git.
    let written = child
        .stdin
        .take()
        .ok_or_else(|| error(ErrorKind::NeedsRecovery, "selection-stdin"))?
        .write_all(input);
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < Duration::from_secs(5) => {
                thread::sleep(Duration::from_millis(5))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(error(ErrorKind::NeedsRecovery, "selection-git-uncertain"));
            }
        }
    };
    written.map_err(io_error)?;
    let bytes = reader
        .join()
        .map_err(|_| error(ErrorKind::NeedsRecovery, "selection-git-reader"))?
        .map_err(io_error)?;
    if bytes.len() > 4096 {
        return Err(error(ErrorKind::Budget, "selection-git-output"));
    }
    Ok((status.success(), bytes))
}
fn reference(request: &Request, name: &str) -> Result<Option<String>> {
    // for-each-ref omits dangling symbolic refs. Check them explicitly rather
    // than treating an observed dangling link as an absent owned lane.
    if git(request, &["symbolic-ref", "--quiet", name], &[])?.0 {
        return Err(error(ErrorKind::Conflict, "selection-direct-ref-required"));
    }
    let (ok, bytes) = git(
        request,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(symref)",
            name,
        ],
        &[],
    )?;
    if !ok {
        return Err(error(ErrorKind::NeedsRecovery, "selection-ref-read"));
    }
    if bytes.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| error(ErrorKind::Integrity, "selection-ref-encoding"))?;
    let fields: Vec<_> = text.trim_end_matches('\n').split('\0').collect();
    if fields.len() != 3 || fields[0] != name || !fields[2].is_empty() {
        return Err(error(ErrorKind::Conflict, "selection-direct-ref-required"));
    }
    GitId::parse(fields[1]).map_err(git_error)?;
    Ok(Some(fields[1].into()))
}

/// Build an exact-byte qualified proposal, then CAS one owned ref. Separate
/// immutable intent/completion refs never stand in for an atomic multi-ref write.
/// An uncertain original operation is reconciled, never blindly applied again.
/// `apply=false` only preflights; it writes no proposal or references.
pub fn run(request: &Request, apply: bool) -> Result<Outcome> {
    run_checked(request, apply, |_| Ok(()))
}

/// Internal semantic adapters inspect the exact detached proposal before any
/// selection intent/CAS, including recovery. Their live custody guards outlive
/// this call. A failing check leaves only a detached proposal, never selection.
pub(crate) fn run_checked(
    request: &Request,
    apply: bool,
    check: impl FnOnce(&GitId) -> Result<()>,
) -> Result<Outcome> {
    validate(request)?;
    let raw = serde_json::to_vec(request)
        .map_err(|_| error(ErrorKind::Invalid, "selection-request-json"))?;
    if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "selection-request-bytes"));
    }
    let _tool = hold_tool(&request.git)?;
    let _root = reel_assets::jobs::open_workspace_directory(&request.repository)
        .map_err(|_| error(ErrorKind::Denied, "selection-repository-namespace"))?;
    let repository = Repository::new(
        request.git.path.clone(),
        request.repository.clone(),
        Limits::default(),
    )
    .map_err(git_error)?;
    let changes = request
        .changes
        .iter()
        .map(|change| {
            Ok(proposal::Replacement {
                path: LogicalPath::parse(&change.path)
                    .map_err(|_| error(ErrorKind::Invalid, "selection-path"))?,
                expected: change.expected.clone(),
                bytes: change.bytes.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let identity = proposal::Identity {
        name: request.identity.name.clone(),
        email: request.identity.email.clone(),
        unix_seconds: request.identity.unix_seconds,
    };
    let message = format!(
        "REEL owned selection {}\nRequest SHA256 {}\n",
        request.operation,
        reel_workflow::digest(&raw)
    );
    let prepared = repository
        .check_proposal(
            GitId::parse(&request.base).map_err(git_error)?,
            &changes,
            &identity,
            &message,
        )
        .map_err(git_error)?;
    let marker = format!("refs/reel-workflow/operations/{}", request.operation);
    if !apply {
        if reference(request, &marker)?.is_some()
            || reference(request, &request.selection)? != request.expected
        {
            return Err(error(ErrorKind::Stale, "selection-preview-baseline"));
        }
        return Ok(Outcome {
            proposal: String::new(),
            operation_reference: marker,
            applied: false,
            currently_selected: false,
        });
    }
    let proposal = prepared.create().map_err(git_error)?;
    check(proposal.commit())?;
    let commit = proposal.commit().as_str();
    let completed = format!("refs/reel-workflow/applied/{}", request.operation);
    if let Some(original) = reference(request, &marker)? {
        if original != commit {
            return Err(error(ErrorKind::Conflict, "selection-operation-reused"));
        }
        return reconcile(request, commit, marker, &completed, false);
    }
    if reference(request, &completed)?.is_some() {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "selection-orphan-completion",
        ));
    }
    if reference(request, &request.selection)? != request.expected {
        return Err(error(ErrorKind::Stale, "selection-baseline-advanced"));
    }
    let zero = "0".repeat(request.base.len());
    let old = request.expected.as_deref().unwrap_or(&zero);
    // Only the successful creator of the intent may initiate CAS. A concurrent
    // retry or uncertain intent creation gets readback/recovery, never effects.
    if !git(
        request,
        &["update-ref", "--no-deref", &marker, commit, &zero],
        &[],
    )?
    .0
    {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "selection-intent-reconcile",
        ));
    }
    let attempted = git(
        request,
        &["update-ref", "--no-deref", &request.selection, commit, old],
        &[],
    );
    let confirmed = matches!(attempted, Ok((true, _)));
    if matches!(attempted, Ok((false, _)))
        && reference(request, &request.selection)? != request.expected
    {
        return Err(error(ErrorKind::Stale, "selection-cas-conflict"));
    }
    reconcile(request, commit, marker, &completed, confirmed)
}

fn reconcile(
    request: &Request,
    commit: &str,
    marker: String,
    completed: &str,
    confirmed: bool,
) -> Result<Outcome> {
    let selected = reference(request, &request.selection)?.as_deref() == Some(commit);
    match reference(request, completed)? {
        Some(original) if original == commit => (),
        Some(_) => return Err(error(ErrorKind::Conflict, "selection-completion-conflict")),
        None if confirmed || selected => {
            let zero = "0".repeat(request.base.len());
            let _ = git(
                request,
                &["update-ref", "--no-deref", completed, commit, &zero],
                &[],
            );
            if reference(request, completed)?.as_deref() != Some(commit) {
                return Err(error(
                    ErrorKind::NeedsRecovery,
                    "selection-completion-reconcile",
                ));
            }
        }
        // Intent alone proves neither CAS nor its absence. The lane may have
        // advanced or reverted after an unobserved success (the ABA case).
        None => {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "selection-reconcile-original-operation",
            ));
        }
    }
    Ok(Outcome {
        proposal: commit.into(),
        operation_reference: marker,
        applied: true,
        currently_selected: reference(request, &request.selection)?.as_deref() == Some(commit),
    })
}

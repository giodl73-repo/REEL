//! Explicit local progress evidence, never automatic resume or custody authority.
use super::{EntryResult, Plan, Report, Result};
use crate::restore;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use tig_assets::graph::Reference;
use tig_core::{Digest, Object, identity::parse_json};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ROW: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
enum Event {
    Started {
        schema: String,
        attempt: Digest,
        plan: Digest,
        source_commit: String,
        source_checkpoint: Digest,
        profile: Object,
        manifest: Object,
        entry_count: usize,
    },
    Completed {
        ordinal: usize,
        record: Reference,
        object: Object,
        action: String,
        destination_verified: bool,
    },
    Finished {
        completed_entries: usize,
        success: bool,
    },
}

/// Historical log observations only; retry must verify the actual destinations.
#[derive(Serialize)]
pub struct Summary {
    pub schema: &'static str,
    pub plan: Digest,
    pub attempt: Digest,
    pub evidence: Object,
    pub entry_count: usize,
    pub logged_completed: usize,
    pub not_logged_completed: usize,
    pub terminal_success: Option<bool>,
    pub incomplete_tail: bool,
    pub destinations_reverified: bool,
    pub source_guards_held: bool,
    pub independent_custody_verified: bool,
    pub authorizes_eviction: bool,
}

fn check_path(path: &Path, destination: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err("progress requires an absolute ordinary file path".into());
    }
    if below(path, destination) {
        return Err("progress must be outside the restore destination root".into());
    }
    for parent in path.parent().ok_or("progress parent required")?.ancestors() {
        if !restore::ordinary(parent)?.is_dir() {
            return Err("progress parent is not an ordinary directory".into());
        }
    }
    Ok(())
}

fn below(path: &Path, root: &Path) -> bool {
    // Conservative across supported local filesystems, including Windows case aliases.
    let components = |p: &Path| {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
    };
    components(path).starts_with(&components(root))
}

struct Writer {
    file: File,
    bytes: u64,
    completed: usize,
}
impl Writer {
    fn append(&mut self, event: &Event) -> Result<()> {
        let mut raw = serde_json::to_vec(event).map_err(|_| "cannot encode progress")?;
        raw.push(b'\n');
        if raw.len() > MAX_ROW || self.bytes + raw.len() as u64 > MAX_BYTES {
            return Err("progress byte budget exceeded".into());
        }
        self.file
            .write_all(&raw)
            .and_then(|()| self.file.sync_all())
            .map_err(|_| "cannot persist progress")?;
        self.bytes += raw.len() as u64;
        Ok(())
    }
    fn entry(&mut self, entry: &EntryResult) -> Result<()> {
        self.append(&Event::Completed {
            ordinal: entry.ordinal,
            record: entry.record.clone(),
            object: entry.object.clone(),
            action: entry.action.into(),
            destination_verified: entry.destination_verified,
        })?;
        self.completed += 1;
        Ok(())
    }
}

impl Plan {
    fn check_progress_path(&self, path: &Path) -> Result<()> {
        check_path(path, &self.profile.destination_root)?;
        if below(path, &self.profile.repository)
            || self
                .profile
                .byte_source
                .places
                .iter()
                .any(|p| below(path, &p.root))
        {
            return Err("progress must be outside the checkout and enrolled source caches".into());
        }
        Ok(())
    }
    fn progress_header(&self, attempt: Digest) -> Event {
        Event::Started {
            schema: "tig.project-progress.experimental-1".into(),
            attempt,
            plan: self.identity.clone(),
            source_commit: self.source_commit.clone(),
            source_checkpoint: self.checkpoint.clone(),
            profile: self.profile_object.clone(),
            manifest: self.manifest_object.clone(),
            entry_count: self.items.len(),
        }
    }

    /// Explicit apply with a fresh caller-owned evidence file. No log is reused.
    /// Each completed copy is synced before the next begins. A crash between copy
    /// and log publication can leave an unlogged destination; retry verifies it.
    pub fn run_logged(&self, expected: &Digest, path: &Path) -> Result<Report> {
        self.run_logged_observed(expected, path, |_| true)
    }

    /// Trusted caller callback runs only after the entry evidence is synced.
    /// Returning false deliberately stops the batch, retaining completed files.
    pub fn run_logged_observed(
        &self,
        expected: &Digest,
        path: &Path,
        mut observe: impl FnMut(&EntryResult) -> bool,
    ) -> Result<Report> {
        if expected != &self.identity {
            return Err("expected plan differs".into());
        }
        self.check_progress_path(path)?;
        let mut nonce = [0_u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| "cannot identify progress attempt")?;
        let attempt = Digest::blob(&nonce);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // FILE_SHARE_READ: inspect, but refuse mutation.
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(path)
            .map_err(|_| "cannot create fresh progress file")?;
        let mut writer = Writer {
            file,
            bytes: 0,
            completed: 0,
        };
        writer.append(&self.progress_header(attempt))?;
        let mut progress_failed = false;
        let result = self.run_observed(true, Some(expected), |entry| {
            if writer.entry(entry).is_err() {
                progress_failed = true;
                false
            } else {
                observe(entry)
            }
        });
        if progress_failed {
            // Do not append after a possibly torn write. Verified copies remain.
            return Err("progress write failed; explicit inspection and retry required".into());
        }
        writer.append(&Event::Finished {
            completed_entries: writer.completed,
            success: result.as_ref().is_ok_and(|report| report.failure.is_none()),
        })?;
        result
    }

    /// Read one explicitly supplied log. No destination/source reads or writes.
    /// A non-newline tail is incomplete evidence, never an acknowledged entry.
    pub fn inspect_progress(&self, expected: &Digest, path: &Path) -> Result<Summary> {
        if expected != &self.identity {
            return Err("expected plan differs".into());
        }
        self.check_progress_path(path)?;
        let metadata = restore::ordinary(path)?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Err("invalid progress file or byte budget".into());
        }
        let mut raw = Vec::new();
        File::open(path)
            .map_err(|_| "cannot open progress")?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|_| "cannot read progress")?;
        if raw.len() as u64 > MAX_BYTES {
            return Err("progress byte budget exceeded".into());
        }
        let evidence = Object::from_bytes(&raw);
        let complete = raw
            .iter()
            .rposition(|b| *b == b'\n')
            .ok_or("progress header incomplete")?
            + 1;
        let incomplete_tail = complete < raw.len();
        if raw[complete..].len() > MAX_ROW {
            return Err("progress row budget exceeded".into());
        }
        let mut completed = 0;
        let mut finished = None;
        let mut rows = raw[..complete - 1].split(|b| *b == b'\n');
        let decode = |row: &[u8]| -> Result<Event> {
            if row.len() > MAX_ROW {
                return Err("progress row budget exceeded".into());
            }
            serde_json::from_value(parse_json(row).map_err(|_| "invalid progress JSON")?)
                .map_err(|_| "invalid progress event".into())
        };
        let first = decode(rows.next().ok_or("progress header required")?)?;
        let attempt = match &first {
            Event::Started { attempt, .. } => attempt.clone(),
            _ => return Err("progress header required".into()),
        };
        if serde_json::to_value(first).map_err(|_| "invalid progress header")?
            != serde_json::to_value(self.progress_header(attempt.clone()))
                .map_err(|_| "invalid progress header")?
        {
            return Err("progress source/profile/plan differs".into());
        }
        for row in rows {
            if finished.is_some() {
                return Err("progress follows terminal event".into());
            }
            match decode(row)? {
                Event::Completed {
                    ordinal,
                    record,
                    object,
                    action,
                    destination_verified,
                } => {
                    let item = self
                        .items
                        .get(completed)
                        .ok_or("progress entry budget exceeded")?;
                    if ordinal != completed
                        || record != item.record
                        || object != item.object
                        || !destination_verified
                        || !matches!(action.as_str(), "restored" | "already-present")
                    {
                        return Err("progress entry differs from plan".into());
                    }
                    completed += 1;
                }
                Event::Finished {
                    completed_entries,
                    success,
                } => {
                    if completed_entries != completed || (success && completed != self.items.len())
                    {
                        return Err("invalid progress completion".into());
                    }
                    finished = Some(success);
                }
                Event::Started { .. } => return Err("duplicate progress header".into()),
            }
        }
        if finished.is_some() && incomplete_tail {
            return Err("progress follows terminal event".into());
        }
        Ok(Summary {
            schema: "tig.project-progress-summary.experimental-1",
            plan: self.identity.clone(),
            attempt,
            evidence,
            entry_count: self.items.len(),
            logged_completed: completed,
            not_logged_completed: self.items.len() - completed,
            terminal_success: finished,
            incomplete_tail,
            destinations_reverified: false,
            source_guards_held: false,
            independent_custody_verified: false,
            authorizes_eviction: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_write_failure_does_not_acknowledge_or_modify_prior_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("evidence");
        std::fs::write(&path, b"retained prefix\n").unwrap();
        let mut writer = Writer {
            file: File::open(&path).unwrap(), // Deliberately read-only fault seam.
            bytes: 16,
            completed: 0,
        };
        assert!(
            writer
                .append(&Event::Finished {
                    completed_entries: 0,
                    success: false
                })
                .is_err()
        );
        assert_eq!(writer.bytes, 16);
        assert_eq!(writer.completed, 0);
        assert_eq!(std::fs::read(path).unwrap(), b"retained prefix\n");
    }
}

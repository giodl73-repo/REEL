//! Bounded observations of private-name candidates, never cleanup authority.
use super::{Plan, Result};
use crate::restore;
use serde::Serialize;
use std::{collections::BTreeSet, fs, path::Path};
use tig_core::{Digest, LogicalPath};

pub(super) fn default_directory_entries() -> usize {
    4096
}

#[derive(Serialize)]
pub struct Candidate {
    pub path: LogicalPath,
    pub classification: &'static str,
    pub observed_logical_bytes: Option<u64>,
}

#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub plan: Digest,
    pub source_commit: String,
    pub requested_directories: usize,
    pub missing_directories: usize,
    pub scanned_entries: usize,
    pub directory_entry_limit: usize,
    pub candidates: Vec<Candidate>,
    pub observed_partial_logical_bytes: u64,
    pub physical_reclaimable_bytes: Option<u64>,
    pub atomic_snapshot: bool,
    pub source_payloads_read: bool,
    pub destination_payloads_verified: bool,
    pub ownership_verified: bool,
    pub active_writers_known: bool,
    pub authorizes_cleanup: bool,
    pub authorizes_eviction: bool,
}

fn private_name(name: &str) -> bool {
    name.strip_prefix(".tig-restore-")
        .and_then(|s| s.strip_suffix(".partial"))
        .is_some_and(|nonce| {
            nonce.len() == 32
                && nonce
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

/// Inspect every ancestor before opening a directory; never follow link/reparse paths.
fn ordinary_directory(path: &Path) -> Result<bool> {
    let mut missing = false;
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => missing = true,
            Err(_) => return Err("partial inspection directory unavailable".into()),
            Ok(_) => {
                if !restore::ordinary(ancestor)?.is_dir() {
                    return Err("partial inspection requires ordinary directories".into());
                }
            }
        }
    }
    Ok(!missing)
}

impl Plan {
    /// Observe only immediate parents of planned destinations. No recursive discovery,
    /// payload verification, attempt ownership inference or mutation is performed.
    pub fn inspect_partials(&self, expected: &Digest) -> Result<Report> {
        if expected != &self.identity {
            return Err("expected plan differs".into());
        }
        let directories: BTreeSet<_> = self
            .items
            .iter()
            .map(|item| {
                item.destination
                    .as_str()
                    .rsplit_once('/')
                    .map(|(parent, _)| parent)
                    .ok_or_else(|| "planned destination parent unavailable".to_owned())
            })
            .collect::<Result<_>>()?;
        let mut report = Report {
            schema: "tig.project-partial-inspection.experimental-1",
            plan: self.identity.clone(),
            source_commit: self.source_commit.clone(),
            requested_directories: directories.len(),
            missing_directories: 0,
            scanned_entries: 0,
            directory_entry_limit: self.profile.limits.partial_directory_entries,
            candidates: Vec::new(),
            observed_partial_logical_bytes: 0,
            physical_reclaimable_bytes: None,
            atomic_snapshot: false,
            source_payloads_read: false,
            destination_payloads_verified: false,
            ownership_verified: false,
            active_writers_known: false,
            authorizes_cleanup: false,
            authorizes_eviction: false,
        };
        for relative in directories {
            let directory = self.profile.destination_root.join(relative);
            if !ordinary_directory(&directory)? {
                report.missing_directories += 1;
                continue;
            }
            for entry in
                fs::read_dir(&directory).map_err(|_| "partial inspection directory unavailable")?
            {
                report.scanned_entries += 1;
                if report.scanned_entries > report.directory_entry_limit {
                    return Err("partial inspection directory entry budget exceeded".into());
                }
                let entry = entry.map_err(|_| "partial inspection entry unavailable")?;
                let name = entry.file_name();
                let Some(name) = name.to_str().filter(|s| private_name(s)) else {
                    continue;
                };
                let path = LogicalPath::parse(&format!("{relative}/{name}"))
                    .map_err(|_| "invalid partial inspection path")?;
                // No payload open, including for active, linked or inaccessible entries.
                let (classification, bytes) = match restore::ordinary(&entry.path()) {
                    Ok(metadata) if metadata.is_file() => {
                        ("unassociated-private-partial", Some(metadata.len()))
                    }
                    Ok(_) => ("non-file-refused", None),
                    Err(_) => ("unverified-entry", None),
                };
                report.observed_partial_logical_bytes = report
                    .observed_partial_logical_bytes
                    .checked_add(bytes.unwrap_or(0))
                    .ok_or("partial inspection byte count overflow")?;
                report.candidates.push(Candidate {
                    path,
                    classification,
                    observed_logical_bytes: bytes,
                });
            }
        }
        report
            .candidates
            .sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        Ok(report)
    }
}

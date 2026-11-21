//! Small exact-base Git proposals. Never checkout, stage, select or upload.
use super::{Error, GitId, Repository, Result, Snapshot};
use std::{collections::BTreeMap, time::Instant};
use tig_core::{LogicalPath, Object};

const FILES: usize = 128;
const FILE_BYTES: usize = 65_536;
const TREES: usize = 128;

/// Existing authored path, expected raw bytes and proposed replacement.
/// Payload custody and native semantic validation remain caller obligations.
pub struct Replacement {
    pub path: LogicalPath,
    pub expected: Object,
    pub bytes: Vec<u8>,
}
/// Explicit Git identity, not evidence of human approval.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub name: String,
    pub email: String,
    pub unix_seconds: u64,
}
/// A read-back Git commit protected from ordinary GC by its proposal reference.
/// Not the active branch or a managed payload-retention acknowledgement.
pub struct Proposal {
    commit: GitId,
    reference: String,
}
/// Exact source/path/byte preflight, not a selected version or custody proof.
/// Borrows immutable caller bytes; only this module can construct the capability.
pub struct PreparedProposal<'a> {
    operation: Operation<'a>,
    base: GitId,
    replacements: &'a [Replacement],
    identity: &'a Identity,
    message: &'a str,
    snapshot: Snapshot,
}
impl PreparedProposal<'_> {
    pub fn base(&self) -> &GitId {
        &self.base
    }
    pub fn changes(&self) -> impl Iterator<Item = (&LogicalPath, &Object, Object)> {
        self.replacements
            .iter()
            .map(|r| (&r.path, &r.expected, Object::from_bytes(&r.bytes)))
    }
}
impl Proposal {
    pub fn commit(&self) -> &GitId {
        &self.commit
    }
    pub fn reference(&self) -> &str {
        &self.reference
    }
}
struct Operation<'a> {
    repository: &'a Repository,
    start: Instant,
    commands: usize,
}
impl Operation<'_> {
    fn check(&self) -> Result<()> {
        if self.start.elapsed().as_millis() > 30_000 || self.commands >= 1024 {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn run(&mut self, args: &[&str], bytes: Vec<u8>, cap: usize) -> Result<Vec<u8>> {
        self.check()?;
        self.commands += 1;
        // update-ref can invoke a repository's reference-transaction hook.
        // Proposal creation must not execute authored/configured hooks.
        let mut command = vec!["-c", "core.hooksPath=/dev/null"];
        command.extend_from_slice(args);
        let bytes = self.repository.run(&command, bytes, cap)?;
        self.check()?;
        Ok(bytes)
    }
    fn oid(&mut self, args: &[&str], bytes: Vec<u8>) -> Result<GitId> {
        let result = self.run(args, bytes, 65)?;
        let text = std::str::from_utf8(&result).map_err(|_| Error::Integrity)?;
        GitId::parse(text.strip_suffix('\n').ok_or(Error::Integrity)?)
    }
    fn rewrite(&mut self, tree: &GitId, changes: &BTreeMap<String, GitId>) -> Result<GitId> {
        let raw = self.run(
            &["ls-tree", "-z", tree.as_str()],
            Vec::new(),
            self.repository.limits.listing_bytes,
        )?;
        let mut rows = BTreeMap::new();
        for row in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let row = std::str::from_utf8(row).map_err(|_| Error::Integrity)?;
            let (meta, name) = row.split_once('\t').ok_or(Error::Integrity)?;
            let fields: Vec<_> = meta.split(' ').collect();
            if fields.len() != 3 || rows.contains_key(name) {
                return Err(Error::Integrity);
            }
            rows.insert(
                name.to_owned(),
                (
                    fields[0].to_owned(),
                    fields[1].to_owned(),
                    GitId::parse(fields[2])?,
                ),
            );
        }
        let mut children: BTreeMap<String, BTreeMap<String, GitId>> = BTreeMap::new();
        for (path, object) in changes {
            if let Some((first, rest)) = path.split_once('/') {
                children
                    .entry(first.into())
                    .or_default()
                    .insert(rest.into(), object.clone());
            } else {
                let row = rows.get_mut(path).ok_or(Error::Incomplete)?;
                if row.1 != "blob" || row.0 != "100644" {
                    return Err(Error::Invalid);
                }
                row.2 = object.clone();
            }
        }
        for (name, changes) in children {
            let row = rows.get_mut(&name).ok_or(Error::Incomplete)?;
            if row.1 != "tree" || row.0 != "040000" {
                return Err(Error::Invalid);
            }
            row.2 = self.rewrite(&row.2, &changes)?;
        }
        let mut bytes = Vec::new();
        for (name, (mode, kind, object)) in rows {
            bytes.extend_from_slice(
                format!("{mode} {kind} {}\t{name}\0", object.as_str()).as_bytes(),
            );
        }
        self.oid(&["mktree", "-z"], bytes)
    }
}
impl Repository {
    /// Write and read back a bounded proposal against an exact Git commit.
    /// This explicit operation writes Git objects and a create-only proposal ref;
    /// HEAD, branches, index, working files, cache and remote remain untouched.
    /// Ordinary Git cherry-pick/merge is the separate worktree application step.
    pub fn propose(
        &self,
        base: GitId,
        replacements: &[Replacement],
        identity: &Identity,
        message: &str,
    ) -> Result<Proposal> {
        self.check_proposal(base, replacements, identity, message)?
            .create()
    }
    /// Validate an exact-base request without writing Git objects/refs or files.
    /// Check and create share this preflight; a serialized report is not a token.
    pub fn check_proposal<'a>(
        &'a self,
        base: GitId,
        replacements: &'a [Replacement],
        identity: &'a Identity,
        message: &'a str,
    ) -> Result<PreparedProposal<'a>> {
        let token = |s: &str| {
            !s.trim().is_empty()
                && s.len() <= 256
                && !s.chars().any(|c| c.is_control() || c == '<' || c == '>')
        };
        if replacements.is_empty()
            || replacements.len() > FILES
            || !token(&identity.name)
            || !token(&identity.email)
            || !identity.email.is_ascii()
            || identity.email.chars().any(char::is_whitespace)
            || !identity.email.contains('@')
            || identity.unix_seconds > 253_402_300_799
            || message.trim().is_empty()
            || message.len() > 4096
            || message.contains('\0')
        {
            return Err(Error::Invalid);
        }
        let mut paths = BTreeMap::new();
        let mut ancestors = std::collections::BTreeSet::from([String::new()]);
        for replacement in replacements {
            let path = replacement.path.as_str();
            if replacement.bytes.len() > FILE_BYTES
                || replacement.expected.bytes > FILE_BYTES as u64
            {
                return Err(Error::Budget);
            }
            if paths.insert(path.to_owned(), replacement).is_some() {
                return Err(Error::Invalid);
            }
            for (offset, _) in path.match_indices('/') {
                ancestors.insert(path[..offset].to_owned());
            }
        }
        if ancestors.len() > TREES {
            return Err(Error::Budget);
        }
        let mut operation = Operation {
            repository: self,
            start: Instant::now(),
            commands: 0,
        };
        let snapshot = self.commit(base.clone())?;
        operation.check()?;
        let entries: BTreeMap<_, _> = snapshot
            .entries
            .iter()
            .map(|entry| (entry.path.as_str(), entry))
            .collect();
        // All source/path checks precede object writes. The base is immutable;
        // concurrent index/worktree edits are left for Git's application step.
        for (path, replacement) in &paths {
            let entry = entries.get(path.as_str()).ok_or(Error::Incomplete)?;
            if entry.mode != "100644" {
                return Err(Error::Invalid);
            }
            let bytes = operation.run(
                &["cat-file", "blob", entry.oid.as_str()],
                Vec::new(),
                FILE_BYTES,
            )?;
            entry.oid.verify_blob(&bytes)?;
            if Object::from_bytes(&bytes) != replacement.expected {
                return Err(Error::Integrity);
            }
        }
        Ok(PreparedProposal {
            operation,
            base,
            replacements,
            identity,
            message,
            snapshot,
        })
    }
}
impl PreparedProposal<'_> {
    /// Explicitly publish this checked request as a separate Git proposal.
    /// The bounded operation deadline remains in force; late callers recheck.
    pub fn create(self) -> Result<Proposal> {
        let Self {
            mut operation,
            base,
            replacements,
            identity,
            message,
            snapshot,
        } = self;
        operation.check()?;
        let mut changes = BTreeMap::new();
        for replacement in replacements {
            let oid =
                operation.oid(&["hash-object", "-w", "--stdin"], replacement.bytes.clone())?;
            oid.verify_blob(&replacement.bytes)?;
            changes.insert(replacement.path.as_str().to_owned(), oid);
        }
        let root = operation.oid(
            &["rev-parse", &format!("{}^{{tree}}", base.as_str())],
            Vec::new(),
        )?;
        let tree = operation.rewrite(&root, &changes)?;
        let person = format!(
            "{} <{}> {} +0000",
            identity.name, identity.email, identity.unix_seconds
        );
        let commit_bytes = format!(
            "tree {}\nparent {}\nauthor {person}\ncommitter {person}\n\n{}\n",
            tree.as_str(),
            base.as_str(),
            message.trim_end_matches('\n')
        )
        .into_bytes();
        let commit = operation.oid(
            &["hash-object", "-t", "commit", "-w", "--stdin"],
            commit_bytes.clone(),
        )?;
        if operation.run(&["cat-file", "commit", commit.as_str()], Vec::new(), 8192)?
            != commit_bytes
        {
            return Err(Error::Integrity);
        }
        let actual = operation.repository.commit(commit.clone())?;
        operation.check()?;
        if actual.entries.len() != snapshot.entries.len() {
            return Err(Error::Integrity);
        }
        for (old, new) in snapshot.entries.iter().zip(&actual.entries) {
            if old.path != new.path
                || old.mode != new.mode
                || new.oid != *changes.get(&old.path).unwrap_or(&old.oid)
            {
                return Err(Error::Integrity);
            }
        }
        let reference = format!("refs/tig/proposals/{}", commit.as_str());
        let expected = format!("{}\n", commit.as_str()).into_bytes();
        let existing = operation.run(
            &["show-ref", "--verify", "--hash", &reference],
            Vec::new(),
            65,
        );
        match existing {
            Ok(value) if value == expected => (),
            Ok(_) => return Err(Error::Integrity),
            Err(Error::Git) => {
                let zero = "0".repeat(commit.as_str().len());
                if operation
                    .run(
                        &[
                            "update-ref",
                            "--no-deref",
                            &reference,
                            commit.as_str(),
                            &zero,
                        ],
                        Vec::new(),
                        0,
                    )
                    .is_err()
                {
                    // A concurrent identical retry may have won create-only CAS.
                    if operation.run(
                        &["show-ref", "--verify", "--hash", &reference],
                        Vec::new(),
                        65,
                    )? != expected
                    {
                        return Err(Error::Integrity);
                    }
                }
            }
            Err(error) => return Err(error),
        }
        if operation.run(
            &["show-ref", "--verify", "--hash", &reference],
            Vec::new(),
            65,
        )? != expected
        {
            return Err(Error::Integrity);
        }
        match operation.run(&["symbolic-ref", "--quiet", &reference], Vec::new(), 4096) {
            Err(Error::Git) => (),
            Ok(_) => return Err(Error::Invalid),
            Err(error) => return Err(error),
        }
        Ok(Proposal { commit, reference })
    }
}

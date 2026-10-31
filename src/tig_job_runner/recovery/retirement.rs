//! Remove only quiescent owned scratch after protected retained-byte readback.
use super::*;
mod transaction;
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Spec {
    workspace: PathBuf,
    max_workspace_bytes: u64,
    #[serde(default)]
    pub(super) retained_result: Option<Selection>,
}

pub(super) struct Packet<'a> {
    pub job_id: &'a str,
    pub result: &'a Selection,
    pub journal: &'a Digest,
    pub request: &'a [u8],
    pub document: &'a Value,
    pub outputs: &'a serde_json::Map<String, Value>,
}

pub(super) struct Plan<'a> {
    profile: &'a Profile<'a>,
    workspace: PathBuf,
    cache: PathBuf,
    manifest: Object,
    max_bytes: u64,
    bytes: u64,
    verified: usize,
    verify_millis: u64,
    _installed_guards: Vec<File>,
    snapshot: Option<reel_assets::jobs::WorkspaceSnapshot>,
    transaction: transaction::Transaction,
}

fn no_reservation(profile: &Profile<'_>, job_id: &str) -> Result<()> {
    if profile.config.coordination_root.as_deref() != Some(profile.coordination.root.as_path()) {
        bail!("retirement requires the original explicit capacity coordination root");
    }
    // Only a short metadata observation, never a lock over hashing/removal.
    let _lock =
        reel_assets::admission::lock_for(&profile.coordination.root, Duration::from_secs(2))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    for extension in ["json", "lease"] {
        let path = profile
            .coordination
            .root
            .join("admissions")
            .join(format!("{job_id}.{extension}"));
        match fs::symlink_metadata(path) {
            Ok(_) => bail!("job reservation remains recorded; preserve unreconciled scratch"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn regular(path: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = m.file_type().is_symlink();
    if !m.is_file() || reparse {
        bail!("retirement requires a regular non-reparse file");
    }
    Ok(m)
}

fn check_file(path: &Path, pin: &Object, millis: u64) -> Result<File> {
    if regular(path)?.len() != pin.bytes {
        bail!("retirement file length differs");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let mut file = options.open(path)?;
    use sha2::{Digest as _, Sha256};
    let mut digest = Sha256::new();
    let mut buf = [0; 128 * 1024];
    let mut bytes = 0u64;
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_millis(millis) {
            bail!("retirement verification deadline exceeded");
        }
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .context("retirement byte overflow")?;
        if bytes > pin.bytes {
            bail!("retirement file grew while verifying");
        }
        digest.update(&buf[..read]);
    }
    let actual: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if bytes != pin.bytes || actual != pin.sha256.as_str() {
        bail!("retirement file hash differs");
    }
    Ok(file)
}

pub(super) fn prepare<'a>(
    profile: &'a Profile<'a>,
    spec: &Spec,
    packet: Packet<'_>,
    cache: &mut ReadCache,
    guards: &mut BTreeMap<Digest, VerifiedRead>,
    budget: &RecoveryBudget,
) -> Result<Plan<'a>> {
    if !cfg!(windows) {
        bail!("verified workspace retirement currently requires Windows mutation/deletion guards");
    }
    if !spec.workspace.is_absolute()
        || spec.max_workspace_bytes == 0
        || spec
            .workspace
            .file_name()
            .is_none_or(|name| name != packet.job_id)
    {
        bail!("exact absolute owned workspace and positive retirement bound required");
    }
    let parent = spec
        .workspace
        .parent()
        .context("workspace parent missing")?
        .canonicalize()?;
    let mut roots = profile.config.cache_roots.iter().filter_map(|root| {
        let canonical = root.canonicalize().ok()?;
        (parent == canonical.join("staging/caminos")).then_some(root.clone())
    });
    let root = roots
        .next()
        .context("workspace is outside enrolled staging")?;
    if roots.next().is_some() {
        bail!("ambiguous workspace cache enrollment");
    }
    let workspace = parent.join(packet.job_id);
    no_reservation(profile, packet.job_id)?;
    let scope = json!({"schema":"reel.tig-retirement-scope.experimental-1",
        "journal":packet.journal,"source":profile.identity.source.as_str(),"owner":profile.identity.owner.as_str(),
        "input_revision":profile.expected_revision,"operation":profile.operation.as_str(),
        "result":packet.result,"manifest":Object::from_bytes(packet.request),"workspace":workspace});
    let transaction = transaction::Transaction::read(&profile.coordination.root, scope)?;
    let mut installed = Vec::new();
    for role in ["inputs", "tools"] {
        for value in packet.document[role]
            .as_object()
            .context("request resource map missing")?
            .values()
        {
            let pin: Object = serde_json::from_value(value.clone())?;
            if let Some(guard) = guards.get(&pin.sha256) {
                if guard.object() != &pin {
                    bail!("conflicting retirement resource length");
                }
            } else {
                guards.insert(pin.sha256.clone(), cache.open_verified(&pin)?);
            }
        }
    }
    let mut installed_bytes = 0u64;
    if let Some(tools) = packet.document["installed_tools"].as_object() {
        for value in tools.values() {
            let path = PathBuf::from(
                value["path"]
                    .as_str()
                    .context("installed tool path missing")?,
            );
            let canonical = path.canonicalize()?;
            if canonical.starts_with(&workspace) {
                bail!("installed tool would be removed with scratch");
            }
            let pin: Object =
                serde_json::from_value(json!({"sha256":value["sha256"],"bytes":value["bytes"]}))?;
            installed_bytes = installed_bytes
                .checked_add(pin.bytes)
                .context("installed byte overflow")?;
            if cache
                .verification_bytes()
                .checked_add(installed_bytes)
                .is_none_or(|n| n > budget.max_verify_bytes)
                || guards.len() + installed.len() + 1 > budget.max_objects as usize
            {
                bail!("installed tool verification exceeds retirement budget");
            }
            installed.push(check_file(&path, &pin, budget.verify_millis)?);
        }
    }
    let manifest = Object::from_bytes(packet.request);
    let mut snapshot = None;
    let bytes = match fs::symlink_metadata(&workspace) {
        Ok(_) => {
            let (canonical, bytes) =
                jobs::inspect_owned_workspace(profile.config, &root, &workspace)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            if canonical != workspace || bytes > spec.max_workspace_bytes {
                bail!("workspace retirement exceeds admitted scope or size");
            }
            let observed = jobs::snapshot_owned_workspace(profile.config, &root, &workspace)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            transaction.check(&observed)?;
            check_manifest(
                &workspace,
                &manifest,
                budget.verify_millis,
                transaction.prepared(),
            )?;
            snapshot = Some(observed);
            for (name, output) in packet.document["outputs"]
                .as_object()
                .context("request outputs missing")?
            {
                let path = workspace.join(output["path"].as_str().context("output path missing")?);
                match fs::symlink_metadata(&path) {
                    Ok(_) => {
                        let retained = packet
                            .outputs
                            .get(name)
                            .context("workspace output has no retained candidate")?;
                        let pin: Object = serde_json::from_value(
                            json!({"sha256":retained["sha256"],"bytes":retained["bytes"]}),
                        )?;
                        drop(check_file(&path, &pin, budget.verify_millis)?);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
            }
            bytes
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => return Err(e.into()),
    };
    Ok(Plan {
        profile,
        workspace,
        cache: root,
        manifest,
        max_bytes: spec.max_workspace_bytes,
        bytes,
        verified: guards.len() + installed.len(),
        verify_millis: budget.verify_millis,
        _installed_guards: installed,
        snapshot,
        transaction,
    })
}

impl Plan<'_> {
    pub(super) fn finish(mut self, apply: bool) -> Result<Value> {
        let id = self
            .workspace
            .file_name()
            .context("workspace ID missing")?
            .to_str()
            .context("workspace ID invalid")?;
        no_reservation(self.profile, id)?;
        let mut already_absent = false;
        match fs::symlink_metadata(&self.workspace) {
            Ok(_) => {
                let (target, bytes) = jobs::inspect_owned_workspace(
                    self.profile.config,
                    &self.cache,
                    &self.workspace,
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?;
                if target != self.workspace || bytes != self.bytes || bytes > self.max_bytes {
                    bail!("workspace changed after retirement verification");
                }
                let observed =
                    jobs::snapshot_owned_workspace(self.profile.config, &self.cache, &target)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                if self.snapshot.as_ref() != Some(&observed) {
                    bail!("workspace entries changed after retirement verification");
                }
                self.transaction.check(&observed)?;
                check_manifest(
                    &target,
                    &self.manifest,
                    self.verify_millis,
                    self.transaction.prepared(),
                )?;
                if apply {
                    self.transaction.begin(observed, bytes)?;
                    // Exact canonical target, enrolled staging parent and all
                    // non-reparse descendants checked above. CAS guards are
                    // still held by the caller; no shared payload is removed.
                    fs::remove_dir_all(&target)?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => already_absent = true,
            Err(e) => return Err(e.into()),
        }
        if apply {
            self.transaction.complete()?;
        }
        Ok(
            json!({"schema":"reel.tig-workspace-retirement.experimental-1","job_id":id,
            "dry_run":!apply,"staging_retired":apply,"already_absent":already_absent,
            "workspace_logical_bytes_verified":self.bytes,"workspace_logical_bytes_removed":if apply && !already_absent {self.bytes}else{0},
            "would_remove_workspace":!apply && !already_absent,"physical_bytes_freed":null,
            "retained_objects_verified":self.verified,"cache_objects_removed":0,"production_claim":null,"approval":false}),
        ).map(|mut result| {
            result["retirement_transaction"] = self.transaction.report();
            result
        })
    }
}

fn check_manifest(workspace: &Path, manifest: &Object, millis: u64, prepared: bool) -> Result<()> {
    match fs::symlink_metadata(workspace.join("job.json")) {
        Ok(_) => {
            drop(check_file(&workspace.join("job.json"), manifest, millis)?);
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && prepared => Ok(()),
        Err(e) => Err(e.into()),
    }
}

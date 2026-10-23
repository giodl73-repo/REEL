//! Copy a registered, hash-bound checkpoint into a fresh admitted workspace.
//! The original recipe and source workspace are never edited or removed.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Checkpoint {
    schema: String,
    job_sha256: String,
    source_job_id: String,
    start_step: usize,
    files: BTreeMap<String, Asset>,
}

pub(super) struct Seed {
    pub checkpoint: Checkpoint,
    pub sha256: String,
    pub bytes: u64,
}

impl Seed {
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)?;
        if bytes.len() > 256 * 1024 {
            return Err("Job checkpoint descriptor exceeds intake budget".into());
        }
        Ok(Self {
            checkpoint: serde_json::from_slice(&bytes)?,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            bytes: bytes.len() as u64,
        })
    }

    pub fn validate(&self, job: &Job, job_bytes: &[u8]) -> Result<()> {
        let checkpoint = &self.checkpoint;
        if checkpoint.schema != "reel.native-job-resume.v1"
            || checkpoint.job_sha256 != format!("{:x}", Sha256::digest(job_bytes))
            || !checkpoint.source_job_id.starts_with("job-")
            || checkpoint.source_job_id.len() > 100
            || !checkpoint
                .source_job_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || checkpoint.start_step >= job.steps.len()
            || checkpoint.files.is_empty()
            || checkpoint.files.len() > 4096
        {
            return Err("Invalid checkpoint identity, coverage or resume step".into());
        }
        let reserved: BTreeSet<String> = job
            .inputs
            .keys()
            .chain(job.tools.keys())
            .chain(job.files.keys())
            .map(|name| name.to_ascii_lowercase())
            .chain(
                job.outputs
                    .values()
                    .map(|output| output.path.to_ascii_lowercase()),
            )
            .chain(["job.json".into(), "result.json".into()])
            .collect();
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        for (name, asset) in &checkpoint.files {
            relative(name)?;
            let name = name.to_ascii_lowercase();
            if !valid_oid(&asset.sha256)
                || name.starts_with("step-") && name.ends_with(".log")
                || reserved.iter().chain(names.iter()).any(|other| {
                    name == *other
                        || name.starts_with(&format!("{other}/"))
                        || other.starts_with(&format!("{name}/"))
                })
            {
                return Err(
                    "Checkpoint file aliases immutable inputs, outputs or job controls".into(),
                );
            }
            names.insert(name);
            add(&mut total, asset.bytes)?;
        }
        if total > job.scratch_bytes {
            return Err("Checkpoint files exceed the original scratch budget".into());
        }
        Ok(())
    }

    pub fn assets(&self) -> impl Iterator<Item = &Asset> {
        self.checkpoint.files.values()
    }

    pub fn pin_registered_sources(&self, cfg: &crate::Config, job_bytes: u64) -> Result<Vec<File>> {
        let identities = std::iter::once((self.checkpoint.job_sha256.as_str(), job_bytes))
            .chain(std::iter::once((self.sha256.as_str(), self.bytes)))
            .chain(
                self.assets()
                    .map(|asset| (asset.sha256.as_str(), asset.bytes)),
            );
        let mut guards = Vec::new();
        for (oid, size) in identities {
            let source = active_existing(cfg, oid, size)?
                .ok_or("Checkpoint authority absent from active caches; no archive access")?;
            let guard = lifecycle::pin_read_only(&source)?;
            verify(&source, oid, size)?;
            guards.push(guard);
        }
        Ok(guards)
    }

    pub fn stage(&self, cfg: &crate::Config, workspace: &Path) -> Result<()> {
        // Private copies: subsequent native steps may update checkpoint files.
        // Canonical checkpoint objects and the original staging remain immutable.
        for (name, asset) in &self.checkpoint.files {
            let source = active_existing(cfg, &asset.sha256, asset.bytes)?
                .ok_or("Checkpoint absent from active caches; no archive access")?;
            let _guard = lifecycle::pin_read_only(&source)?;
            verify(&source, &asset.sha256, asset.bytes)?;
            let target = workspace.join(relative(name)?);
            fs::create_dir_all(target.parent().ok_or("Missing checkpoint parent")?)?;
            let mut input = File::open(source)?.take(asset.bytes.saturating_add(1));
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            drop(output);
            verify(&target, &asset.sha256, asset.bytes)?;
        }
        Ok(())
    }

    pub fn start_step(&self) -> usize {
        self.checkpoint.start_step
    }

    pub fn evidence(&self) -> Value {
        json!({"schema":"reel.native-job-resume-evidence.v1",
            "original_manifest_sha256":self.checkpoint.job_sha256,
            "source_job_id":self.checkpoint.source_job_id,
            "checkpoint_descriptor_sha256":self.sha256,
            "checkpoint_files":self.checkpoint.files.len(),
            "first_executed_step":self.checkpoint.start_step,
            "original_manifest_modified":false,"source_workspace_modified":false,
            "copied_checkpoints_independently_verified":true,
            "native_build_checks_required":true,"human_approval":false})
    }
}

//! Explicit metadata-copy plans and checked application; no source-journal setup.
#![forbid(unsafe_code)]
use git_asset_core::{Digest, Object, identity};
use git_asset_store::metadata::{
    self, Checkpoint, Limits, MetadataReader, MetadataStore,
    copy::{self, Budget, Observation},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const MAX: usize = 65536;
const PROFILE: &str = "tig.metadata-provider-copy-profile.experimental-1";
const PLAN: &str = "tig.metadata-provider-copy-plan.experimental-1";
const REPORT: &str = "tig.metadata-provider-copy-report.experimental-1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    source_root: PathBuf,
    source_minimum: Checkpoint,
    target_root: PathBuf,
    target_limits: Limits,
    budget: Budget,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: String,
    profile: Object,
    source: Checkpoint,
    source_limits: Limits,
    source_directory: PathBuf,
    target_parent: PathBuf,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema: String,
    profile: Object,
    source_root: PathBuf,
    target_root: PathBuf,
    observation: Observation,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<(T, Object)> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > MAX as u64 {
        return Err("invalid bounded profile".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err("reparse profile refused".into());
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let file = options.open(path)?;
    let mut raw = Vec::new();
    file.take((MAX + 1) as u64).read_to_end(&mut raw)?;
    if raw.len() > MAX {
        return Err("profile exceeds bound".into());
    }
    Ok((
        serde_json::from_value(identity::parse_json(&raw)?)?,
        Object::from_bytes(&raw),
    ))
}
fn fresh(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err("destination already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
fn output_path(path: &Path, forbidden: &[&Path]) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    fresh(&path)?;
    let parent = fs::canonicalize(path.parent().ok_or("output parent missing")?)?;
    if path.file_name().is_none() {
        return Err("output name missing".into());
    }
    for root in forbidden {
        match fs::canonicalize(root) {
            Ok(root) if parent.starts_with(&root) => {
                return Err("output overlaps metadata provider".into());
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(parent.join(path.file_name().unwrap()))
}
fn inspect(profile: &Profile, pin: Object) -> Result<Plan> {
    if profile.schema != PROFILE
        || !profile.source_root.is_absolute()
        || !profile.target_root.is_absolute()
        || profile.target_root.file_name().is_none()
    {
        return Err("invalid metadata-copy profile".into());
    }
    profile.target_limits.validate()?;
    profile.budget.validate()?;
    fresh(&profile.target_root)?;
    let source_directory = fs::canonicalize(&profile.source_root)?;
    let target_parent = fs::canonicalize(
        profile
            .target_root
            .parent()
            .ok_or("target parent missing")?,
    )?;
    let target = target_parent.join(profile.target_root.file_name().unwrap());
    if target_parent.starts_with(&source_directory) || source_directory.starts_with(&target) {
        return Err("nested providers refused".into());
    }
    let source = MetadataReader::open(&profile.source_root, &profile.source_minimum)?;
    let current = source.checkpoint();
    if current.nodes() > profile.budget.nodes
        || current.bytes() > profile.budget.bytes
        || current.nodes() > profile.target_limits.nodes
        || current.bytes() > profile.target_limits.bytes
    {
        return Err(metadata::Error::Budget.into());
    }
    Ok(Plan {
        schema: PLAN.into(),
        profile: pin,
        source: current.clone(),
        source_limits: source.limits(),
        source_directory,
        target_parent,
    })
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn publish(value: &impl Serialize, path: &Path) -> Result<()> {
    let bytes = identity::canonical_json(&serde_json::to_value(value)?)?;
    if bytes.len() > MAX {
        return Err("report exceeds bound".into());
    }
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce)?;
    let parent = path.parent().ok_or("report parent missing")?;
    let temporary_path = parent.join(format!(
        ".git-asset-metadata-{}.tmp",
        Digest::blob(&nonce).as_str()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    let temporary = Temporary(temporary_path);
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::hard_link(&temporary.0, path)?;
    Ok(())
}
fn plan(profile_path: &Path, output: &Path) -> Result<()> {
    let (profile, pin) = read::<Profile>(profile_path)?;
    let output = output_path(output, &[&profile.source_root, &profile.target_root])?;
    let plan = inspect(&profile, pin)?;
    publish(&plan, &output)?;
    println!(
        "{}",
        json!({"schema":"tig.metadata-provider-copy-plan-summary.experimental-1","dry_run":true,
        "source_nodes":plan.source.nodes(),"source_bytes":plan.source.bytes(),"target_created":false,
        "nodes_verified":false,"physical_capacity_reserved_by_cli":false,"authorizes_eviction":false})
    );
    Ok(())
}
fn apply(profile_path: &Path, plan_path: &Path, output: &Path) -> Result<()> {
    let (profile, pin) = read::<Profile>(profile_path)?;
    let (planned, _) = read::<Plan>(plan_path)?;
    let output = output_path(output, &[&profile.source_root, &profile.target_root])?;
    let current = inspect(&profile, pin)?;
    if planned.schema != PLAN
        || identity::canonical_json(&serde_json::to_value(&planned)?)?
            != identity::canonical_json(&serde_json::to_value(&current)?)?
    {
        return Err(metadata::Error::Foreign.into());
    }
    let mut target = MetadataStore::create(&profile.target_root, profile.target_limits)?;
    let observation = copy::copy_to_empty(
        &profile.source_root,
        &planned.source,
        &profile.target_root,
        &mut target,
        profile.budget,
    )?;
    drop(target);
    let report = Report {
        schema: REPORT.into(),
        profile: current.profile,
        source_root: profile.source_root,
        target_root: profile.target_root,
        observation,
    };
    publish(&report, &output)?;
    println!(
        "{}",
        json!({"schema":"tig.metadata-provider-copy-summary.experimental-1","copy_complete":true,
        "nodes":report.observation.nodes,"bytes":report.observation.bytes,"fresh_target_readback_verified":true,
        "source_journal_accessed":false,"provider_selected":false,"physical_capacity_reserved_by_cli":false,
        "independent_custody_verified":false,"authorizes_eviction":false})
    );
    Ok(())
}
fn verify(report_path: &Path, output: &Path) -> Result<()> {
    let (report, _) = read::<Report>(report_path)?;
    if report.schema != REPORT {
        return Err("invalid copy report".into());
    }
    let output = output_path(output, &[&report.source_root, &report.target_root])?;
    let budget = Budget {
        nodes: report.observation.nodes.max(1),
        bytes: report.observation.bytes.max(1),
        millis: 600000,
    };
    copy::verify_target(&report.target_root, &report.observation, budget)?;
    let result = json!({"schema":"tig.metadata-provider-copy-verification.experimental-1",
        "target":report.observation.target,"nodes":report.observation.nodes,"bytes":report.observation.bytes,
        "object_set":report.observation.object_set,"target_matches_admitted_copy_observation":true,
        "current_source_revalidated":false,"provider_selected":false,"source_journal_accessed":false,
        "independent_custody_verified":false,"authorizes_eviction":false});
    publish(&result, &output)?;
    println!(
        "{}",
        json!({"schema":"tig.metadata-provider-copy-verification-summary.experimental-1",
        "target_matches_admitted_copy_observation":true,"nodes":report.observation.nodes,
        "provider_selected":false,"authorizes_eviction":false})
    );
    Ok(())
}
fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).take(5).collect();
    let result = match args.as_slice() {
        [action, profile, output] if action == "plan" => {
            plan(Path::new(profile), Path::new(output))
        }
        [action, profile, plan, output] if action == "apply" => {
            apply(Path::new(profile), Path::new(plan), Path::new(output))
        }
        [action, report, output] if action == "verify" => {
            verify(Path::new(report), Path::new(output))
        }
        _ => {
            eprintln!(
                "usage: git-asset-metadata plan PROFILE NEW_PLAN; apply PROFILE PLAN NEW_REPORT; verify REPORT NEW_VERIFICATION"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if matches!(
                error.downcast_ref::<metadata::Error>(),
                Some(metadata::Error::Budget)
            ) {
                eprintln!("metadata provider copy refused: bounded resource budget exceeded");
            } else {
                eprintln!(
                    "metadata provider copy refused; verify plan, provider witnesses and paths"
                );
            }
            ExitCode::from(1)
        }
    }
}

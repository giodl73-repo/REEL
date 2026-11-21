//! Existing-root-only owner settlement. Private paths never enter portable plans.
use super::*;
use git_asset_store::{
    metadata::{self, MetadataReader, MetadataStore},
    source_claims::{
        admission::Namespace,
        subset::{self, Enrollment, Plan, Request},
    },
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    enrollment: Enrollment,
    namespace_root: PathBuf,
    namespace_minimum: Checkpoint,
    metadata_root: PathBuf,
    metadata_minimum: metadata::Checkpoint,
    #[serde(default)]
    allow_settlement: bool,
}
fn read(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err("subset input exceeds budget".into());
    }
    Ok(bytes)
}
pub(super) fn run(
    mode: &str,
    profile: &Path,
    input: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !["plan", "status", "apply", "resume"].contains(&mode) {
        return Err("unsupported subset operation".into());
    }
    let profile: Profile = serde_json::from_value(parse_json(&read(profile)?)?)?;
    if profile.schema != "tig.subset-profile.design-1"
        || !profile.namespace_root.is_absolute()
        || !profile.metadata_root.is_absolute()
    {
        return Err("invalid subset profile".into());
    }
    profile.enrollment.id()?;
    if matches!(mode, "apply" | "resume") && !profile.allow_settlement {
        return Err("subset settlement disabled".into());
    }
    let mut ns = Namespace::open(&profile.namespace_root, &profile.namespace_minimum)?;
    let bytes = read(input)?;
    if mode == "plan" {
        let request: Request = serde_json::from_value(parse_json(&bytes)?)?;
        let reader = MetadataReader::open(&profile.metadata_root, &profile.metadata_minimum)?;
        let plan = subset::plan(&profile.enrollment, &mut ns, request, &reader)?;
        std::io::stdout().write_all(&plan.to_json()?)?;
    } else {
        let plan = Plan::from_json(&bytes)?;
        let status = if mode == "status" {
            subset::inspect(&profile.enrollment, &mut ns, &plan)?
        } else {
            let namespace_root = profile.namespace_root.canonicalize()?;
            let metadata_root = profile.metadata_root.canonicalize()?;
            if namespace_root.starts_with(&metadata_root)
                || metadata_root.starts_with(&namespace_root)
            {
                return Err("overlapping subset authority and metadata roots".into());
            }
            let mut metadata =
                MetadataStore::open(&profile.metadata_root, &profile.metadata_minimum)?;
            subset::apply(&profile.enrollment, &mut ns, &mut metadata, &plan)?
        };
        println!("{}", serde_json::to_string(&status)?);
    }
    Ok(())
}

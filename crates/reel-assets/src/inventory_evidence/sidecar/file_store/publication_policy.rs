//! Explicit, immutable enrollment requirement for the FileStore and native-job
//! producer families. This is not a cache-wide coverage or eviction certificate.
use super::*;
use collector::fence::{Binding, Domain};

const NAME: &str = "reel-managed-publication.v1.json";
const SCHEMA: &str = "reel.managed-publication-requirement.v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Requirement {
    schema: String,
    cache: PathBuf,
    publication_fence: Binding,
}

/// Pins the immutable requirement and its exact fence identity, without holding
/// any asset range lock. Private local control, never a removal permit.
pub(crate) struct Guard {
    _files: Vec<File>,
    _domain: Domain,
}

/// Install only while affected producers are quiescent. Never overwrites,
/// repairs, discovers consumers or migrates existing sessions. No CLI activates
/// this policy automatically. A partial install fails closed on subsequent use.
pub fn install(cache: &Path, domain: &Domain) -> Result<()> {
    let cache = checked_directory(cache)?;
    let requirement = Requirement {
        schema: SCHEMA.into(),
        publication_fence: domain.binding(&cache)?,
        cache: cache.clone(),
    };
    create_control(&cache.join(NAME), &requirement)
}

pub(crate) fn check(cache: &Path, requested: Option<&Domain>) -> Result<Option<Guard>> {
    let cache = checked_directory(cache)?;
    let path = cache.join(NAME);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(_) => (),
    }
    plain(&path, false)?;
    let mut files = crate::demotion::platform::guard_dirs(&path)?;
    let mut file = crate::demotion::platform::open(&path, false)?;
    if file.metadata()?.len() > CONTROL_BYTES {
        return Err("Managed publication requirement exceeds control budget".into());
    }
    let requirement: Requirement = decode(&read_limited(&mut file, CONTROL_BYTES)?)?;
    if requirement.schema != SCHEMA || requirement.cache != cache {
        return Err("Unsupported or misplaced managed publication requirement".into());
    }
    let domain = Domain::from_binding(&cache, &requirement.publication_fence)?;
    if !requested
        .ok_or("Cache requires persisted publication enrollment; legacy writer refused")?
        .same_domain(&domain)?
    {
        return Err("Writer publication domain differs from cache requirement".into());
    }
    files.push(file);
    Ok(Some(Guard {
        _files: files,
        _domain: domain,
    }))
}

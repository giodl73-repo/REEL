use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
use tig_core::{Digest, LogicalPath, identity::parse_json};
use tig_git::source::{GitId, Scope, protection_work::Binding};
use tig_store::{Key, metadata, source_claims::Checkpoint};
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub root: PathBuf,
    pub identity: Digest,
    pub minimum_checkpoint: Checkpoint,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub root: PathBuf,
    pub minimum_checkpoint: metadata::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub actor: Key,
    pub owner: Key,
    pub expected_revision: u64,
    pub commit: String,
    pub scope: Vec<LogicalPath>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub operation: Key,
    pub git_executable: PathBuf,
    pub repository: PathBuf,
    pub binding: Binding,
    pub commit: String,
    pub expected_revision: u64,
    #[serde(default)]
    pub companions: Vec<Projection>,
    pub index: Authority,
    pub journal: Authority,
    pub metadata: Metadata,
}
pub fn scope(paths: &[LogicalPath]) -> Result<(), String> {
    if paths.is_empty() || paths.len() > 32 {
        return Err("source scope budget".into());
    }
    Scope::new(paths.to_vec()).map_err(|_| "invalid source scope")?;
    Ok(())
}
pub fn bindings(
    git: &std::path::Path,
    repo: &std::path::Path,
    index: &Authority,
    journal: &Authority,
    metadata: &Metadata,
) -> Result<(), String> {
    if [git, repo, &index.root, &journal.root, &metadata.root]
        .iter()
        .any(|p| !p.is_absolute())
        || index.identity == journal.identity
    {
        return Err("absolute distinct source bindings required".into());
    }
    Ok(())
}
pub fn parse(bytes: &[u8]) -> Result<Profile, String> {
    // Leave room for native work references/checkpoint growth in the 64 KiB output.
    if bytes.len() > 61_440 {
        return Err("source stage input byte budget exceeded".into());
    }
    let p: Profile =
        serde_json::from_value(parse_json(bytes).map_err(|_| "invalid/over-budget source JSON")?)
            .map_err(|_| "invalid source profile fields")?;
    if p.schema != "tig.source-stage-profile.experimental-1" || p.companions.len() > 15 {
        return Err("unsupported source profile".into());
    }
    bindings(
        &p.git_executable,
        &p.repository,
        &p.index,
        &p.journal,
        &p.metadata,
    )?;
    scope(&p.binding.scope)?;
    GitId::parse(&p.commit).map_err(|_| "exact current commit required")?;
    let mut actors = BTreeSet::from([p.binding.actor.as_str().to_string()]);
    for c in &p.companions {
        if !actors.insert(c.actor.as_str().to_string()) {
            return Err("duplicate source actor".into());
        }
        scope(&c.scope)?;
        GitId::parse(&c.commit).map_err(|_| "exact companion commit required")?;
    }
    Ok(p)
}

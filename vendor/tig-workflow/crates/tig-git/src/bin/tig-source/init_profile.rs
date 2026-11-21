use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
use tig_core::{Digest, LogicalPath, identity::parse_json};
use tig_git::source::{GitId, protection_work::Binding};
use tig_store::{
    Key, metadata,
    source_claims::protection::{Actor, Enrollment},
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub commit: String,
    pub scope: Vec<LogicalPath>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    pub actor: Key,
    pub owner: Key,
    pub stores: Vec<Digest>,
    pub selection: Option<Selection>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub root: PathBuf,
    pub operation: Key,
    pub project: Key,
    pub source: Key,
    pub index_epoch: Key,
    pub journal_epoch: Key,
    pub controller: Key,
    pub subject: Digest,
    pub policies: Vec<Digest>,
    pub git_executable: PathBuf,
    pub repository: PathBuf,
    pub binding: Binding,
    pub commit: String,
    pub main_stores: Vec<Digest>,
    pub companions: Vec<Companion>,
    pub max_bindings: u64,
    pub metadata_limits: metadata::Limits,
}
impl Profile {
    pub fn enrollment(&self) -> Enrollment {
        let mut actors = vec![Actor {
            id: self.binding.actor.clone(),
            owner: self.binding.owner.clone(),
            stores: self.main_stores.clone(),
        }];
        actors.extend(self.companions.iter().map(|a| Actor {
            id: a.actor.clone(),
            owner: a.owner.clone(),
            stores: a.stores.clone(),
        }));
        actors.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        let stores = actors
            .iter()
            .flat_map(|a| a.stores.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Enrollment {
            owner: self.controller.clone(),
            subject: self.subject.clone(),
            policies: self.policies.clone(),
            stores,
            actors,
            max_bindings: self.max_bindings,
        }
    }
}
pub fn parse(bytes: &[u8]) -> Result<Profile, String> {
    if bytes.len() > 61_440 {
        return Err("source initialization input byte budget".into());
    }
    let p: Profile = serde_json::from_value(
        parse_json(bytes).map_err(|_| "invalid source initialization JSON")?,
    )
    .map_err(|_| "invalid source initialization fields")?;
    if p.schema != "tig.source-init-profile.experimental-1"
        || !p.root.is_absolute()
        || !p.git_executable.is_absolute()
        || !p.repository.is_absolute()
        || p.companions.len() > 15
    {
        return Err("unsupported source initialization profile".into());
    }
    p.enrollment()
        .validate()
        .map_err(|_| "invalid source enrollment")?;
    p.metadata_limits
        .validate()
        .map_err(|_| "invalid source metadata limits")?;
    super::profile::scope(&p.binding.scope)?;
    GitId::parse(&p.commit).map_err(|_| "exact initial commit required")?;
    for actor in &p.companions {
        if let Some(s) = &actor.selection {
            super::profile::scope(&s.scope)?;
            GitId::parse(&s.commit)
                .map_err(|_| "exact companion initialization commit required")?;
        }
    }
    Ok(p)
}

//! Explicit metadata-only wake/catch-up. Trigger hints never select a snapshot
//! or grant permission. This is not authentication of a hosted event/ref.
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use tig_core::identity;
use tig_store::{
    metadata::{Checkpoint, MetadataReader},
    retirement_intent::policy::{AuthorityBindings, GrantRef, NativeAuthority},
    source_claims::{
        SourceJournal, coordinated::accepted_head::hold_current, policy_registry::Registry,
        protection::Index,
    },
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    root: PathBuf,
    minimum_checkpoint: Checkpoint,
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    Push,
    CatchUp,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Trigger {
    kind: Kind,
    repository: u64,
    reference: Option<String>,
    delivery: Option<String>,
    commit_hint: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    repository: u64,
    reference: String,
    authority: AuthorityBindings,
    metadata: Metadata,
    trigger: Trigger,
}

pub fn observe(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > 65536 {
        return Err("wake input byte budget".into());
    }
    let p: Input =
        serde_json::from_value(identity::parse_json(bytes).map_err(|_| "invalid wake JSON")?)
            .map_err(|_| "invalid wake fields")?;
    // Exact enrollment matching, not a general Git ref parser. Event strings
    // never become paths, shell arguments, authority minima or executables.
    if p.schema != "tig.native-reconcile-wake.experimental-1"
        || p.repository == 0
        || p.reference.is_empty()
        || p.reference.len() > 256
        || p.reference.chars().any(char::is_control)
        || !p.metadata.root.is_absolute()
        || p.repository != p.trigger.repository
        || p.trigger
            .delivery
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
        || p.trigger
            .commit_hint
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
        || match p.trigger.kind {
            Kind::Push => p.trigger.reference.as_ref() != Some(&p.reference),
            Kind::CatchUp => p.trigger.reference.is_some() || p.trigger.commit_hint.is_some(),
        }
    {
        return Err("wake trigger scope refused".into());
    }
    NativeAuthority::new(p.authority.clone()).map_err(|_| "invalid wake authority bindings")?;
    let mut registry = Registry::open(&p.authority.registry_root, &p.authority.registry_minimum)
        .map_err(|_| "wake registry unavailable/rolled back")?;
    let mut index = Index::open(&p.authority.index_root, &p.authority.index_minimum)
        .map_err(|_| "wake index unavailable/rolled back")?;
    let mut source = SourceJournal::open(&p.authority.source_root, &p.authority.source_minimum)
        .map_err(|_| "wake source unavailable/rolled back")?;
    let nodes = MetadataReader::open(&p.metadata.root, &p.metadata.minimum_checkpoint)
        .map_err(|_| "wake metadata unavailable/rolled back")?;
    if registry.identity() != &p.authority.registry
        || index.identity() != &p.authority.index
        || source.identity() != &p.authority.source
    {
        return Err("wake authority identity mismatch".into());
    }
    let grant = registry
        .hold(&p.authority.slot, None, &nodes)
        .map_err(|_| "wake policy unavailable/pending/stopped")?;
    let head = hold_current(&mut index, &mut source)
        .map_err(|_| "wake accepted source unavailable/pending")?;
    let policy = grant.policy().config();
    if grant.owner() != &index.enrollment_profile().owner
        || policy.owner != index.enrollment_profile().subject
        || &policy.namespace != head.journal()
        || policy.source != head.frame().source
        || policy.authority != head.frame().authority
        || policy.epoch != head.frame().epoch
    {
        return Err("wake source/policy enrollment mismatch".into());
    }
    let selected = GrantRef {
        registry: registry.identity().clone(),
        slot: p.authority.slot.clone(),
        version: grant.version().clone(),
    };
    // Delivery/commit hints intentionally absent: duplicate, delayed and catch-up
    // wakes over the same current authority produce identical observations.
    let value = json!({"schema":"tig.native-reconcile-observation.experimental-1",
        "repository":p.repository,"reference":p.reference,"namespace":head.journal(),
        "frame":head.frame(),"grant":selected,"policy":grant.policy().fingerprint().map_err(|_|"invalid policy fingerprint")?,
        "provider_bindings":policy.provider_bindings,"selection":head.selection(),
        "minimums":{"registry":registry.checkpoint(),"index":index.checkpoint(),"source":source.checkpoint(),"metadata":nodes.checkpoint()},
        "execution_permission":"none","hosted_current_authority":"unverified"});
    identity::canonical_json(&value).map_err(|_| "wake output budget".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_and_oversized_inputs_refuse_without_authority_io() {
        assert!(observe(&vec![b' '; 65537]).is_err());
        assert!(observe(br#"{"schema":"x","schema":"y"}"#).is_err());
        assert!(observe(br#"{"root":"from-event","approval":{}}"#).is_err());
    }
}

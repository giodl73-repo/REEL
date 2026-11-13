//! Explicit reuse of a pinned song/poem source role, without rewriting old custody.
use crate::{
    Config, Result,
    versions::{Dependency, State},
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: String,
    pub entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    Song,
    Poem,
}

impl SourceKind {
    fn kind(&self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Poem => "poem",
        }
    }
    fn permits(&self, role: &str) -> bool {
        match self {
            Self::Song => matches!(role, "recording" | "listening"),
            Self::Poem => matches!(
                role,
                "reading/es" | "reading/en" | "spanish-corrected-body" | "english-translated-body"
            ),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub producer: Dependency,
    pub source_kind: SourceKind,
    pub role: String,
    pub sha256: String,
    pub consumers: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Grants {
    pub inputs: BTreeMap<String, BTreeSet<String>>,
    pub metadata_roots: BTreeSet<String>,
}

fn validate(
    state: &State,
    policy: Policy,
    mut package: impl FnMut(&str) -> Result<Value>,
) -> Result<Grants> {
    if policy.schema != "reel.source-output-reuse.v1" || policy.entries.len() > 64 {
        return Err("Invalid or oversized source-output reuse policy".into());
    }
    let mut grants = Grants::default();
    for e in policy.entries {
        if !crate::valid_oid(&e.sha256)
            || !e.source_kind.permits(&e.role)
            || e.consumers.is_empty()
            || e.consumers.len() > 64
            || e.consumers.iter().any(|x| x.is_empty() || x.len() > 256)
        {
            return Err("Invalid source role, identity or consumer scope".into());
        }
        let key = format!("{}@{}", e.producer.logical_id, e.producer.version_id);
        let producer = state
            .versions
            .get(&key)
            .ok_or("Source producer version missing")?;
        if producer.manifest_sha256 != e.producer.manifest_sha256
            || producer
                .dependency_contract
                .as_ref()
                .is_none_or(|c| !c.outputs.contains(&e.sha256))
        {
            return Err("Source producer lock/output mismatch".into());
        }
        let p = package(&e.producer.manifest_sha256)?;
        let asset = &p["assets"][&e.role];
        if p["schema"] != "caminos.artifact-package.v1"
            || p["artifact_id"] != e.producer.logical_id
            || p["version_id"] != e.producer.version_id
            || p["kind"] != e.source_kind.kind()
            || asset["sha256"] != e.sha256
            || asset["cache_uri"] != format!("cache://sha256/{}", e.sha256)
            || asset["bytes"].as_u64().is_none_or(|n| n == 0)
        {
            return Err("Pinned source package kind/role/asset mismatch".into());
        }
        grants.metadata_roots.insert(e.producer.manifest_sha256);
        for consumer in e.consumers {
            grants
                .inputs
                .entry(consumer)
                .or_default()
                .insert(e.sha256.clone());
        }
    }
    Ok(grants)
}

pub(crate) fn read(root: &Path, state: &State, storage: Option<&Config>) -> Result<Grants> {
    let path = root.join("registry/source-output-reuse.json");
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Grants::default()),
        Err(e) => return Err(e.into()),
    };
    if !meta.file_type().is_file() || meta.len() > 256 * 1024 {
        return Err("Source-output reuse control exceeds budget or is not a regular file".into());
    }
    let policy: Policy = serde_json::from_slice(&fs::read(path)?)?;
    let registered = crate::generic_objects_with(root, storage)?;
    if policy
        .entries
        .iter()
        .any(|e| !registered.contains_key(&e.sha256))
    {
        return Err("Reused source asset is not registered".into());
    }
    let cfg = storage
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| crate::config(root))?;
    validate(state, policy, |sha| {
        let bytes = *registered
            .get(sha)
            .ok_or("Source package is not registered")?;
        if bytes > 4 * 1024 * 1024 {
            return Err("Source package exceeds metadata budget".into());
        }
        let path = crate::active_existing(&cfg, sha, bytes)?
            .ok_or("Source package absent from active caches; archive not probed")?;
        let data = fs::read(path)?;
        if data.len() as u64 != bytes || format!("{:x}", Sha256::digest(&data)) != sha {
            return Err("Source package hash or byte count mismatch".into());
        }
        let value: Value = serde_json::from_slice(&data)?;
        // Role identity is validated above; its declared size must also be registered.
        for e in value["assets"]
            .as_object()
            .into_iter()
            .flat_map(|a| a.values())
        {
            if let (Some(h), Some(n)) = (e["sha256"].as_str(), e["bytes"].as_u64())
                && registered.get(h).is_some_and(|actual| *actual != n)
            {
                return Err("Source package contains conflicting registered sizes".into());
            }
        }
        Ok(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::{DependencyContract, Version};
    use serde_json::json;
    fn version(id: &str, input: bool) -> Version {
        let hash = "a".repeat(64);
        Version {
            logical_id: id.into(),
            version_id: "v1".into(),
            manifest_sha256: "b".repeat(64),
            payloads: [hash.clone()].into(),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
            dependency_contract: Some(DependencyContract {
                primary_inputs: if input {
                    [hash.clone()].into()
                } else {
                    BTreeSet::new()
                },
                outputs: if input {
                    BTreeSet::new()
                } else {
                    [hash].into()
                },
                references: BTreeSet::new(),
            }),
        }
    }
    fn fixture() -> (State, Value, Value) {
        let mut s = State::default();
        s.versions
            .insert("source@v1".into(), version("source", false));
        let p = json!({"schema":"caminos.artifact-package.v1","artifact_id":"source","kind":"song","version_id":"v1",
            "assets":{"listening":{"sha256":"a".repeat(64),"bytes":17,"cache_uri":format!("cache://sha256/{}","a".repeat(64))}}});
        let policy = json!({"schema":"reel.source-output-reuse.v1","entries":[{"producer":{"logical_id":"source","version_id":"v1","manifest_sha256":"b".repeat(64)},"source_kind":"song","role":"listening","sha256":"a".repeat(64),"consumers":["film"]}]});
        (s, p, policy)
    }
    #[test]
    fn strict_default_and_scoped_source_grant_preserve_immutable_versions() {
        let (mut s, p, policy) = fixture();
        let film = version("film", true);
        assert!(s.validate_dependency_contract(&film).is_err());
        let before = serde_json::to_vec(&s).unwrap();
        let grants = validate(&s, serde_json::from_value(policy).unwrap(), |_| {
            Ok(p.clone())
        })
        .unwrap();
        s.source_inputs = grants.inputs;
        s.validate_dependency_contract(&film).unwrap();
        assert_eq!(before, serde_json::to_vec(&s).unwrap());
        assert!(
            s.validate_dependency_contract(&version("unlisted", true))
                .is_err()
        );
        let mut dep = film.clone();
        dep.dependencies.push(Dependency {
            logical_id: "source".into(),
            version_id: "v1".into(),
            manifest_sha256: "b".repeat(64),
        });
        assert!(s.validate_dependency_contract(&dep).is_err());
        s.versions.insert("film@v1".into(), film);
        s.validate_dependency_contract(&version("source-alias", false))
            .unwrap();
    }
    #[test]
    fn rejects_movie_kind_wrong_role_hash_and_producer_lock() {
        let (s, p, policy) = fixture();
        for (field, value) in [
            ("kind", json!("music-video")),
            ("artifact_id", json!("different")),
            ("version_id", json!("v2")),
        ] {
            let mut bad = p.clone();
            bad[field] = value;
            assert!(
                validate(&s, serde_json::from_value(policy.clone()).unwrap(), |_| Ok(
                    bad.clone()
                ))
                .is_err()
            );
        }
        for (field, value) in [
            ("role", json!("delivery/movie")),
            ("sha256", json!("c".repeat(64))),
            ("consumers", json!([])),
            ("source_kind", json!("poem")),
        ] {
            let mut bad = policy.clone();
            bad["entries"][0][field] = value;
            assert!(validate(&s, serde_json::from_value(bad).unwrap(), |_| Ok(p.clone())).is_err());
        }
        let mut bad = policy;
        bad["entries"][0]["producer"]["manifest_sha256"] = json!("c".repeat(64));
        assert!(validate(&s, serde_json::from_value(bad).unwrap(), |_| Ok(p.clone())).is_err());
    }
    #[test]
    fn poem_reading_and_no_policy_are_bounded() {
        let (s, mut p, mut policy) = fixture();
        p["kind"] = json!("poem");
        p["assets"]["reading/es"] = p["assets"]["listening"].take();
        policy["entries"][0]["source_kind"] = json!("poem");
        policy["entries"][0]["role"] = json!("reading/es");
        validate(&s, serde_json::from_value(policy.clone()).unwrap(), |_| {
            Ok(p.clone())
        })
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path(), &s, None).unwrap().inputs.is_empty());
        policy["entries"] = json!(vec![policy["entries"][0].clone(); 65]);
        assert!(
            validate(&s, serde_json::from_value(policy).unwrap(), |_| Ok(
                p.clone()
            ))
            .is_err()
        );
    }
    #[test]
    fn cached_package_is_freshly_verified_and_never_recovered_from_archive() {
        let (mut state, p, mut policy) = fixture();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let cache = root.join("active");
        let archive = root.join("archive-must-not-be-created");
        let data = serde_json::to_vec(&p).unwrap();
        let sha = format!("{:x}", Sha256::digest(&data));
        state.versions.get_mut("source@v1").unwrap().manifest_sha256 = sha.clone();
        policy["entries"][0]["producer"]["manifest_sha256"] = json!(sha);
        fs::create_dir_all(root.join("registry")).unwrap();
        let path = crate::object(&cache, &sha);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &data).unwrap();
        fs::write(
            root.join("registry/source-output-reuse.json"),
            policy.to_string(),
        )
        .unwrap();
        fs::write(
            root.join("registry/objects.tsv"),
            format!("{}\t{}\n{}\t17\n", sha, data.len(), "a".repeat(64)),
        )
        .unwrap();
        let cfg = Config {
            path_policy: crate::PathPolicy::ConfiguredLocalRoots,
            cache_roots: vec![cache],
            archive_roots: vec![archive.clone()],
            ..Config::default()
        };
        let grants = read(root, &state, Some(&cfg)).unwrap();
        assert!(grants.inputs["film"].contains(&"a".repeat(64)));
        let mut corrupt = data.clone();
        corrupt[0] ^= 1;
        fs::write(&path, corrupt).unwrap();
        assert!(read(root, &state, Some(&cfg)).is_err());
        fs::remove_file(path).unwrap();
        assert!(read(root, &state, Some(&cfg)).is_err());
        assert!(!archive.exists());
        fs::write(
            root.join("registry/source-output-reuse.json"),
            vec![b' '; 256 * 1024 + 1],
        )
        .unwrap();
        assert!(read(root, &state, Some(&cfg)).is_err());
    }
}

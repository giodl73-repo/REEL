//! Bounded, read-only reference inspection. A graph is not a deletion permit or build certificate.
use crate::{Result, config, object, selected, tiering, valid_oid, versions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Component, Path, PathBuf},
};

mod authority_delivery;
mod native_alignment;
mod presentation_source;
mod presentation_template;
mod scene_context;
pub use scene_context::{CacheDocument, DocumentSource, SceneContext};

const PROFILE: &str = "reel.asset-inspection-project.v1";
const REPORT: &str = "reel.asset-reference-report.v1";

#[derive(Debug)]
struct Budget(&'static str);
impl std::fmt::Display for Budget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Budget {}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema: String,
    pub project_id: String,
    pub roots: Vec<String>,
    /// Explicit bindings for native episode, scene, policy, and template identifiers.
    #[serde(default)]
    pub documents: BTreeMap<String, DocumentSource>,
    /// Explicit native resolver inputs. These do not select or build a scene.
    #[serde(default)]
    pub scene_contexts: Vec<SceneContext>,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Deserialize, Serialize, JsonSchema, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub documents: usize,
    pub nodes: usize,
    pub edges: usize,
    pub depth: usize,
    pub document_bytes: u64,
    pub total_document_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            documents: 1000,
            nodes: 20000,
            edges: 100000,
            depth: 64,
            document_bytes: 16 * 1024 * 1024,
            total_document_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Deserialize, Serialize, JsonSchema, Clone)]
pub struct Asset {
    pub sha256: String,
    pub bytes: u64,
    pub cache_uri: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize, JsonSchema)]
struct Edition {
    version_id: String,
    manifest: Asset,
    version_lock: versions::Dependency,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize, JsonSchema)]
struct Record {
    schema: String,
    artifact_id: String,
    kind: String,
    current_version: String,
    versions: Vec<Edition>,
    #[serde(default)]
    current_input_inventory: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize, JsonSchema)]
struct Package {
    schema: String,
    artifact_id: String,
    kind: String,
    version_id: String,
    assets: BTreeMap<String, Asset>,
    #[serde(default)]
    dependencies: Vec<versions::Dependency>,
    #[serde(default)]
    episode_golden: Option<versions::Dependency>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize, JsonSchema)]
struct Part {
    record: String,
    version_id: String,
    manifest: Asset,
    version_lock: versions::Dependency,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize, JsonSchema)]
struct Inventory {
    schema: String,
    episode_id: String,
    episode_golden: versions::Dependency,
    verified_assets: Vec<Asset>,
    parts: Vec<Part>,
    #[serde(default)]
    unavailable_ancestral_source_references: Vec<Value>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize, JsonSchema)]
struct IndexRow {
    episode_id: String,
    record: String,
    golden: versions::Dependency,
    inventory: versions::Dependency,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
#[derive(Deserialize, Serialize, JsonSchema)]
struct InventoryIndex {
    schema: String,
    episodes: Vec<IndexRow>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Deserialize, Serialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub kind: String,
    pub schema: Option<String>,
    pub status: String,
    pub details: Value,
}
#[derive(Deserialize, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub field: String,
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub node: String,
    pub code: String,
    pub detail: String,
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub project_id: String,
    pub roots: Vec<String>,
    pub registry_generation: u64,
    pub nodes: BTreeMap<String, Node>,
    pub edges: BTreeSet<Edge>,
    pub findings: Vec<Finding>,
    pub scope: String,
    pub report_sha256: String,
}

fn parse(bytes: &[u8]) -> Result<Value> {
    let text = std::str::from_utf8(bytes)?.trim_start_matches('\u{feff}');
    Ok(serde_json::from_str(text)?)
}
fn guarded(path: &Path) -> Result<()> {
    for p in path.ancestors() {
        let m = fs::symlink_metadata(p)?;
        if m.file_type().is_symlink() {
            return Err("Reference path contains a symlink".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err("Reference path contains a reparse point".into());
            }
        }
    }
    Ok(())
}
fn relative(root: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.contains('\\')
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Invalid repository-relative reference".into());
    }
    let path = root.join(name);
    guarded(&path)?;
    crate::accounting::relative(root, name)
}
fn lock_id(lock: &versions::Dependency) -> String {
    format!(
        "version:{}@{}#{}",
        lock.logical_id, lock.version_id, lock.manifest_sha256
    )
}
fn state_digest(state: &versions::State) -> Result<String> {
    Ok(tiering::digest(&serde_json::to_vec(&(
        state,
        &state.metadata_roots,
    ))?))
}
fn report_digest(report: &Report) -> Result<String> {
    let mut value = serde_json::to_value(report)?;
    value["report_sha256"] = json!("");
    Ok(tiering::digest(&serde_json::to_vec(&value)?))
}

struct Pending {
    id: String,
    depth: usize,
    expected: Option<String>,
    identity: Option<(String, String)>,
    logical: Option<String>,
}
struct Inspector<'a> {
    root: &'a Path,
    project: Project,
    cfg: crate::Config,
    registered: std::collections::HashMap<String, u64>,
    state: versions::State,
    report: Report,
    queue: VecDeque<Pending>,
    seen: BTreeSet<String>,
    source_hashes: BTreeMap<PathBuf, String>,
    total: u64,
    locks: BTreeSet<String>,
    native_documents: BTreeMap<String, Value>,
}
impl Inspector<'_> {
    /// The only path for opt-in resolver-selected native JSON; media stays opaque.
    fn selected_native_metadata(
        &mut self,
        context: &str,
        key: &str,
        a: &reel_assembly::scene_authoring::AssetRef,
        kind: &str,
        schemas: &[&str],
    ) -> Result<Value> {
        let id = self.asset(
            context,
            &Asset {
                sha256: a.sha256.clone(),
                bytes: a.bytes,
                cache_uri: a.cache_uri.clone(),
                extra: BTreeMap::new(),
            },
            kind,
            key,
        )?;
        if self.registered.get(&a.sha256) != Some(&a.bytes) {
            return Err(
                "Selected native metadata is not authorized with its exact byte count".into(),
            );
        }
        if !self.native_documents.contains_key(&id) {
            if !self.seen.contains(&id) && self.seen.len() >= self.project.limits.documents {
                return Err(Budget("Document count budget exceeded").into());
            }
            self.seen.insert(id.clone());
            let bytes = self.read_document(&id)?;
            let value = parse(&bytes)?;
            let schema = value["schema"].as_str().unwrap_or("");
            if !schemas.contains(&schema) {
                return Err("Selected native metadata has the wrong schema".into());
            }
            // Match the renderer's byte-level native parser before a Value can
            // collapse duplicate struct fields (or hide an unsupported BOM).
            match schema {
                reel_assembly::scene_authoring::NATIVE_ALIGNMENT_SCHEMA => {
                    let _: reel_assembly::scene_authoring::NativeAlignment =
                        serde_json::from_slice(&bytes)?;
                }
                reel_assembly::template_presentation::SOURCE_TEXT_SCHEMA
                | reel_assembly::template_presentation::SCENE_SOURCE_TEXT_SCHEMA_V2 => {
                    let _: reel_assembly::template_presentation::PresentationSourceText =
                        serde_json::from_slice(&bytes)?;
                }
                reel_assembly::template_presentation::TEMPLATE_SCHEMA => {
                    let _: reel_assembly::template_presentation::EditableTextTemplate =
                        serde_json::from_slice(&bytes)?;
                }
                _ => return Err("Unsupported selected native metadata parser".into()),
            }
            let n = self.report.nodes.get_mut(&id).unwrap();
            n.kind = "asset-document".into();
            n.schema = Some(schema.into());
            n.status = "inspected-reference-projection".into();
            n.details["metadata_sha256"] = json!(tiering::digest(&bytes));
            n.details["verification"] = json!(
                "Selected native metadata freshly hash/size verified; original manuscript and media contents not read"
            );
            self.native_documents.insert(id.clone(), value);
        }
        let value = &self.native_documents[&id];
        if !schemas.contains(&value["schema"].as_str().unwrap_or("")) {
            return Err("Selected native metadata has the wrong schema".into());
        }
        Ok(value.clone())
    }

    fn finding(&mut self, node: &str, code: &str, detail: impl Into<String>) {
        self.report.findings.push(Finding {
            node: node.into(),
            code: code.into(),
            detail: detail.into(),
        });
    }
    fn node(&mut self, id: &str, kind: &str, status: &str, details: Value) -> Result<()> {
        self.report.nodes.entry(id.into()).or_insert(Node {
            kind: kind.into(),
            schema: None,
            status: status.into(),
            details,
        });
        if self.report.nodes.len() > self.project.limits.nodes {
            return Err(Budget("Reference node budget exceeded").into());
        }
        Ok(())
    }
    fn edge(&mut self, from: &str, to: &str, kind: &str, field: &str) -> Result<()> {
        self.report.edges.insert(Edge {
            from: from.into(),
            to: to.into(),
            kind: kind.into(),
            field: field.into(),
        });
        if self.report.edges.len() > self.project.limits.edges {
            return Err(Budget("Reference edge budget exceeded").into());
        }
        Ok(())
    }
    fn enqueue(
        &mut self,
        id: String,
        depth: usize,
        expected: Option<&str>,
        identity: Option<(String, String)>,
    ) -> Result<()> {
        if depth > self.project.limits.depth {
            return Err(Budget("Reference depth budget exceeded").into());
        }
        self.queue.push_back(Pending {
            id,
            depth,
            expected: expected.map(str::to_owned),
            identity,
            logical: None,
        });
        if self.queue.len() > self.project.limits.edges {
            return Err(Budget("Reference queue budget exceeded").into());
        }
        Ok(())
    }
    fn opaque(&mut self, id: &str, extra: &BTreeMap<String, Value>) {
        if !extra.is_empty() {
            self.finding(
                id,
                "uninspected-fields",
                extra.keys().cloned().collect::<Vec<_>>().join(", "),
            );
        }
    }
    fn asset(&mut self, from: &str, a: &Asset, kind: &str, field: &str) -> Result<String> {
        if !valid_oid(&a.sha256) || a.cache_uri != format!("cache://sha256/{}", a.sha256) {
            return Err("Invalid asset descriptor".into());
        }
        let id = a.cache_uri.clone();
        if let Some(n) = self.report.nodes.get(&id) {
            if n.details["bytes"].as_u64() != Some(a.bytes) {
                return Err("Conflicting reference byte counts".into());
            }
        } else {
            let mut status = "missing-active-copy";
            if self.registered.get(&a.sha256) != Some(&a.bytes) {
                status = "unregistered-or-size-conflict";
            } else {
                for cache in &self.cfg.cache_roots {
                    let p = object(cache, &a.sha256);
                    match fs::symlink_metadata(&p) {
                        Ok(m) => {
                            status = if !m.is_file() || guarded(&p).is_err() {
                                "unsafe-active-copy"
                            } else if m.len() != a.bytes {
                                "wrong-size-active-copy"
                            } else {
                                "present-size-only"
                            };
                            break;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => {
                            status = "unreadable-active-copy";
                            break;
                        }
                    }
                }
            }
            self.node(&id,"asset",status,json!({"sha256":a.sha256,"bytes":a.bytes,"verification":"media bytes not read; active tiers only"}))?;
            if status != "present-size-only" {
                self.finding(
                    &id,
                    status,
                    "No verified readable metadata target or size-consistent active media copy",
                );
            }
        }
        self.edge(from, &id, kind, field)?;
        Ok(id)
    }
    fn version(
        &mut self,
        from: &str,
        lock: &versions::Dependency,
        kind: &str,
        depth: usize,
    ) -> Result<()> {
        let id = lock_id(lock);
        if lock.logical_id.is_empty()
            || lock.version_id.is_empty()
            || !valid_oid(&lock.manifest_sha256)
        {
            return Err("Invalid version lock".into());
        }
        self.edge(from, &id, kind, "version_lock")?;
        if !self.locks.insert(id.clone()) {
            return Ok(());
        }
        if depth > self.project.limits.depth {
            return Err(Budget("Version depth budget exceeded").into());
        }
        let key = format!("{}@{}", lock.logical_id, lock.version_id);
        let Some(v) = self.state.versions.get(&key).cloned() else {
            self.node(
                &id,
                "version",
                "missing",
                json!({"logical_id":lock.logical_id,"version_id":lock.version_id}),
            )?;
            self.finding(&id, "missing-version", "Exact immutable version is absent");
            return Ok(());
        };
        if v.manifest_sha256 != lock.manifest_sha256 {
            self.node(&id, "version", "manifest-mismatch", json!({}))?;
            self.finding(
                &id,
                "version-manifest-mismatch",
                "Immutable registry disagrees with reference",
            );
            return Ok(());
        }
        self.node(&id,"version","registered",json!({"logical_id":v.logical_id,"version_id":v.version_id,"deprecated":v.deprecated,"current":self.state.current.get(&v.logical_id)==Some(&v.version_id)}))?;
        for hash in &v.payloads {
            if let Some(bytes) = self.registered.get(hash) {
                self.asset(
                    &id,
                    &Asset {
                        sha256: hash.clone(),
                        bytes: *bytes,
                        cache_uri: format!("cache://sha256/{hash}"),
                        extra: BTreeMap::new(),
                    },
                    "retains-asset",
                    "registry.payloads",
                )?;
            } else {
                self.finding(
                    &id,
                    "unregistered-payload",
                    format!("cache://sha256/{hash}"),
                );
            }
        }
        for dep in &v.dependencies {
            self.version(&id, dep, "requires-version", depth + 1)?;
        }
        Ok(())
    }
    fn document_ref(
        &mut self,
        from: &str,
        path: &str,
        kind: &str,
        field: &str,
        depth: usize,
        expected: Option<&str>,
    ) -> Result<()> {
        let id = format!("repo:{path}");
        self.edge(from, &id, kind, field)?;
        self.enqueue(id, depth + 1, expected, None)
    }
    fn named(
        &mut self,
        from: &str,
        name: &str,
        kind: &str,
        field: &str,
        depth: usize,
        expected: Option<&str>,
    ) -> Result<()> {
        if let Some(source) = self.project.documents.get(name).cloned() {
            self.declared_document(from, &source, kind, field, depth, expected)?;
            self.queue.back_mut().unwrap().logical = Some(name.into());
            Ok(())
        } else {
            let id = format!("logical:{name}");
            self.node(
                &id,
                "logical-reference",
                "unresolved",
                json!({"logical_id":name}),
            )?;
            self.edge(from, &id, kind, field)?;
            self.finding(
                &id,
                "unresolved-logical-reference",
                "Provide an explicit project.documents binding",
            );
            Ok(())
        }
    }
    fn read_document(&mut self, id: &str) -> Result<Vec<u8>> {
        let path = if let Some(name) = id.strip_prefix("repo:") {
            relative(self.root, name)?
        } else if let Some(hash) = id.strip_prefix("cache://sha256/") {
            let size = *self
                .registered
                .get(hash)
                .ok_or("Unregistered metadata object")?;
            if size > self.project.limits.document_bytes {
                return Err(Budget("Document exceeds byte budget").into());
            }
            let mut found = None;
            for cache in &self.cfg.cache_roots {
                let p = object(cache, hash);
                match fs::symlink_metadata(&p) {
                    Ok(_) => {
                        guarded(&p)?;
                        found = Some(p);
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            found.ok_or("Metadata object absent from active caches")?
        } else {
            return Err("Unknown document locator".into());
        };
        if fs::metadata(&path)?.len() > self.project.limits.document_bytes {
            return Err(Budget("Document exceeds byte budget").into());
        }
        let bytes = tiering::read_bounded(&path, self.project.limits.document_bytes)?;
        let hash = tiering::digest(&bytes);
        if let Some(expected) = id.strip_prefix("cache://sha256/")
            && (hash != expected || self.registered.get(expected) != Some(&(bytes.len() as u64)))
        {
            return Err("Metadata object hash/size mismatch".into());
        }
        self.total = self
            .total
            .checked_add(bytes.len() as u64)
            .ok_or("Metadata byte overflow")?;
        if self.total > self.project.limits.total_document_bytes {
            return Err(Budget("Total document byte budget exceeded").into());
        }
        self.source_hashes.insert(path, hash);
        if id.starts_with("repo:")
            && let Some((hash, size)) = crate::registry::parse_pointer(&bytes)?
        {
            if self.registered.get(&hash) != Some(&size) {
                return Err("LFS control document is unregistered or has a size conflict".into());
            }
            if size > self.project.limits.document_bytes {
                return Err(Budget("Document exceeds byte budget").into());
            }
            let cache_uri = format!("cache://sha256/{hash}");
            let a = Asset {
                sha256: hash,
                bytes: size,
                cache_uri,
                extra: BTreeMap::new(),
            };
            let target = self.asset(id, &a, "lfs-control-document", "Git LFS pointer")?;
            let body = self.read_document(&target)?;
            let node = self.report.nodes.get_mut(&target).unwrap();
            node.kind = "asset-document".into();
            node.status = "metadata-sha256-verified".into();
            node.details["verification"] = json!("fresh metadata SHA-256; no archive lookup");
            return Ok(body);
        }
        Ok(bytes)
    }
    fn process(&mut self, p: Pending) -> Result<()> {
        // Expectations are checked for every incoming edge, including duplicate targets.
        if self.seen.contains(&p.id) {
            let n = self.report.nodes[&p.id].clone();
            if let Some(name) = &p.logical
                && !n.details["declared_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.contains(&json!(name)))
            {
                self.finding(
                    &p.id,
                    "wrong-logical-identity",
                    format!("Document does not declare {name}"),
                );
            }
            if p.expected
                .as_ref()
                .is_some_and(|s| n.schema.as_ref() != Some(s))
            {
                self.finding(
                    &p.id,
                    "wrong-target-schema",
                    "Target differs from declared document type",
                );
            }
            if let Some((artifact, version)) = p.identity
                && (n.details["artifact_id"] != artifact || n.details["version_id"] != version)
            {
                self.finding(
                    &p.id,
                    "wrong-package-identity",
                    "Package differs from immutable record",
                );
            }
            return Ok(());
        }
        if self.seen.len() >= self.project.limits.documents {
            return Err(Budget("Document count budget exceeded").into());
        }
        self.seen.insert(p.id.clone());
        let bytes = match self.read_document(&p.id) {
            Ok(b) => b,
            Err(e) => {
                if e.is::<Budget>() {
                    return Err(e);
                }
                self.node(&p.id, "document", "unreadable", json!({}))?;
                self.report.nodes.get_mut(&p.id).unwrap().status = "unreadable".into();
                self.finding(&p.id, "unreadable-document", e.to_string());
                return Ok(());
            }
        };
        let value = match parse(&bytes) {
            Ok(v) => v,
            Err(e) => {
                self.node(&p.id, "document", "invalid-json", json!({}))?;
                self.report.nodes.get_mut(&p.id).unwrap().status = "invalid-json".into();
                self.finding(&p.id, "invalid-json", e.to_string());
                return Ok(());
            }
        };
        let schema = value["schema"].as_str().unwrap_or("").to_string();
        let declared_ids: Vec<Value> = match schema.as_str() {
            authority_delivery::AUTHORITY => vec![value["authority_id"].clone()],
            authority_delivery::DELIVERY => vec![value["delivery_id"].clone()],
            "reel.episode-authoring.v1" => vec![value["episode_id"].clone()],
            "reel.scene-authoring.v2" => vec![value["scene_id"].clone()],
            "reel.scene-policy.v1" => vec![value["policy_id"].clone()],
            "reel.scene-template-catalog.v1" => value["templates"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| v["template_id"].clone())
                .collect(),
            _ => vec![],
        };
        self.node(
            &p.id,
            "document",
            "inspected-reference-projection",
            json!({}),
        )?;
        {
            let n = self.report.nodes.get_mut(&p.id).unwrap();
            n.schema = Some(schema.clone());
            n.kind = if p.id.starts_with("cache:") {
                "asset-document"
            } else {
                "document"
            }
            .into();
            n.status = "inspected-reference-projection".into();
            n.details["declared_ids"] = json!(declared_ids);
            n.details["metadata_sha256"] = json!(tiering::digest(&bytes));
            n.details["verification"] = json!(if p.id.starts_with("cache:") {
                "metadata bytes freshly hash/size verified"
            } else {
                "repository metadata bytes freshly hashed"
            });
        }
        if let Some(name) = &p.logical
            && !declared_ids.contains(&json!(name))
        {
            self.finding(
                &p.id,
                "wrong-logical-identity",
                format!("Document does not declare {name}"),
            );
        }
        if p.expected.as_ref().is_some_and(|s| s != &schema) {
            self.finding(
                &p.id,
                "wrong-target-schema",
                "Target differs from declared document type",
            );
            return Ok(());
        }
        if let Some((artifact, version)) = &p.identity
            && (value["artifact_id"] != *artifact || value["version_id"] != *version)
        {
            self.finding(
                &p.id,
                "wrong-package-identity",
                "Package differs from immutable record",
            );
            return Ok(());
        }
        if matches!(
            schema.as_str(),
            "reel.episode-authoring.v1"
                | "reel.scene-authoring.v2"
                | "reel.scene-asset-bindings.v1"
                | "reel.scene-policy.v1"
                | "reel.scene-template-catalog.v1"
                | authority_delivery::AUTHORITY
        ) {
            self.native_documents.insert(p.id.clone(), value.clone());
        }
        let result = self.extract(&p, &schema, value);
        if let Err(e) = result {
            if e.is::<Budget>() {
                return Err(e);
            }
            self.finding(&p.id, "invalid-contract", e.to_string());
            self.report.nodes.get_mut(&p.id).unwrap().status = "invalid-contract".into();
        }
        Ok(())
    }
    fn extract(&mut self, p: &Pending, schema: &str, value: Value) -> Result<()> {
        let id = &p.id;
        match schema {
            authority_delivery::AUTHORITY => self.source_authority(id, value)?,
            authority_delivery::DELIVERY => self.delivery_record(id, value, p.depth)?,
            "caminos.artifact-record.v1" => {
                let r: Record = serde_json::from_value(value)?;
                if r.versions.is_empty()
                    || r.versions.len() > 128
                    || !r.versions.iter().any(|v| v.version_id == r.current_version)
                {
                    return Err("Invalid record version list/current pointer".into());
                }
                self.opaque(id, &r.extra);
                if let Some(path) = &r.current_input_inventory {
                    self.document_ref(
                        id,
                        path,
                        "inventory-snapshot",
                        "current_input_inventory",
                        p.depth,
                        Some("caminos.artifact-record.v1"),
                    )?;
                }
                if self.state.current.get(&r.artifact_id) != Some(&r.current_version) {
                    self.finding(
                        id,
                        "current-selection-disagreement",
                        "Record current pointer differs from native registry",
                    );
                }
                let mut ids = BTreeSet::new();
                for v in r.versions {
                    if !ids.insert(v.version_id.clone())
                        || v.version_id != v.version_lock.version_id
                        || r.artifact_id != v.version_lock.logical_id
                        || v.manifest.sha256 != v.version_lock.manifest_sha256
                    {
                        return Err("Record and version lock disagree".into());
                    }
                    self.opaque(id, &v.extra);
                    self.version(
                        id,
                        &v.version_lock,
                        if v.version_id == r.current_version {
                            "current-selection"
                        } else {
                            "historical-version"
                        },
                        p.depth + 1,
                    )?;
                    let target = self.asset(
                        &lock_id(&v.version_lock),
                        &v.manifest,
                        "manifest",
                        "manifest",
                    )?;
                    self.enqueue(
                        target,
                        p.depth + 1,
                        Some("caminos.artifact-package.v1"),
                        Some((r.artifact_id.clone(), v.version_id)),
                    )?;
                }
            }
            "caminos.artifact-package.v1" => {
                let a: Package = serde_json::from_value(value)?;
                self.report.nodes.get_mut(id).unwrap().details["artifact_id"] =
                    json!(a.artifact_id);
                self.report.nodes.get_mut(id).unwrap().details["version_id"] = json!(a.version_id);
                let hash = id
                    .strip_prefix("cache://sha256/")
                    .ok_or("Package must be addressed by immutable cache hash")?;
                let lock = versions::Dependency {
                    logical_id: a.artifact_id.clone(),
                    version_id: a.version_id.clone(),
                    manifest_sha256: hash.into(),
                };
                self.version(id, &lock, "describes-version", p.depth + 1)?;
                let registry = self
                    .state
                    .versions
                    .get(&format!("{}@{}", a.artifact_id, a.version_id));
                let declared: BTreeSet<_> = a.dependencies.iter().map(lock_id).collect();
                let actual: BTreeSet<_> = registry
                    .into_iter()
                    .flat_map(|v| v.dependencies.iter().map(lock_id))
                    .collect();
                // Some existing inventory packages omit dependencies; the registry stays authoritative.
                if !declared.is_empty() && declared != actual {
                    self.finding(
                        id,
                        "dependency-disagreement",
                        "Package dependency declarations differ from native registry",
                    );
                }
                for dep in &a.dependencies {
                    self.version(id, dep, "declared-dependency", p.depth + 1)?;
                }
                if let Some(golden) = &a.episode_golden {
                    self.version(id, golden, "describes-version", p.depth + 1)?;
                }
                self.opaque(id, &a.extra);
                let closure = if self
                    .state
                    .versions
                    .contains_key(&format!("{}@{}", a.artifact_id, a.version_id))
                {
                    Some(versions::payload_closure(&self.state, &lock)?)
                } else {
                    None
                };
                for (role, asset) in a.assets {
                    let membership = a.kind == "episode-input-part";
                    let target = self.asset(
                        id,
                        &asset,
                        if membership {
                            "inventory-member"
                        } else {
                            "declares-asset"
                        },
                        &format!("assets/{role}"),
                    )?;
                    if let Some(closure) = &closure
                        && !closure.contains(&asset.sha256)
                        && lock.manifest_sha256 != asset.sha256
                    {
                        self.finding(
                            &target,
                            "outside-version-closure",
                            "Package asset is not retained by its exact version closure",
                        );
                    }
                    if a.kind == "episode-input-inventory" && role == "inventory" {
                        self.enqueue(
                            target,
                            p.depth + 1,
                            Some("caminos.episode-input-inventory.v1"),
                            None,
                        )?;
                    }
                    // JSON media, reports, and arbitrary role names are intentionally opaque.
                }
            }
            "caminos.episode-input-inventory.v1" => {
                let i: Inventory = serde_json::from_value(value)?;
                self.version(id, &i.episode_golden, "describes-version", p.depth + 1)?;
                self.opaque(id, &i.extra);
                for a in i.verified_assets {
                    self.asset(id, &a, "inventory-member", "verified_assets")?;
                }
                if !i.unavailable_ancestral_source_references.is_empty() {
                    self.finding(
                        id,
                        "unavailable-ancestry",
                        format!(
                            "{} unresolved historical references; byte sizes remain unknown",
                            i.unavailable_ancestral_source_references.len()
                        ),
                    );
                }
                for part in i.parts {
                    if part.version_id != part.version_lock.version_id
                        || part.manifest.sha256 != part.version_lock.manifest_sha256
                    {
                        return Err("Inventory part lock disagrees".into());
                    }
                    self.version(id, &part.version_lock, "inventory-part", p.depth + 1)?;
                    self.document_ref(
                        id,
                        &part.record,
                        "record-link",
                        "parts/record",
                        p.depth,
                        Some("caminos.artifact-record.v1"),
                    )?;
                    let target =
                        self.asset(id, &part.manifest, "inventory-part", "parts/manifest")?;
                    self.enqueue(
                        target,
                        p.depth + 1,
                        Some("caminos.artifact-package.v1"),
                        Some((part.version_lock.logical_id, part.version_id)),
                    )?;
                    self.opaque(id, &part.extra);
                }
            }
            "caminos.episode-input-index.v1" => {
                let index: InventoryIndex = serde_json::from_value(value)?;
                self.opaque(id, &index.extra);
                for row in index.episodes {
                    self.document_ref(
                        id,
                        &row.record,
                        "record-link",
                        "episodes/record",
                        p.depth,
                        Some("caminos.artifact-record.v1"),
                    )?;
                    self.version(id, &row.golden, "describes-version", p.depth + 1)?;
                    self.version(id, &row.inventory, "inventory-snapshot", p.depth + 1)?;
                    self.opaque(id, &row.extra);
                }
            }
            "reel.episode-authoring.v1" => {
                let e: reel_assembly::scene_authoring::Episode = serde_json::from_value(value)?;
                self.report.nodes.get_mut(id).unwrap().details["episode_id"] = json!(e.episode_id);
                for scene in e.scene_ids {
                    self.named(
                        id,
                        &scene,
                        "requires-scene",
                        "scene_ids",
                        p.depth,
                        Some("reel.scene-authoring.v2"),
                    )?;
                }
                self.named(
                    id,
                    &e.master_template_id,
                    "requires-template",
                    "master_template_id",
                    p.depth,
                    Some("reel.scene-template-catalog.v1"),
                )?;
                self.named(
                    id,
                    &e.scene_policy_id,
                    "requires-policy",
                    "scene_policy_id",
                    p.depth,
                    Some("reel.scene-policy.v1"),
                )?;
                self.finding(id,"partial-native-extraction","Standalone episode extraction is partial; explicit scene-context nodes resolve one scene only. Whole-episode presentation, conform and delivery checks are not performed");
            }
            "reel.scene-authoring.v2" => {
                let s: reel_assembly::scene_authoring::Scene = serde_json::from_value(value)?;
                self.report.nodes.get_mut(id).unwrap().details["scene_id"] = json!(s.scene_id);
                self.named(
                    id,
                    &s.source_authority_id,
                    "source-authority",
                    "source_authority_id",
                    p.depth,
                    Some(authority_delivery::AUTHORITY),
                )?;
                if let Some(policy) = &s.scene_policy_override_id {
                    self.named(
                        id,
                        policy,
                        "requires-policy",
                        "scene_policy_override_id",
                        p.depth,
                        Some("reel.scene-policy.v1"),
                    )?;
                }
                if let Some(presentation) = &s.presentation {
                    self.named(
                        id,
                        &presentation.template_id,
                        "requires-template",
                        "presentation/template_id",
                        p.depth,
                        Some("reel.scene-template-catalog.v1"),
                    )?;
                }
                self.named(
                    id,
                    &s.episode_id,
                    "episode-context",
                    "episode_id",
                    p.depth,
                    Some("reel.episode-authoring.v1"),
                )?;
                self.finding(id,"partial-native-extraction","Standalone scene extraction is partial; explicit scene-context nodes report native symbolic resolution separately. Source evidence, rendered timing and approvals are not certified");
            }
            "reel.scene-asset-bindings.v1" => {
                let bindings: reel_assembly::scene_authoring::ScopedBindings =
                    serde_json::from_value(value)?;
                for (name, a) in bindings.assets {
                    let a: Asset = serde_json::from_value(serde_json::to_value(a)?)?;
                    self.asset(id, &a, "bound-asset", &format!("assets/{name}"))?;
                }
                self.finding(id,"reference-projection-only","Standalone descriptor check only; explicit scene-context nodes report native resolution separately. Creative approval is not certified");
            }
            "reel.scene-policy.v1" => {
                let _: reel_assembly::scene_authoring::ScenePolicy = serde_json::from_value(value)?;
            }
            "reel.scene-template-catalog.v1" => {
                let _: reel_assembly::scene_authoring::TemplateCatalog =
                    serde_json::from_value(value)?;
                self.finding(id,"partial-native-extraction","Template structure parsed; template identifiers and content semantics require authoring validation");
            }
            _ => {
                self.report.nodes.get_mut(id).unwrap().status = "unsupported-schema".into();
                self.finding(
                    id,
                    "unsupported-schema",
                    format!("No registered reference adapter for {schema:?}"),
                );
            }
        }
        Ok(())
    }
}

/// No network, archive crawl, media hydration, cache mutation, or long-lived lock.
pub fn inspect(root: &Path, profile: &str) -> Result<Report> {
    inspect_with(root, profile, || {})
}
fn inspect_with(root: &Path, profile: &str, before_recheck: impl FnOnce()) -> Result<Report> {
    let root = root.canonicalize()?;
    let profile_path = relative(&root, profile)?;
    let profile_bytes = tiering::read_bounded(&profile_path, 256 * 1024)?;
    let project: Project = serde_json::from_value(parse(&profile_bytes)?)?;
    let l = &project.limits;
    if project.schema != PROFILE
        || project.project_id.is_empty()
        || project.roots.is_empty()
        || project.roots.len() > 1000
        || project.scene_contexts.len() > 64
        || l.documents == 0
        || l.documents > 10000
        || l.nodes == 0
        || l.nodes > 100000
        || l.edges == 0
        || l.edges > 500000
        || l.depth == 0
        || l.depth > 128
        || l.document_bytes == 0
        || l.document_bytes > 64 * 1024 * 1024
        || l.total_document_bytes == 0
        || l.total_document_bytes > 256 * 1024 * 1024
    {
        return Err("Invalid inspector profile or limits".into());
    }
    // Existing storage/project controls are part of the snapshot, not a new cache configuration.
    let mut controls = BTreeMap::new();
    let absent_controls: Vec<_> = [
        root.join("asset-project.json"),
        root.join(".local/storage-rust.json"),
    ]
    .into_iter()
    .filter(|p| !p.exists())
    .collect();
    for path in [
        root.join("asset-project.json"),
        root.join(".local/storage-rust.json"),
    ] {
        if path.exists() {
            let b = tiering::read_bounded(&path, 256 * 1024)?;
            controls.insert(path, tiering::digest(&b));
        }
    }
    if root.join("asset-project.json").exists() {
        let p = crate::accounting::project(&root)?;
        let path = relative(&root, &p.storage_config)?;
        controls.insert(
            path.clone(),
            tiering::digest(&tiering::read_bounded(&path, 256 * 1024)?),
        );
    }
    let cfg = config(&root)?;
    let state = versions::read(&root)?;
    let before = state_digest(&state)?;
    let registered = selected(&root)?;
    let mut roots = project
        .roots
        .iter()
        .map(|p| format!("repo:{p}"))
        .collect::<Vec<_>>();
    let mut context_ids = BTreeSet::new();
    for context in &project.scene_contexts {
        if context.id.trim().is_empty()
            || context.id.len() > 256
            || !context_ids.insert(context.id.clone())
        {
            return Err("Scene context IDs must be nonempty, bounded and unique".into());
        }
        roots.push(format!("scene-context:{}", context.id));
    }
    let report=Report {schema:REPORT.into(),project_id:project.project_id.clone(),roots:roots.clone(),registry_generation:state.generation,nodes:BTreeMap::new(),edges:BTreeSet::new(),findings:vec![],scope:"Explicit root closure only. Inventory members and evidence are not build dependencies. Media presence is size-only; metadata bytes are freshly hashed. No creative approval, native build certification, orphan classification or removal authorization.".into(),report_sha256:String::new()};
    controls.insert(profile_path, tiering::digest(&profile_bytes));
    let mut i = Inspector {
        root: &root,
        project,
        cfg,
        registered,
        state,
        report,
        queue: VecDeque::new(),
        seen: BTreeSet::new(),
        source_hashes: controls,
        total: 0,
        locks: BTreeSet::new(),
        native_documents: BTreeMap::new(),
    };
    for id in roots.into_iter().filter(|id| id.starts_with("repo:")) {
        i.enqueue(id, 0, None, None)?;
    }
    i.prepare_scene_contexts()?;
    while let Some(p) = i.queue.pop_front() {
        i.process(p)?;
    }
    i.resolve_scene_contexts()?;
    i.check_authority_scopes();
    // DFS only required relationships; contextual episode/scene cycles are legitimate.
    let mut visiting = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut required: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for edge in i
        .report
        .edges
        .iter()
        .filter(|e| e.kind == "requires-version")
    {
        required
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to.clone());
    }
    fn visit(
        id: &str,
        edges: &BTreeMap<String, Vec<String>>,
        visiting: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
        depth: usize,
    ) -> bool {
        if done.contains(id) {
            return false;
        }
        if depth > 128 || !visiting.insert(id.into()) {
            return true;
        }
        for target in edges.get(id).into_iter().flatten() {
            if visit(target, edges, visiting, done, depth + 1) {
                return true;
            }
        }
        visiting.remove(id);
        done.insert(id.into());
        false
    }
    let cyclic = i
        .report
        .nodes
        .keys()
        .any(|id| visit(id, &required, &mut visiting, &mut done, 0));
    if cyclic {
        i.finding(
            "registry",
            "required-dependency-cycle",
            "Exact version dependencies contain a cycle or exceed the supported depth",
        );
    }
    before_recheck();
    if before != state_digest(&versions::read(&root)?)? || i.registered != selected(&root)? {
        return Err("Registry changed during reference inspection; retry".into());
    }
    if absent_controls.iter().any(|p| p.exists()) {
        return Err("Project/storage configuration appeared during inspection; retry".into());
    }
    for (path, hash) in &i.source_hashes {
        guarded(path)?;
        if tiering::digest(&tiering::read_bounded(
            path,
            i.project.limits.document_bytes.max(256 * 1024),
        )?) != *hash
        {
            return Err(
                "Source document or storage controls changed during inspection; retry".into(),
            );
        }
    }
    i.report.report_sha256 = report_digest(&i.report)?;
    Ok(i.report)
}

pub fn query(report: &Report, id: &str) -> Result<Value> {
    if report.schema != REPORT || report_digest(report)? != report.report_sha256 {
        return Err("Reference report digest/schema mismatch".into());
    }
    let node = report.nodes.get(id).ok_or("Unknown graph node")?;
    let incoming: Vec<_> = report.edges.iter().filter(|e| e.to == id).collect();
    let outgoing: Vec<_> = report.edges.iter().filter(|e| e.from == id).collect();
    Ok(
        json!({"schema":"reel.asset-reference-query.v1","node_id":id,"node":node,"incoming":incoming,"outgoing":outgoing,"findings":report.findings.iter().filter(|f|f.node==id).collect::<Vec<_>>(),"scope":report.scope,"impact":"Direct typed relationships only; incoming inventory/evidence links do not imply a rebuild or retention authority"}),
    )
}

/// Standalone private browser view. No server, network access, or mutation controls.
pub fn review(report: &Report, output: &Path) -> Result<Value> {
    use std::io::Write;
    if report.schema != REPORT || report_digest(report)? != report.report_sha256 {
        return Err("Reference report digest/schema mismatch".into());
    }
    let data = serde_json::to_string(report)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let html = include_str!("reference_review.html")
        .replacen("__SCRIPT__", include_str!("reference_review.js"), 1)
        .replacen("__DATA__", &data, 1);
    if html.len() > 32 * 1024 * 1024 {
        return Err("Reference review exceeds 32 MiB budget".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = crate::Temporary(crate::unique(parent, "reference-review"));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp.0)?;
    file.write_all(html.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::hard_link(&tmp.0, output)?;
    Ok(
        json!({"schema":"reel.asset-reference-review.v1","private_html":output,"report_sha256":report.report_sha256,"network_used":false,"removal_authorized":false}),
    )
}

pub fn catalogue() -> Result<Value> {
    Ok(
        json!({"schema":"reel.asset-schema-catalogue.v1","json_schema_dialect":"https://json-schema.org/draft/2020-12/schema","contracts":[
        {"schema":PROFILE,"coverage":"complete profile contract","json_schema":schemars::schema_for!(Project)},
        {"schema":REPORT,"coverage":"complete report contract","json_schema":schemars::schema_for!(Report)},
        {"schema":"reel.asset-project.v1","coverage":"strict profile structure; runtime checks selections, coverage and primary-input graph","json_schema":schemars::schema_for!(crate::accounting::Project)},
        {"schema":authority_delivery::AUTHORITY,"coverage":"strict source revision/scope and evidence references; no approval or source-content verification","json_schema":schemars::schema_for!(authority_delivery::SourceAuthority)},
        {"schema":authority_delivery::DELIVERY,"coverage":"strict exact-version output/evidence references; no remote readback or release authorization","json_schema":schemars::schema_for!(authority_delivery::DeliveryRecord)},
        {"schema":"caminos.artifact-record.v1","coverage":"reference projection; extra fields opaque","json_schema":schemars::schema_for!(Record)},
        {"schema":"caminos.artifact-package.v1","coverage":"reference projection; extra fields opaque","json_schema":schemars::schema_for!(Package)},
        {"schema":"caminos.episode-input-inventory.v1","coverage":"membership projection, not build closure","json_schema":schemars::schema_for!(Inventory)},
        {"schema":"caminos.episode-input-index.v1","coverage":"reference projection; extra fields opaque","json_schema":schemars::schema_for!(InventoryIndex)},
        {"schema":"reel.editable-text-template.v1","coverage":"native Rust parser; explicit scene-context catalog hash, content and presentation clock checks; no standalone traversal or rendered acceptance","json_schema":Value::Null},
        {"schema":"reel.scene-native-alignment.v1","coverage":"native Rust parser; opt-in selected alignment identity/clock bounds and semantic-event spans; no audio measurement or standalone traversal","json_schema":Value::Null},
        {"schema":"reel.scene-asset-bindings.v1","coverage":"native Rust structure; explicit scene contexts resolve selected bindings","json_schema":Value::Null},
        {"schema":"reel.episode-authoring.v1","coverage":"native Rust structure; scene, template and policy references extracted; other semantics uninspected","json_schema":Value::Null},
        {"schema":"reel.scene-authoring.v2","coverage":"native Rust structure; episode, policy and presentation-template references extracted; explicit scene contexts resolve selected bindings","json_schema":Value::Null},
        {"schema":"reel.scene-policy.v1","coverage":"native Rust structure only","json_schema":Value::Null},
        {"schema":"reel.scene-template-catalog.v1","coverage":"native Rust structure only","json_schema":Value::Null}
    ],"registry_structures":{"coverage":"Strict serialized structures; partition, cross-version roles and dependency graph are checked at runtime","immutable_version":schemars::schema_for!(versions::Version),"dependency_contract":schemars::schema_for!(versions::DependencyContract)},"edge_semantics":{"lfs-control-document":"Standard Git LFS control pointer to registered metadata; fresh active-tier SHA-256 verification, no archive lookup","selected-presentation-template":"Exact catalog-selected editable template metadata; in-memory compile only, not visual acceptance","selected-native-alignment":"Exact selected native alignment metadata; declared clocks only, not measured audio timing","aligned-take":"Alignment-to-selected-take identity; media size-only","requires-asset":"Exact selected input returned by the native scene resolver; media presence is size-only","authoring-document":"Explicit metadata input to a native scene context","requires-version":"Exact registry dependency; rebuild/retention candidate relationship","inventory-member":"Observed bytes; no build-dependency inference","retains-asset":"Immutable version custody, not creative selection","describes-version":"Contextual scope, not dependency","declares-asset":"Package declaration; role-specific semantics not inferred","current-selection":"Record current pointer; native registry current state is reported separately","historical-version":"Previous record version","episode-context":"Contextual backlink, not a dependency cycle"},"limitations":"Not a complete catalogue of all REEL schemas. Unsupported schemas and uninspected metadata are explicit findings. No arbitrary JSON/hash search."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiering::tests::Fixture;
    pub(super) fn descriptor(hash: &str, bytes: u64) -> Value {
        json!({"sha256":hash,"bytes":bytes,"cache_uri":format!("cache://sha256/{hash}")})
    }
    pub(super) fn write(f: &Fixture, path: &str, value: &Value) {
        fs::write(f.root().join(path), serde_json::to_vec(value).unwrap()).unwrap();
    }
    pub(super) fn profile(f: &Fixture, roots: Value) {
        write(
            f,
            "inspect.json",
            &json!({"schema":PROFILE,"project_id":"synthetic","roots":roots}),
        );
    }
    pub(super) fn fixture() -> (Fixture, String, String) {
        let mut f = Fixture::new();
        let media = f.put(b"synthetic media", true);
        let old = f.put(b"previous golden", true);
        let old_manifest=serde_json::to_vec(&json!({"schema":"caminos.artifact-package.v1","artifact_id":"episode/demo","kind":"episode","version_id":"v0","assets":{"delivery":descriptor(&old,15)}})).unwrap();
        let old_hash = f.put(&old_manifest, true);
        f.state
            .register(versions::Version {
                logical_id: "episode/demo".into(),
                version_id: "v0".into(),
                manifest_sha256: old_hash.clone(),
                payloads: BTreeSet::from([old]),
                dependencies: vec![],
                supersedes: None,
                deprecated: true,
                dependency_contract: None,
            })
            .unwrap();
        let package=serde_json::to_vec(&json!({"schema":"caminos.artifact-package.v1","artifact_id":"episode/demo","kind":"episode","version_id":"v1","assets":{"delivery":descriptor(&media,15)}})).unwrap();
        let hash = f.put(&package, true);
        let golden = f.version("episode/demo", &hash);
        f.state
            .versions
            .get_mut("episode/demo@v1")
            .unwrap()
            .payloads
            .insert(media.clone());
        f.state.select("episode/demo", "v1").unwrap();
        let inventory=serde_json::to_vec(&json!({"schema":"caminos.episode-input-inventory.v1","episode_id":"demo","episode_golden":golden,"verified_assets":[descriptor(&media,15)],"parts":[],"scope":"source superset"})).unwrap();
        let inv = f.put(&inventory, true);
        let ip=serde_json::to_vec(&json!({"schema":"caminos.artifact-package.v1","artifact_id":"inputs/demo","kind":"episode-input-inventory","version_id":"v1","dependencies":[golden],"assets":{"inventory":descriptor(&inv,inventory.len() as u64)}})).unwrap();
        let iph = f.put(&ip, true);
        let ilock = f.version("inputs/demo", &iph);
        let v = f.state.versions.get_mut("inputs/demo@v1").unwrap();
        v.payloads.insert(inv);
        v.dependencies.push(golden.clone());
        f.state.select("inputs/demo", "v1").unwrap();
        f.save();
        write(
            &f,
            "inputs.json",
            &json!({"schema":"caminos.artifact-record.v1","artifact_id":"inputs/demo","kind":"episode-input-inventory","current_version":"v1","versions":[{"version_id":"v1","manifest":descriptor(&iph,ip.len() as u64),"version_lock":ilock}]}),
        );
        write(
            &f,
            "episode-record.json",
            &json!({"schema":"caminos.artifact-record.v1","artifact_id":"episode/demo","kind":"episode","current_version":"v1","versions":[{"version_id":"v1","manifest":descriptor(&hash,package.len() as u64),"version_lock":golden},{"version_id":"v0","manifest":descriptor(&old_hash,old_manifest.len() as u64),"version_lock":{"logical_id":"episode/demo","version_id":"v0","manifest_sha256":old_hash}}]}),
        );
        profile(&f, json!(["inputs.json", "episode-record.json"]));
        (f, media, iph)
    }
    #[test]
    fn repository_lfs_controls_resolve_registered_metadata_and_reject_corrupt_first_tier() {
        let (f, _, _) = fixture();
        let path = f.root().join("episode-record.json");
        let bytes = fs::read(&path).unwrap();
        let hash = f.put(&bytes, true);
        fs::write(
            &path,
            crate::registry::pointer(&hash, bytes.len() as u64).unwrap(),
        )
        .unwrap();
        profile(&f, json!(["episode-record.json"]));
        let report = inspect(f.root(), "inspect.json").unwrap();
        assert_eq!(
            report.nodes["repo:episode-record.json"].schema.as_deref(),
            Some("caminos.artifact-record.v1")
        );
        assert!(
            report
                .edges
                .iter()
                .any(|e| e.kind == "lfs-control-document" && e.to.ends_with(&hash))
        );
        assert_eq!(
            report.nodes[&format!("cache://sha256/{hash}")].status,
            "metadata-sha256-verified"
        );
        let d = object(&f.root().join("history"), &hash);
        fs::create_dir_all(d.parent().unwrap()).unwrap();
        fs::write(d, &bytes).unwrap();
        fs::write(
            object(&f.root().join("fast"), &hash),
            vec![b'x'; bytes.len()],
        )
        .unwrap();
        let report = inspect(f.root(), "inspect.json").unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == "unreadable-document"
                    && finding.detail.contains("hash/size mismatch"))
        );
        assert!(!f.root().join("offline-cold").exists());
    }
    #[test]
    fn inventory_membership_history_and_required_versions_remain_distinct() {
        let (f, media, _) = fixture();
        let r = inspect(f.root(), "inspect.json").unwrap();
        assert!(
            r.edges
                .iter()
                .any(|e| e.kind == "inventory-member" && e.to.ends_with(&media))
        );
        assert!(r.edges.iter().any(|e| e.kind == "requires-version"));
        assert!(r.edges.iter().any(|e| e.kind == "historical-version"));
        assert!(
            !r.edges
                .iter()
                .any(|e| e.kind == "requires-version" && e.to.starts_with("cache:"))
        );
        assert!(
            r.findings.iter().all(|f| f.code == "uninspected-fields"),
            "{}",
            serde_json::to_string(&r.findings).unwrap()
        );
        assert!(!f.root().join("offline-cold").exists());
        assert!(
            query(&r, &format!("cache://sha256/{media}")).unwrap()["incoming"]
                .as_array()
                .unwrap()
                .len()
                > 1
        );
    }
    #[test]
    fn json_payloads_and_hashes_in_opaque_metadata_are_not_traversed() {
        let (mut f, _, _) = fixture();
        let trap = "a".repeat(64);
        let bytes = serde_json::to_vec(&json!({"schema":"unknown-report","sha256":trap})).unwrap();
        let hash = f.put(&bytes, true);
        let a = descriptor(&hash, bytes.len() as u64);
        let bytes=serde_json::to_vec(&json!({"schema":"caminos.artifact-package.v1","artifact_id":"report","kind":"report","version_id":"v1","assets":{"inventory":a},"evidence":{"sha256":trap}})).unwrap();
        let h = f.put(&bytes, true);
        let lock = f.version("report", &h);
        f.state
            .versions
            .get_mut("report@v1")
            .unwrap()
            .payloads
            .insert(hash.clone());
        f.state.select("report", "v1").unwrap();
        f.save();
        write(
            &f,
            "report.json",
            &json!({"schema":"caminos.artifact-record.v1","artifact_id":"report","kind":"report","current_version":"v1","versions":[{"version_id":"v1","manifest":descriptor(&h,bytes.len() as u64),"version_lock":lock}]}),
        );
        profile(&f, json!(["report.json"]));
        let r = inspect(f.root(), "inspect.json").unwrap();
        assert!(!r.nodes.keys().any(|k| k.contains(&trap)));
        assert_eq!(r.nodes[&format!("cache://sha256/{hash}")].schema, None);
        assert!(!r.findings.iter().any(|f| f.code == "unsupported-schema"));
    }
    #[test]
    fn corruption_in_first_tier_holds_even_with_valid_second_tier() {
        let (f, _, hash) = fixture();
        let path = object(&f.root().join("fast"), &hash);
        let len = fs::metadata(&path).unwrap().len();
        fs::write(path, vec![b'x'; len as usize]).unwrap();
        let r = inspect(f.root(), "inspect.json").unwrap();
        assert!(r.findings.iter().any(|f|f.code=="unreadable-document" && f.detail.contains("hash/size mismatch")));
    }
    #[test]
    fn missing_wrong_size_unknown_and_bad_descriptors_are_explicit() {
        let (f, media, _) = fixture();
        fs::write(object(&f.root().join("fast"), &media), b"wrong").unwrap();
        write(
            &f,
            "unknown.json",
            &json!({"schema":"unknown.v1","sha256":"a".repeat(64)}),
        );
        write(
            &f,
            "bad.json",
            &json!({"schema":"reel.scene-asset-bindings.v1","assets":{"bad":{"sha256":"b".repeat(64),"bytes":2,"cache_uri":"wrong"}}}),
        );
        profile(
            &f,
            json!(["inputs.json", "unknown.json", "bad.json", "missing.json"]),
        );
        let r = inspect(f.root(), "inspect.json").unwrap();
        for code in [
            "wrong-size-active-copy",
            "unsupported-schema",
            "invalid-contract",
            "unreadable-document",
        ] {
            assert!(r.findings.iter().any(|f| f.code == code), "{code}");
        }
        assert!(!r.nodes.keys().any(|k| k.contains(&"a".repeat(64))));
    }
    #[test]
    fn budgets_abort_instead_of_publishing_a_partial_success() {
        let (f, _, _) = fixture();
        for limits in [
            json!({"nodes":1}),
            json!({"edges":1}),
            json!({"documents":1}),
            json!({"depth":1}),
            json!({"document_bytes":1}),
            json!({"total_document_bytes":1}),
        ] {
            write(
                &f,
                "inspect.json",
                &json!({"schema":PROFILE,"project_id":"synthetic","roots":["inputs.json"],"limits":limits}),
            );
            assert!(inspect(f.root(), "inspect.json").is_err(), "{limits}");
        }
    }
    #[test]
    fn native_episode_uses_existing_parser_and_resolves_only_explicit_bindings() {
        let (f, _, _) = fixture();
        write(
            &f,
            "episode.json",
            &json!({"schema":"reel.episode-authoring.v1","episode_id":"demo","season_id":"one","authoring_state":"draft","master_template_id":"master","scene_policy_id":"policy","presentation":[],"score_palette":[],"scene_ids":["scene-one"]}),
        );
        profile(&f, json!(["episode.json"]));
        let r = inspect(f.root(), "inspect.json").unwrap();
        assert_eq!(
            r.edges
                .iter()
                .filter(|e| e.kind == "requires-scene")
                .count(),
            1
        );
        assert_eq!(
            r.findings
                .iter()
                .filter(|f| f.code == "unresolved-logical-reference")
                .count(),
            3
        );
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "partial-native-extraction")
        );
    }
    #[test]
    fn exact_locks_schema_expectations_and_cycles_are_checked() {
        let (mut f, _, hash) = fixture();
        let inv = f.state.versions["inputs/demo@v1"].clone();
        f.state
            .versions
            .get_mut("episode/demo@v1")
            .unwrap()
            .dependencies
            .push(versions::Dependency {
                logical_id: inv.logical_id,
                version_id: inv.version_id,
                manifest_sha256: hash,
            });
        f.save();
        let r = inspect(f.root(), "inspect.json").unwrap();
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "required-dependency-cycle")
        );
        f.state
            .versions
            .get_mut("inputs/demo@v1")
            .unwrap()
            .dependencies[0]
            .manifest_sha256 = "a".repeat(64);
        f.save();
        let r = inspect(f.root(), "inspect.json").unwrap();
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "version-manifest-mismatch")
        );
    }
    #[test]
    fn report_queries_reject_tampering_and_html_cannot_escape_json_container() {
        let (f, media, _) = fixture();
        let mut r = inspect(f.root(), "inspect.json").unwrap();
        r.project_id = "</script><script>alert(1)</script>&".into();
        assert!(query(&r, &format!("cache://sha256/{media}")).is_err());
        r.report_sha256 = report_digest(&r).unwrap();
        let path = f.root().join("review.html");
        review(&r, &path).unwrap();
        let html = fs::read_to_string(&path).unwrap();
        assert!(!html.contains("</script><script>alert(1)"));
        assert!(html.contains("connect-src 'none'"));
        assert!(review(&r, &path).is_err());
    }
    #[test]
    fn paths_cannot_escape_and_catalogue_is_honest_about_projection_coverage() {
        let (f, _, _) = fixture();
        assert!(relative(f.root(), "../escape").is_err());
        assert!(relative(f.root(), "C:/escape").is_err());
        assert!(relative(f.root(), "foo\\bar").is_err());
        let c = catalogue().unwrap();
        assert_eq!(
            c["contracts"][0]["json_schema"]["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(
            c["contracts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["schema"] == "reel.episode-authoring.v1" && c["json_schema"].is_null())
        );
        assert_eq!(
            c["registry_structures"]["immutable_version"]["additionalProperties"],
            false
        );
        assert!(
            c["registry_structures"]["immutable_version"]["properties"]["dependency_contract"]
                .is_object()
        );
    }
    #[test]
    fn inventory_parts_are_typed_members_and_index_links_are_resolved() {
        let (mut f, media, _) = fixture();
        let golden = versions::Dependency {
            logical_id: "episode/demo".into(),
            version_id: "v1".into(),
            manifest_sha256: f.state.versions["episode/demo@v1"].manifest_sha256.clone(),
        };
        let bytes=serde_json::to_vec(&json!({"schema":"caminos.artifact-package.v1","artifact_id":"part/demo","kind":"episode-input-part","version_id":"v1","assets":{"input/shared":descriptor(&media,15)},"episode_golden":golden})).unwrap();
        let h = f.put(&bytes, true);
        let part = f.version("part/demo", &h);
        f.state
            .versions
            .get_mut("part/demo@v1")
            .unwrap()
            .payloads
            .insert(media.clone());
        f.state.select("part/demo", "v1").unwrap();
        f.save();
        write(
            &f,
            "part.json",
            &json!({"schema":"caminos.artifact-record.v1","artifact_id":"part/demo","kind":"episode-input-part","current_version":"v1","versions":[{"version_id":"v1","manifest":descriptor(&h,bytes.len() as u64),"version_lock":part}]}),
        );
        write(
            &f,
            "inventory.json",
            &json!({"schema":"caminos.episode-input-inventory.v1","episode_id":"demo","episode_golden":golden,"verified_assets":[],"parts":[{"record":"part.json","version_id":"v1","manifest":descriptor(&h,bytes.len() as u64),"version_lock":part}],"unavailable_ancestral_source_references":[{"source":"unknown","bytes":null}]}),
        );
        profile(&f, json!(["inventory.json"]));
        let r = inspect(f.root(), "inspect.json").unwrap();
        assert!(r.edges.iter().any(|e| e.kind == "inventory-part"));
        assert!(r.edges.iter().any(|e| e.from.ends_with(&h)
            && e.to.ends_with(&media)
            && e.kind == "inventory-member"));
        assert!(r.findings.iter().any(|f| f.code == "unavailable-ancestry"));
        assert!(!r.findings.iter().any(|f| f.code == "invalid-contract"));
    }
    #[test]
    fn concurrent_source_registry_and_configuration_changes_fail_without_a_global_lock() {
        let (f, _, _) = fixture();
        let result = inspect_with(f.root(), "inspect.json", || {
            fs::write(f.root().join("inputs.json"), b"{}").unwrap()
        });
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("changed during inspection")
        );
        let (mut f, _, _) = fixture();
        let root = f.root().to_owned();
        let result = inspect_with(&root, "inspect.json", || {
            f.state.generation += 1;
            f.save();
        });
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Registry changed")
        );
    }
    #[test]
    fn duplicate_targets_still_enforce_schema_expectations() {
        let (f, _, _) = fixture();
        write(
            &f,
            "episode.json",
            &json!({"schema":"reel.episode-authoring.v1","episode_id":"demo","season_id":"one","authoring_state":"draft","master_template_id":"master","scene_policy_id":"policy","presentation":[],"score_palette":[],"scene_ids":["scene-one"]}),
        );
        write(
            &f,
            "inspect.json",
            &json!({"schema":PROFILE,"project_id":"synthetic","roots":["episode.json"],"documents":{"scene-one":"episode.json","policy":"episode.json","master":"episode.json"}}),
        );
        let r = inspect(f.root(), "inspect.json").unwrap();
        assert_eq!(
            r.findings
                .iter()
                .filter(|f| f.code == "wrong-target-schema")
                .count(),
            3
        );
        assert_eq!(
            r.findings
                .iter()
                .filter(|f| f.code == "wrong-logical-identity")
                .count(),
            3
        );
    }
}

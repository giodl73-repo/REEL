//! Explicit combined positive protection and restrictive planner shadow. No
//! cache deletion, global discovery, job locks or removal authorization.
use super::*;
use crate::tiering;
use serde_json::{Value, json};
pub mod command;
pub mod custody;
mod jobs;
#[cfg(test)]
mod tests;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogScope {
    pub directory: PathBuf,
    pub project_id: String,
    pub authority_namespace: String,
    pub catalog_id: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheScope {
    pub cache: PathBuf,
    pub staging_parent: PathBuf,
    pub retained_parent: PathBuf,
    pub catalogs: Vec<CatalogScope>,
    #[serde(default)]
    pub job_parent: Option<PathBuf>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consumer {
    pub root: PathBuf,
    pub policy: PathBuf,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub scopes: usize,
    pub sessions: usize,
    pub retained_roots: usize,
    pub files: usize,
    pub metadata_bytes: u64,
    pub node_bytes: u64,
    pub data_bytes: u64,
    pub graph_nodes: usize,
    pub objects: usize,
    pub holds: usize,
    pub catalog_keys: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            scopes: 32,
            sessions: 4096,
            retained_roots: 4096,
            files: 100_000,
            metadata_bytes: 64 * 1024 * 1024,
            node_bytes: 256 * 1024,
            data_bytes: 1024 * 1024 * 1024,
            graph_nodes: 100_000,
            objects: 1_000_000,
            holds: 1_000_000,
            catalog_keys: 10_000,
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub caches: Vec<CacheScope>,
    pub consumers: Vec<Consumer>,
    #[serde(default)]
    pub limits: Limits,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    Selected,
    WorkingOrPinned,
    JobLease,
    RegistryControl,
    TierPin,
    Staging,
    RetainedIndex,
    CatalogIndex,
    RequiredSource,
    JobWorkspaceInput,
    JobWorkspaceOutput,
}
pub enum Membership<'a> {
    Held(&'a BTreeSet<Reason>),
    OutsideDeclaredScope,
}
pub struct Snapshot {
    request: Request,
    holds: BTreeMap<String, BTreeSet<Reason>>,
    stamp: BTreeMap<String, Blob>,
    catalogs: Vec<(catalog::Catalog, catalog::protection::ProtectionSnapshot)>,
}
impl Snapshot {
    pub fn classify(&self, subject: &str) -> Result<Membership<'_>> {
        if !valid_oid(subject) {
            return Err("Invalid collector subject".into());
        }
        Ok(self
            .holds
            .get(subject)
            .map_or(Membership::OutsideDeclaredScope, Membership::Held))
    }
    pub fn held_subjects(&self) -> usize {
        self.holds.len()
    }
    pub fn revalidate(&self) -> Result<()> {
        let fresh = capture(&self.request)?;
        if fresh.stamp != self.stamp || fresh.holds != self.holds {
            return Err("Combined protection snapshot changed".into());
        }
        for (catalog, snapshot) in &self.catalogs {
            snapshot.revalidate(catalog)?;
        }
        Ok(())
    }
}
struct Reader {
    limits: Limits,
    files: usize,
    entries: usize,
    metadata: u64,
    data: u64,
    graph_nodes: usize,
    objects: usize,
    catalog_keys: usize,
    stamp: BTreeMap<String, Blob>,
}
impl Reader {
    fn node(&mut self, path: &Path, max: u64) -> Result<Vec<u8>> {
        if self.graph_nodes >= self.limits.graph_nodes {
            return Err("Collector aggregate node budget exceeded before read".into());
        }
        self.graph_nodes += 1;
        self.file(path, true, max)
    }
    fn file(&mut self, path: &Path, data: bool, max: u64) -> Result<Vec<u8>> {
        let count = self.files.checked_add(1).ok_or("Collector file overflow")?;
        let bytes = plain(path, false)?.len();
        let used = if data {
            &mut self.data
        } else {
            &mut self.metadata
        };
        let next = used.checked_add(bytes).ok_or("Collector byte overflow")?;
        if count > self.limits.files
            || bytes > max
            || next
                > if data {
                    self.limits.data_bytes
                } else {
                    self.limits.metadata_bytes
                }
        {
            return Err("Collector file/byte budget exceeded before read".into());
        }
        let raw = if bytes == 0 {
            Vec::new()
        } else {
            read_limited(File::open(path)?, bytes)?
        };
        self.files = count;
        *used = next;
        self.stamp.insert(
            path.to_string_lossy().into_owned(),
            Blob {
                sha256: hash(&raw),
                bytes,
            },
        );
        Ok(raw)
    }
    fn control<T: serde::de::DeserializeOwned + Serialize>(&mut self, path: &Path) -> Result<T> {
        decode(&self.file(path, false, CONTROL_BYTES)?)
    }
    fn directory(&mut self, path: &Path, max: usize) -> Result<Vec<PathBuf>> {
        checked_directory(path)?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(path)? {
            if paths.len() >= max || self.entries >= self.limits.files {
                return Err("Collector directory budget exceeded".into());
            }
            self.entries += 1;
            paths.push(entry?.path());
        }
        paths.sort();
        let names: Vec<_> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let bytes = serde_json::to_vec(&names)?;
        // Directory names consume metadata budget too, before retaining the stamp.
        self.metadata = self
            .metadata
            .checked_add(bytes.len() as u64)
            .ok_or("Collector directory byte overflow")?;
        if self.metadata > self.limits.metadata_bytes {
            return Err("Collector directory metadata budget exceeded".into());
        }
        self.stamp.insert(
            format!("directory:{}", path.display()),
            Blob {
                sha256: hash(&bytes),
                bytes: bytes.len() as u64,
            },
        );
        Ok(paths)
    }
}
fn hold(
    holds: &mut BTreeMap<String, BTreeSet<Reason>>,
    subject: &str,
    reason: Reason,
    max: usize,
) -> Result<()> {
    if !valid_oid(subject) {
        return Err("Invalid collector protection descriptor".into());
    }
    if !holds.contains_key(subject) && holds.len() >= max {
        return Err("Collector hold budget exceeded".into());
    }
    holds.entry(subject.into()).or_default().insert(reason);
    Ok(())
}
fn optional(reader: &mut Reader, path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            reader.file(path, false, CONTROL_BYTES)?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn stage(
    reader: &mut Reader,
    scope: &CacheScope,
    holds: &mut BTreeMap<String, BTreeSet<Reason>>,
) -> Result<()> {
    for session in reader.directory(&scope.staging_parent, reader.limits.sessions)? {
        checked_directory(&session)?;
        let name = session
            .file_name()
            .unwrap()
            .to_str()
            .ok_or("Invalid session filename")?;
        session_path(&scope.staging_parent, name)?;
        let intent: Session = reader.control(&session.join("session.json"))?;
        check_limits(intent.limits)?;
        if intent.schema != SESSION_SCHEMA
            || intent.cache != scope.cache
            || intent.staging_parent != scope.staging_parent
            || intent.id != name
        {
            return Err("Staging session scope mismatch".into());
        }
        let contents = reader.directory(&session, 8)?;
        for entry in contents {
            let entry_name = entry
                .file_name()
                .unwrap()
                .to_str()
                .ok_or("Invalid session control name")?;
            match entry_name {
                "objects" => (),
                "session.json" => (),
                "owner.lock" | "capacity.json" | "root.json" | "retirement.pending"
                | "retirement.json" => {
                    reader.file(&entry, false, CONTROL_BYTES)?;
                }
                _ => return Err("Unknown staging control; scope incomplete".into()),
            }
        }
        let mut bytes = 0u64;
        for object in reader.directory(&session.join("objects"), intent.limits.nodes)? {
            let subject = object
                .file_name()
                .unwrap()
                .to_str()
                .ok_or("Invalid staged subject")?;
            if !valid_oid(subject) {
                return Err("Unknown staged filename".into());
            }
            let raw = reader.node(
                &object,
                intent.limits.node_bytes.min(reader.limits.node_bytes),
            )?;
            bytes = bytes
                .checked_add(raw.len() as u64)
                .ok_or("Staging byte overflow")?;
            if bytes > intent.limits.staged_bytes {
                return Err("Staged byte budget exceeded".into());
            }
            // A partial in-flight write is demand/protection, NOT verified custody.
            hold(holds, subject, Reason::Staging, reader.limits.holds)?;
        }
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Retained {
    schema: String,
    root: Descriptor,
    source: Value,
    closure: String,
    nodes: usize,
    bytes: u64,
}
fn retained(
    reader: &mut Reader,
    scope: &CacheScope,
    holds: &mut BTreeMap<String, BTreeSet<Reason>>,
) -> Result<()> {
    for root in reader.directory(&scope.retained_parent, reader.limits.retained_roots)? {
        let subject = root
            .file_name()
            .unwrap()
            .to_str()
            .ok_or("Invalid retained root filename")?;
        if !valid_oid(subject) {
            return Err("Unknown retained root".into());
        }
        checked_directory(&root)?;
        let _owner = reader_owner(&root.join("owner.lock"))?;
        for entry in reader.directory(&root, 4)? {
            match entry
                .file_name()
                .unwrap()
                .to_str()
                .ok_or("Invalid retained control name")?
            {
                "objects" | "root.json" | "root.pending" => (),
                "owner.lock" => {
                    reader.file(&entry, false, CONTROL_BYTES)?;
                }
                _ => return Err("Unknown retained control; scope incomplete".into()),
            }
        }
        let receipt: Retained = reader.control(&root.join("root.json"))?;
        if receipt.schema != "reel.inventory-sidecar.retained.v1"
            || receipt.root.sha256 != subject
            || !valid_oid(&receipt.closure)
            || receipt.nodes > reader.limits.graph_nodes
        {
            return Err("Retained root scope/budget mismatch".into());
        }
        let mut descriptors = BTreeMap::new();
        let mut bytes = 0u64;
        for path in reader.directory(&root.join("objects"), receipt.nodes)? {
            let id = path
                .file_name()
                .unwrap()
                .to_str()
                .ok_or("Invalid retained node name")?;
            if !valid_oid(id) {
                return Err("Unknown retained node".into());
            }
            let raw = reader.node(&path, reader.limits.node_bytes)?;
            if hash(&raw) != id {
                return Err("Retained metadata corruption".into());
            }
            let blob = Blob {
                sha256: id.into(),
                bytes: raw.len() as u64,
            };
            bytes = bytes
                .checked_add(blob.bytes)
                .ok_or("Retained byte overflow")?;
            descriptors.insert(id.to_owned(), blob);
            hold(holds, id, Reason::RetainedIndex, reader.limits.holds)?;
        }
        let wire: Vec<_> = descriptors.values().map(Descriptor::from).collect();
        if descriptors.len() != receipt.nodes
            || bytes != receipt.bytes
            || descriptors.get(subject) != Some(&receipt.root.blob())
            || hash(&encode(&wire)?) != receipt.closure
        {
            return Err("Incomplete retained metadata protection".into());
        }
        // Positive provenance demand only; receipt data grants no custody or removal rights.
        let source = catalog::source_from_key(&receipt.source)?;
        hold(
            holds,
            &source.encoded.sha256,
            Reason::RequiredSource,
            reader.limits.holds,
        )?;
        optional(reader, &root.join("root.pending"))?;
    }
    Ok(())
}
fn normalize(request: &Request) -> Result<Request> {
    if request.schema != "reel.collector-protection-scope.v1"
        || request.caches.len() > request.limits.scopes
        || request.consumers.is_empty()
        || request.consumers.len() > request.limits.scopes
        || request.limits.scopes > 32
        || request.limits.files > 1_000_000
        || request.limits.holds > 1_000_000
        || request.limits.sessions > 100_000
        || request.limits.retained_roots > 100_000
        || request.limits.metadata_bytes > 256 * 1024 * 1024
        || request.limits.data_bytes > 4 * 1024 * 1024 * 1024
        || request.limits.graph_nodes > 1_000_000
        || request.limits.objects > 1_000_000
        || request.limits.catalog_keys > 100_000
    {
        return Err("Invalid collector scope or budget".into());
    }
    check_node_limit(request.limits.node_bytes)?;
    let mut request = request.clone();
    let mut parents = BTreeSet::new();
    let mut consumers = BTreeSet::new();
    let mut catalogs = BTreeSet::new();
    for scope in &mut request.caches {
        scope.cache = checked_directory(&scope.cache)?;
        scope.staging_parent = checked_directory(&scope.staging_parent)?;
        scope.retained_parent = checked_directory(&scope.retained_parent)?;
        if let Some(job_parent) = &mut scope.job_parent {
            *job_parent = checked_directory(job_parent)?;
            if *job_parent == scope.cache
                || !job_parent.starts_with(&scope.cache)
                || crate::admission::volume_id(job_parent)?
                    != crate::admission::volume_id(&scope.cache)?
                || !parents.insert(job_parent.clone())
            {
                return Err("Invalid explicit job workspace namespace".into());
            }
        }
        for parent in [&scope.staging_parent, &scope.retained_parent] {
            if *parent == scope.cache
                || !parent.starts_with(&scope.cache)
                || !parents.insert(parent.clone())
            {
                return Err("Overlapping/duplicate collector namespaces".into());
            }
            if crate::admission::volume_id(parent)? != crate::admission::volume_id(&scope.cache)? {
                return Err("Collector metadata namespace is on another filesystem".into());
            }
        }
        if scope.staging_parent.starts_with(&scope.retained_parent)
            || scope.retained_parent.starts_with(&scope.staging_parent)
            || scope.catalogs.len() > request.limits.scopes
        {
            return Err("Invalid collector metadata namespace coverage".into());
        }
        for catalog in &mut scope.catalogs {
            catalog.directory = checked_directory(&catalog.directory)?;
            if !catalog.directory.starts_with(&scope.cache)
                || catalog.directory == scope.cache
                || !catalogs.insert(catalog.directory.clone())
                || catalogs.len() > request.limits.scopes
            {
                return Err("Duplicate/out-of-scope collector catalog".into());
            }
        }
    }
    let all_parents: Vec<_> = parents.iter().chain(catalogs.iter()).collect();
    for (i, first) in all_parents.iter().enumerate() {
        for second in all_parents.iter().skip(i + 1) {
            if first.starts_with(second) || second.starts_with(first) {
                return Err("Collector namespaces overlap".into());
            }
        }
    }
    for consumer in &mut request.consumers {
        consumer.root = checked_directory(&consumer.root)?;
        if !consumer.policy.is_absolute() || !consumers.insert(consumer.root.clone()) {
            return Err("Duplicate or non-explicit collector consumer".into());
        }
        checked_directory(
            consumer
                .policy
                .parent()
                .ok_or("Missing consumer policy parent")?,
        )?;
    }
    Ok(request)
}
fn capture_once(request: &Request) -> Result<Snapshot> {
    let mut reader = Reader {
        limits: request.limits,
        files: 0,
        entries: 0,
        metadata: 0,
        data: 0,
        graph_nodes: 0,
        objects: 0,
        catalog_keys: 0,
        stamp: BTreeMap::new(),
    };
    let mut holds = BTreeMap::new();
    let mut catalogs = Vec::new();
    for scope in &request.caches {
        stage(&mut reader, scope, &mut holds)?;
        retained(&mut reader, scope, &mut holds)?;
        jobs::capture(&mut reader, scope, &mut holds)?;
        for config in &scope.catalogs {
            let catalog = catalog::Catalog::open(
                &scope.cache,
                &config.directory,
                &catalog::Scope {
                    project_id: config.project_id.clone(),
                    authority_namespace: config.authority_namespace.clone(),
                    catalog_id: config.catalog_id.clone(),
                },
            )?;
            let snapshot = catalog::protection::collect(
                &catalog,
                catalog::protection::Limits {
                    keys: request.limits.catalog_keys - reader.catalog_keys,
                    metadata_bytes: request.limits.metadata_bytes - reader.metadata,
                    node_bytes: request.limits.node_bytes,
                    total_bytes: request.limits.data_bytes - reader.data,
                    nodes: request.limits.graph_nodes - reader.graph_nodes,
                    objects: request.limits.objects - reader.objects,
                },
            )?;
            let (data, nodes, objects, metadata) = snapshot.work();
            reader.data += data;
            reader.graph_nodes += nodes;
            reader.objects += objects;
            reader.metadata += metadata;
            reader.catalog_keys += snapshot.keys();
            for (id, _) in snapshot.descriptors() {
                hold(&mut holds, id, Reason::CatalogIndex, reader.limits.holds)?;
            }
            for (id, _) in snapshot.source_descriptors() {
                hold(&mut holds, id, Reason::RequiredSource, reader.limits.holds)?;
            }
            catalogs.push((catalog, snapshot));
        }
    }
    for consumer in &request.consumers {
        let policy_raw = reader.file(&consumer.policy, false, CONTROL_BYTES * 4)?;
        let policy: tiering::Policy = serde_json::from_slice(&policy_raw)?;
        for name in ["asset-project.json", "registry/versions.json"] {
            reader.file(
                &consumer.root.join(name),
                false,
                request.limits.metadata_bytes,
            )?;
        }
        let live = tiering::collector_live(&consumer.root, &policy)?;
        reader.stamp.insert(
            format!("consumer:{}", consumer.root.display()),
            Blob {
                sha256: live.fingerprint.clone(),
                bytes: 0,
            },
        );
        for (set, reason) in [
            (&live.selected, Reason::Selected),
            (&live.current, Reason::WorkingOrPinned),
            (&live.leased, Reason::JobLease),
            (&live.controls, Reason::RegistryControl),
        ] {
            for id in set {
                hold(&mut holds, id, reason, request.limits.holds)?;
            }
        }
        for id in policy.tier_pins.keys() {
            hold(&mut holds, id, Reason::TierPin, request.limits.holds)?;
        }
    }
    Ok(Snapshot {
        request: request.clone(),
        holds,
        stamp: reader.stamp,
        catalogs,
    })
}
/// Double collection establishes stable declared scopes, not complete knowledge
/// of all projects/caches. No absence result or execution lease is issued.
pub fn capture(request: &Request) -> Result<Snapshot> {
    let request = normalize(request)?;
    let first = capture_once(&request)?;
    let second = capture_once(&request)?;
    if first.stamp != second.stamp || first.holds != second.holds {
        return Err("Collector protection changed during capture; retry".into());
    }
    for (catalog, snapshot) in &first.catalogs {
        snapshot.revalidate(catalog)?;
    }
    Ok(second)
}

pub struct Comparison {
    baseline: Value,
    shadow: Value,
    summary: Summary,
}
#[derive(Serialize)]
pub struct Summary {
    pub schema: &'static str,
    pub objects: usize,
    pub protected_subjects: usize,
    pub held_proposals: usize,
    pub unchanged_rows: usize,
    pub reference_parity: bool,
    pub dry_run: bool,
    pub removal_authorized: bool,
    pub payload_bytes_copied: u64,
    pub declared_scope_only: bool,
}
impl Comparison {
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
    pub fn baseline(&self) -> &Value {
        &self.baseline
    }
    pub fn shadow(&self) -> &Value {
        &self.shadow
    }
}
/// Run the unchanged reference planner once. Overlay ONLY additional holds;
/// batch slots are not refilled and no new action is introduced. Shadow schema
/// is intentionally rejected by the existing removal/check-plan workflow.
pub fn compare(
    root: &Path,
    policy: &Path,
    census: &Path,
    request: &Request,
    now: u64,
) -> Result<Comparison> {
    let root = checked_directory(root)?;
    let policy = policy.canonicalize()?;
    if !request.consumers.iter().any(|c| {
        c.root.canonicalize().ok().as_ref() == Some(&root)
            && c.policy.canonicalize().ok().as_ref() == Some(&policy)
    }) {
        return Err("Planner root/policy missing from explicit collector coverage".into());
    }
    let snapshot = capture(request)?;
    let baseline = tiering::plan(&root, &policy, census, now)?;
    let mut shadow = baseline.clone();
    let rows = shadow["objects"]
        .as_array_mut()
        .ok_or("Missing reference plan rows")?;
    let mut held = 0usize;
    for row in rows.iter_mut() {
        let subject = row["sha256"].as_str().ok_or("Missing plan subject")?;
        if matches!(snapshot.classify(subject)?, Membership::Held(_))
            && matches!(
                row["proposal"].as_str(),
                Some(
                    "verify-and-demote"
                        | "verify-and-archive-output"
                        | "register-and-archive-first"
                )
            )
        {
            row["proposal"] = json!("hold");
            row["reason"] = json!("explicit combined collector protection");
            row["estimated_hot_reclaim_bytes"] = Value::Null;
            held += 1;
        }
    }
    let mut candidates = 0usize;
    let mut bytes = 0u64;
    for row in rows.iter() {
        if matches!(
            row["proposal"].as_str(),
            Some("verify-and-demote" | "verify-and-archive-output")
        ) {
            candidates += 1;
            bytes = bytes
                .checked_add(row["bytes"].as_u64().ok_or("Invalid plan size")?)
                .ok_or("Shadow batch overflow")?;
        }
    }
    shadow["schema"] = json!("reel.asset-collector-shadow-plan.v1");
    shadow["collector_scope_is_not_global"] = json!(true);
    shadow["collector_removal_authority"] = json!(false);
    shadow["candidate_batch_objects"] = json!(candidates);
    shadow["candidate_batch_logical_bytes"] = json!(bytes);
    // Recompute estimates from remaining rows, never invent physical savings.
    let mut estimates: BTreeMap<String, u64> = BTreeMap::new();
    for row in shadow["objects"].as_array().unwrap() {
        let class = row["class"].as_str().ok_or("Missing plan class")?;
        let count = estimates.entry(class.into()).or_default();
        *count = count
            .checked_add(row["estimated_hot_reclaim_bytes"].as_u64().unwrap_or(0))
            .ok_or("Shadow estimate overflow")?;
    }
    for (class, entry) in shadow["summary"]
        .as_object_mut()
        .ok_or("Missing plan summary")?
    {
        entry["estimated_hot_reclaim_bytes"] = json!(estimates.get(class).copied().unwrap_or(0));
    }
    shadow.as_object_mut().unwrap().remove("plan_sha256");
    shadow["plan_sha256"] = json!(tiering::digest(&serde_json::to_vec(&shadow)?));
    snapshot.revalidate()?;
    let summary = Summary {
        schema: "reel.collector-shadow-summary.v1",
        objects: baseline["objects"].as_array().unwrap().len(),
        protected_subjects: snapshot.held_subjects(),
        held_proposals: held,
        unchanged_rows: baseline["objects"].as_array().unwrap().len() - held,
        reference_parity: held == 0,
        dry_run: true,
        removal_authorized: false,
        payload_bytes_copied: 0,
        declared_scope_only: true,
    };
    Ok(Comparison {
        baseline,
        shadow,
        summary,
    })
}

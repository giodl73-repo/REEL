//! Agent-oriented read operations over existing native catalogue/graph mechanisms.
use super::*;
use serde::{Deserialize, Serialize};

mod admission;
mod compare_view;
mod impact;
mod manifest_impact;
mod manifests;
mod preview;
mod view;
pub const REQUEST_SCHEMA: &[u8] = include_bytes!("../../../../specs/AGENT-QUERY.schema.json");
pub const RESULT_SCHEMA: &[u8] = include_bytes!("../../../../specs/AGENT-QUERY-RESULT.schema.json");

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub project: String,
    pub id: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Projects,
    Works {
        project: String,
        #[serde(default)]
        collection: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        state: String,
        #[serde(default)]
        facet: String,
    },
    ManifestReferences {
        project: String,
        collection: String,
        work: String,
        sha256: String,
        #[serde(default)]
        expected_content: Option<git_asset_core::Digest>,
    },
    ManifestImpact {
        project: String,
        collection: String,
        work: String,
        sha256: String,
        kinds: BTreeSet<graph::EdgeKind>,
        #[serde(default)]
        expected_content: Option<git_asset_core::Digest>,
    },
    Inspect {
        project: String,
        asset: String,
    },
    Admission {
        project: String,
        kinds: BTreeSet<graph::EdgeKind>,
        #[serde(default)]
        definition: String,
    },
    PreviewAdmission {
        project: String,
        asset: String,
        kinds: BTreeSet<graph::EdgeKind>,
        profiles: Vec<graph::Profile>,
    },
    Find {
        project: String,
        text: String,
        #[serde(default)]
        category: String,
    },
    Shared {
        left: String,
        right: String,
        #[serde(default)]
        category: String,
    },
    References {
        project: String,
        sha256: String,
    },
    Compare {
        left: Asset,
        right: Asset,
    },
    #[serde(alias = "where_used")]
    Impact {
        project: String,
        asset: String,
        kinds: BTreeSet<graph::EdgeKind>,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub enrollment: PathBuf,
    pub query: Operation,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub expected_snapshots: BTreeMap<String, String>,
    #[serde(default)]
    pub expected_basis: Option<git_asset_core::Digest>,
}
impl Request {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() > 65536 {
            return Err("invalid_request".into());
        }
        let value = git_asset_core::identity::parse_json(raw).map_err(|_| "invalid_request")?;
        git_asset_model::schema::check_document(REQUEST_SCHEMA, Dialect::Draft202012, &value)
            .map_err(|_| "invalid_request")?;
        let request: Self = serde_json::from_value(value).map_err(|_| "invalid_request")?;
        request.validate()?;
        Ok(request)
    }
    fn validate(&self) -> Result<()> {
        let value = serde_json::to_value(self).map_err(|_| "invalid_request")?;
        git_asset_model::schema::check_document(REQUEST_SCHEMA, Dialect::Draft202012, &value)
            .map_err(|_| "invalid_request")?;
        if !self.enrollment.is_absolute() || self.offset > 1_000_000 {
            return Err("invalid_request".into());
        }
        if matches!(
            self.query,
            Operation::Projects | Operation::Compare { .. } | Operation::Inspect { .. }
        ) && self.offset != 0
        {
            return Err("invalid_request".into());
        }
        let bounded = |s: &str, n: usize| s.len() <= n && !s.chars().any(char::is_control);
        match &self.query {
            Operation::Find { text, category, .. }
                if !bounded(text, 256) || !bounded(category, 256) =>
            {
                return Err("invalid_request".into());
            }
            Operation::Shared { category, .. } if !bounded(category, 256) => {
                return Err("invalid_request".into());
            }
            Operation::Shared { left, right, .. }
                if left == "all" || right == "all" || left == right =>
            {
                return Err("invalid_request".into());
            }
            Operation::Compare { left, right }
                if !bounded(&left.id, 256) || !bounded(&right.id, 256) =>
            {
                return Err("invalid_request".into());
            }
            Operation::Compare { left, right }
                if left.project == "all" || right.project == "all" =>
            {
                return Err("invalid_request".into());
            }
            Operation::Impact { asset, project, .. }
                if !bounded(asset, 256) || project == "all" =>
            {
                return Err("invalid_request".into());
            }
            Operation::Inspect { asset, project } if !bounded(asset, 256) || project == "all" => {
                return Err("invalid_request".into());
            }
            Operation::Admission { definition, .. } if !bounded(definition, 256) => {
                return Err("invalid_request".into());
            }
            Operation::Works {
                project,
                collection,
                text,
                state,
                facet,
            } if project == "all"
                || !bounded(collection, 80)
                || !bounded(text, 256)
                || !bounded(state, 120)
                || !bounded(facet, 120) =>
            {
                return Err("invalid_request".into());
            }
            Operation::ManifestReferences {
                project,
                collection,
                work,
                expected_content,
                ..
            }
            | Operation::ManifestImpact {
                project,
                collection,
                work,
                expected_content,
                ..
            } if project == "all"
                || !bounded(collection, 80)
                || !bounded(work, 256)
                || self.expected_basis.is_none()
                || (self.offset > 0 && expected_content.is_none()) =>
            {
                return Err("invalid_request".into());
            }
            Operation::PreviewAdmission { project, asset, .. }
                if project == "all" || !bounded(asset, 256) || self.expected_basis.is_none() =>
            {
                return Err("invalid_request".into());
            }
            _ => {}
        }
        Ok(())
    }
}
impl Operation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Projects => "projects",
            Self::Works { .. } => "works",
            Self::ManifestReferences { .. } => "manifest_references",
            Self::ManifestImpact { .. } => "manifest_impact",
            Self::Inspect { .. } => "inspect",
            Self::Admission { .. } => "admission",
            Self::PreviewAdmission { .. } => "preview_admission",
            Self::Find { .. } => "find",
            Self::Shared { .. } => "shared",
            Self::References { .. } => "references",
            Self::Compare { .. } => "compare",
            Self::Impact { .. } => "impact",
        }
    }
    fn projects(&self) -> BTreeSet<&str> {
        match self {
            Self::Projects => BTreeSet::from(["all"]),
            Self::Find { project, .. }
            | Self::Works { project, .. }
            | Self::ManifestReferences { project, .. }
            | Self::ManifestImpact { project, .. }
            | Self::References { project, .. }
            | Self::Inspect { project, .. }
            | Self::Admission { project, .. }
            | Self::PreviewAdmission { project, .. }
            | Self::Impact { project, .. } => BTreeSet::from([project.as_str()]),
            Self::Shared { left, right, .. } => BTreeSet::from([left.as_str(), right.as_str()]),
            Self::Compare { left, right } => {
                BTreeSet::from([left.project.as_str(), right.project.as_str()])
            }
        }
    }
}

/// Loads only explicitly requested projects; no implicit main/working-tree lookup.
pub fn execute(mut config: Config, request: &Request) -> Result<Value> {
    request.validate()?;
    let ids = request.query.projects();
    for id in &ids {
        if *id != "all" && !config.projects.iter().any(|p| p.id == *id) {
            return Err("unknown_project".into());
        }
    }
    if !ids.contains("all") {
        config.projects.retain(|p| ids.contains(p.id.as_str()));
    }
    config.default_project = None;
    config.storage_domains.clear();
    let mut catalogue = Catalogue::load(config).map_err(|_| "source_unavailable")?;
    catalogue.agent_query(request)
}

impl Catalogue {
    pub(super) fn query_basis(
        &self,
        ids: &BTreeSet<&str>,
    ) -> Result<(BTreeMap<String, Value>, git_asset_core::Digest)> {
        let mut basis = BTreeMap::new();
        for (id, p) in self
            .projects
            .iter()
            .filter(|(id, _)| ids.contains("all") || ids.contains(id.as_str()))
        {
            let projection = json!({"query_schema":Object::from_bytes(REQUEST_SCHEMA),"display":p.enrollment.display,"fields":p.enrollment.fields,"collections":p.enrollment.collections,"graph_profiles":p.enrollment.graph_profiles,"manifest_interpretation":manifests::interpretation(p)});
            let interpretation =
                Object::from_bytes(&serde_json::to_vec(&projection).map_err(|_| "query_refused")?);
            basis.insert(id.clone(),json!({"commit":p.commit,"checkpoint":p.snapshot.checkpoint(),"records":p.profile.records,"definitions":p.profile.definitions,"interpretation":interpretation}));
        }
        let digest =
            Object::from_bytes(&serde_json::to_vec(&basis).map_err(|_| "query_refused")?).sha256;
        Ok((basis, digest))
    }

    pub fn agent_query(&mut self, request: &Request) -> Result<Value> {
        request.validate()?;
        let ids = request.query.projects();
        if ids
            .iter()
            .any(|id| *id != "all" && !self.projects.contains_key(*id))
        {
            return Err("unknown_project".into());
        }
        let snapshots: BTreeMap<_, _> = self
            .projects
            .iter()
            .filter(|(id, _)| ids.contains("all") || ids.contains(id.as_str()))
            .map(|(id, p)| (id.clone(), p.commit.clone()))
            .collect();
        if !request.expected_snapshots.is_empty() && request.expected_snapshots != snapshots {
            return Err("stale_snapshot".into());
        }
        let (basis, basis_digest) = self.query_basis(&ids)?;
        if request
            .expected_basis
            .as_ref()
            .is_some_and(|expected| expected != &basis_digest)
        {
            return Err("stale_basis".into());
        }
        let mut complete = true;
        let (data, total) = match &request.query {
            Operation::Projects => (self.summaries(), None),
            Operation::Works {
                project,
                collection,
                text,
                state,
                facet,
            } => {
                let p = self.projects.get(project).ok_or("unknown_project")?;
                if !collection.is_empty()
                    && !p.enrollment.collections.iter().any(|c| c.id == *collection)
                {
                    return Err("unknown_collection".into());
                }
                let d =
                    self.work_list((project, collection), state, facet, text, request.offset)?;
                let total = d["total"].as_u64();
                (d, total)
            }
            Operation::ManifestReferences {
                project,
                collection,
                work,
                sha256,
                expected_content,
            } => {
                let d = self.manifest_references(
                    project,
                    collection,
                    work,
                    sha256,
                    expected_content.as_ref(),
                    request.offset,
                )?;
                complete = d["complete_extraction"] == true;
                let total = d["total"].as_u64();
                (d, total)
            }
            Operation::ManifestImpact { .. } => {
                let d = self.manifest_impact_query(request)?;
                complete = d["complete_declared_coverage"] == true;
                let total = d["total_known"].as_u64();
                (d, total)
            }
            Operation::Inspect { project, asset } => {
                let p = self.projects.get(project).ok_or("unknown_project")?;
                let d = p.inspect(asset).map_err(|_| "unknown_asset")?;
                (d, None)
            }
            Operation::Admission {
                project,
                kinds,
                definition,
            } => {
                let d = self.admission_query(project, kinds, definition, request.offset)?;
                complete = d["complete_requested_admission"] == true;
                let total = d["total"].as_u64();
                (d, total)
            }
            Operation::PreviewAdmission {
                project,
                asset,
                kinds,
                profiles,
            } => {
                let d = self.preview_admission(project, asset, kinds, profiles, request.offset)?;
                complete = d["complete_comparison"] == true;
                let total = d["total"].as_u64();
                (d, total)
            }
            Operation::Find {
                project,
                text,
                category,
            } => {
                let d = self.list(project, category, text, request.offset, 100)?;
                let total = d["total"].as_u64();
                (d, total)
            }
            Operation::Shared {
                left,
                right,
                category,
            } => {
                let d = self.shared_page(left, right, category, request.offset)?;
                let total = d["shared_objects"].as_u64();
                (d, total)
            }
            Operation::References { project, sha256 } => {
                let d = self.object_references(sha256, project, request.offset)?;
                let total = d["total"].as_u64();
                (d, total)
            }
            Operation::Compare { left, right } => (self.compare_assets(left, right)?, None),
            Operation::Impact {
                project,
                asset,
                kinds,
            } => {
                let d = self.impact_query(project, asset, kinds, request.offset)?;
                complete = d["complete_declared_coverage"] == true;
                let total = d["total_known"].as_u64();
                (d, total)
            }
        };
        let returned = data["items"].as_array().map_or(0, Vec::len);
        let paging=total.map(|total|json!({"offset":request.offset,"returned":returned,"total_known":total,
            "next_offset":if (request.offset as u64+returned as u64)<total {Some(request.offset+returned)}else{None}}));
        let request_object = Object::from_bytes(
            &git_asset_core::identity::canonical_json(
                &serde_json::to_value(request).map_err(|_| "invalid_request")?,
            )
            .map_err(|_| "invalid_request")?,
        );
        Ok(
            json!({"schema":"tig.query-result.experimental-1","operation":request.query.name(),
            "status":if complete{"complete"}else{"partial"},"request":request_object,"basis":basis,"basis_sha256":basis_digest,"paging":paging,
            "coverage":{"complete_declared_scope":complete,"complete_project_scope":false,"payloads_opened":if matches!(request.query,Operation::ManifestReferences{..}|Operation::ManifestImpact{..}){Value::Null}else{json!(0)},"media_leaves_opened":0,
                "exclusions":["unregistered files","unadmitted nested inventories","other Git LFS pointers","cross-project semantic dependencies","active jobs and custody"]},
            "data":data,"authorizes_eviction":false}),
        )
    }

    fn compare_assets(&self, left: &Asset, right: &Asset) -> Result<Value> {
        let resolve = |a: &Asset| {
            self.projects
                .get(&a.project)
                .ok_or("unknown_project")?
                .snapshot
                .resolve_id(&a.id)
                .map_err(|_| "unknown_asset")
        };
        let a = resolve(left)?;
        let b = resolve(right)?;
        let mut sizes = BTreeMap::new();
        for object in a
            .selected()
            .ok()
            .into_iter()
            .chain(b.selected().ok())
            .chain(
                a.stream_bindings()
                    .iter()
                    .chain(b.stream_bindings())
                    .map(|s| &s.object),
            )
        {
            if sizes
                .insert(&object.sha256, object.bytes)
                .is_some_and(|old| old != object.bytes)
            {
                return Err("conflicting_identity".into());
            }
        }
        let aa: BTreeMap<_, _> = a
            .stream_bindings()
            .iter()
            .map(|s| (s.slot.as_str(), s))
            .collect();
        let bb: BTreeMap<_, _> = b
            .stream_bindings()
            .iter()
            .map(|s| (s.slot.as_str(), s))
            .collect();
        let stream = |s: &git_asset_model::resolve::StreamBinding| json!({"slot":s.slot,"role":s.role,"group":s.group,"object":s.object});
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();
        let mut unchanged = 0;
        for (slot, s) in &aa {
            if let Some(other) = bb.get(slot) {
                if s.object == other.object && s.role == other.role && s.group == other.group {
                    unchanged += 1;
                } else {
                    changed.push(json!({"slot":slot,"before":stream(s),"after":stream(other)}));
                }
            } else {
                removed.push(stream(s));
            }
        }
        for (slot, s) in &bb {
            if !aa.contains_key(slot) {
                added.push(stream(s));
            }
        }
        let fields: BTreeSet<_> = a
            .authored()
            .keys()
            .chain(b.authored().keys())
            .filter(|k| a.authored().get(*k) != b.authored().get(*k))
            .collect();
        let selected_equal = a
            .selected()
            .ok()
            .zip(b.selected().ok())
            .map(|(x, y)| x == y);
        Ok(
            json!({"left":{"selector":left,"record":a.reference(),"definition":a.definition(),"selected":a.selected().ok()},
            "right":{"selector":right,"record":b.reference(),"definition":b.definition(),"selected":b.selected().ok()},
            "record_equal":a.reference().object==b.reference().object,"definition_equal":a.definition()==b.definition(),
            "selected_equal":selected_equal,"authored_changed_fields":fields,"streams":{"added":added,"removed":removed,"changed":changed,"unchanged":unchanged},
            "semantics":"Exact record/definition/selected-object and slot-binding comparison; no decoded media or transitive dependency equivalence."}),
        )
    }
}

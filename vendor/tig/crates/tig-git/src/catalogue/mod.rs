//! Explicit, read-only multi-project application service. No second resolver.
use crate::source::{
    GitId, Limits as GitLimits, Repository, Scope,
    native::{AssetSnapshot, Limits},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tig_assets::{
    graph,
    resolve::{AssetResolver, Resolved},
    schema::Dialect,
};
use tig_core::{LogicalPath, Object, identity::parse_json};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};
pub mod http;
pub mod storage;
pub mod works;

pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolverProfile {
    pub schema: String,
    pub git_executable: PathBuf,
    pub repository: PathBuf,
    pub commit: String,
    pub records: Vec<String>,
    pub definitions: Vec<String>,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub byte_source: Option<ByteProfile>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayRule {
    #[serde(default)]
    pub id_prefix: Option<String>,
    #[serde(default)]
    pub definition: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub category_pointer: Option<String>,
    #[serde(default)]
    pub title_pointers: Vec<String>,
    #[serde(default)]
    pub basename: bool,
    #[serde(default)]
    pub icon: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub label: String,
    pub pointer: String,
    pub section: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub id: String,
    pub label: String,
    pub profile: PathBuf,
    #[serde(default)]
    pub display: Vec<DisplayRule>,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub graph_profiles: Vec<graph::Profile>,
    #[serde(default)]
    pub collections: Vec<works::Collection>,
    #[serde(default)]
    pub storage: Option<storage::Profile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub projects: Vec<Enrollment>,
    #[serde(default)]
    pub default_project: Option<String>,
    #[serde(default)]
    pub storage_domains: Vec<storage::global::Domain>,
}
fn read<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Result<T> {
    if !path.is_absolute() {
        return Err("Enrollment must use an explicit absolute file path".into());
    }
    let mut raw = Vec::new();
    File::open(path)
        .map_err(|_| "Cannot open enrolled metadata")?
        .take(limit + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "Cannot read enrolled metadata")?;
    if raw.len() as u64 > limit {
        return Err("Enrollment byte budget exceeded".into());
    }
    let raw = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&raw);
    serde_json::from_value(parse_json(raw).map_err(|_| "Invalid enrollment JSON")?)
        .map_err(|_| "Unsupported enrollment fields".into())
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn pointer(s: &str) -> bool {
    s.starts_with('/') && s.len() <= 256 && !s.chars().any(char::is_control)
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let config: Self = read(path, 262144)?;
        if config.schema != "tig.catalogue-enrollment.experimental-1"
            || config.projects.is_empty()
            || config.projects.len() > 32
        {
            return Err("Unsupported catalogue enrollment".into());
        }
        let mut ids = BTreeSet::new();
        storage::global::validate(&config.storage_domains)?;
        for p in &config.projects {
            works::validate(&p.collections)?;
            if let Some(s) = &p.storage {
                storage::validate(s)?;
            }
            if !token(&p.id)
                || !ids.insert(&p.id)
                || p.label.is_empty()
                || p.label.len() > 120
                || p.label.chars().any(char::is_control)
                || p.display.len() > 128
                || p.fields.len() > 32
                || p.graph_profiles.len() > 128
            {
                return Err("Invalid or duplicate project enrollment".into());
            }
            for rule in &p.display {
                if rule.title_pointers.len() > 8
                    || rule.title_pointers.iter().any(|p| !pointer(p))
                    || rule.category_pointer.as_ref().is_some_and(|p| !pointer(p))
                    || rule
                        .category
                        .as_ref()
                        .is_some_and(|s| s.is_empty() || s.len() > 120)
                    || rule.id_prefix.as_ref().is_some_and(|s| s.len() > 256)
                    || rule.definition.as_ref().is_some_and(|s| s.len() > 256)
                {
                    return Err("Invalid display admission".into());
                }
            }
            if p.fields.iter().any(|f| {
                !pointer(&f.pointer)
                    || f.label.len() > 120
                    || !matches!(f.section.as_str(), "source" | "delivery")
            }) {
                return Err("Invalid inspector field admission".into());
            }
        }
        if config
            .default_project
            .as_ref()
            .is_some_and(|id| !ids.contains(id))
        {
            return Err("Unknown default project".into());
        }
        Ok(config)
    }
}
fn scope(paths: &[String]) -> Result<Scope> {
    Scope::new(
        paths
            .iter()
            .map(|p| LogicalPath::parse(p))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| "Invalid metadata scope")?,
    )
    .map_err(|_| "Invalid metadata scope".into())
}
fn repo(p: &ResolverProfile) -> Result<Repository> {
    Repository::new(
        p.git_executable.clone(),
        p.repository.clone(),
        GitLimits::default(),
    )
    .map_err(|_| "Enrolled repository unavailable".into())
}
fn value(asset: &Resolved) -> Value {
    json!({"authored":asset.authored(),"status":asset.status()})
}
fn brief(v: &Value, max: usize) -> String {
    let s = if let Some(s) = v.as_str() {
        s.to_owned()
    } else {
        v.to_string()
    };
    s.chars().take(max).collect()
}
#[derive(Clone, Serialize)]
pub struct Card {
    pub project: String,
    pub id: String,
    pub definition: String,
    pub category: String,
    pub title: String,
    pub icon: String,
    pub selected: Option<Object>,
    pub streams: usize,
    pub status: BTreeMap<String, String>,
}
pub struct Project {
    enrollment: Enrollment,
    profile: ResolverProfile,
    snapshot: AssetSnapshot,
    commit: String,
    cards: Vec<Card>,
    refs: Vec<(String, String)>,
    works: Vec<works::Work>,
    storage: Option<std::sync::Arc<storage::Observation>>,
    storage_sources: BTreeMap<String, std::sync::Arc<storage::Observation>>,
    nested: BTreeMap<String, storage::nested::Loaded>,
}
impl Project {
    pub fn open(enrollment: Enrollment) -> Result<Self> {
        let profile: ResolverProfile = read(&enrollment.profile, 65536)?;
        if profile.schema != "tig.asset-resolver-profile.experimental-1" {
            return Err("Unsupported resolver profile".into());
        }
        let repo = repo(&profile)?;
        let refs = repo
            .named_commits()
            .map_err(|_| "Cannot read local references")?
            .into_iter()
            .map(|(n, c)| (n, c.as_str().to_owned()))
            .collect();
        let snapshot = Self::capture(&profile, &profile.commit)?;
        let mut project = Self {
            commit: profile.commit.clone(),
            enrollment,
            profile,
            snapshot,
            cards: Vec::new(),
            refs,
            works: Vec::new(),
            storage: None,
            storage_sources: BTreeMap::new(),
            nested: BTreeMap::new(),
        };
        project.index();
        Ok(project)
    }
    fn capture(profile: &ResolverProfile, commit: &str) -> Result<AssetSnapshot> {
        let repo = repo(profile)?;
        let view = repo
            .commit(GitId::parse(commit).map_err(|_| "Invalid exact commit")?)
            .map_err(|_| "Exact commit unavailable")?;
        let snapshot = repo
            .native(
                &view,
                scope(&profile.records)?,
                scope(&profile.definitions)?,
                profile.limits,
            )
            .map_err(|_| "Snapshot refused: metadata integrity, scope or budget")?;
        snapshot
            .validate_authored_schemas(Dialect::Draft202012)
            .map_err(|_| "Authored schema validation failed")?;
        Ok(snapshot)
    }
    fn index(&mut self) {
        self.cards = self
            .snapshot
            .selected_records()
            .map(|a| {
                let doc = value(a);
                let id = &a.reference().id;
                let rule = self.enrollment.display.iter().find(|r| {
                    r.id_prefix.as_ref().is_none_or(|p| id.starts_with(p))
                        && r.definition
                            .as_ref()
                            .is_none_or(|d| d == &a.definition().id)
                });
                let category = rule
                    .and_then(|r| {
                        r.category.clone().or_else(|| {
                            r.category_pointer
                                .as_ref()
                                .and_then(|p| doc.pointer(p)?.as_str().map(str::to_owned))
                        })
                    })
                    .unwrap_or_else(|| a.definition().id.clone());
                let mut title = rule
                    .and_then(|r| {
                        r.title_pointers.iter().find_map(|p| {
                            doc.pointer(p)?
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(str::to_owned)
                        })
                    })
                    .unwrap_or_else(|| id.clone());
                if rule.is_some_and(|r| r.basename) {
                    title = title
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(&title)
                        .to_owned();
                }
                Card {
                    project: self.enrollment.id.clone(),
                    id: id.clone(),
                    definition: a.definition().id.clone(),
                    category: category.chars().take(120).collect(),
                    title: title.chars().take(240).collect(),
                    icon: rule
                        .and_then(|r| r.icon.clone())
                        .unwrap_or_else(|| "asset".into()),
                    selected: a.selected().ok().cloned(),
                    streams: a.stream_bindings().len(),
                    status: a
                        .status()
                        .iter()
                        .filter(|(_, v)| v.is_string() || v.is_boolean() || v.is_number())
                        .take(16)
                        .map(|(k, v)| (k.clone(), brief(v, 120)))
                        .collect(),
                }
            })
            .collect();
        self.works = works::build(&self.enrollment.collections, &self.snapshot, &self.cards);
    }
    pub fn summary(&self) -> Value {
        let mut categories = BTreeMap::<&str, usize>::new();
        let mut objects = BTreeSet::new();
        let mut logical = 0u64;
        for c in &self.cards {
            *categories.entry(&c.category).or_default() += 1;
            if let Some(o) = &c.selected {
                objects.insert(o.clone());
                logical = logical.saturating_add(o.bytes);
            }
        }
        let mut summary = json!({"id":self.enrollment.id,"label":self.enrollment.label,"commit":self.commit,"checkpoint":self.snapshot.checkpoint(),"records":self.cards.len(),"categories":categories,"selected_records":self.cards.iter().filter(|c|c.selected.is_some()).count(),"unique_selected_objects":objects.len(),"declared_selected_bytes":logical,"unique_selected_bytes":objects.iter().fold(0u64,|n,o|n.saturating_add(o.bytes)),"refs":self.refs,"storage_locations":self.profile.byte_source.as_ref().map_or(0,|p|p.places.len()),"automatic_eviction":false});
        summary["collections"] = works::summaries(&self.enrollment.collections, &self.works);
        summary["storage_available"] = json!(self.enrollment.storage.is_some());
        summary["retained_inventory"] = json!(
            self.enrollment
                .storage
                .as_ref()
                .is_some_and(|s| s.inventory.is_some())
        );
        summary["storage_defaults"] = self.enrollment.storage.as_ref().map_or(
            Value::Null,
            |s| json!({"family":s.default_family,"cache":s.default_cache}),
        );
        summary["work_count"] = json!(
            self.works
                .iter()
                .filter(|w| w.top_level)
                .map(|w| &w.id)
                .collect::<BTreeSet<_>>()
                .len()
        );
        summary
    }
    pub fn change_snapshot(&mut self, commit: &str) -> Result<()> {
        if commit != self.profile.commit && !self.refs.iter().any(|(_, c)| c == commit) {
            return Err("Commit is not in the enrolled local reference list".into());
        }
        let next = Self::capture(&self.profile, commit)?;
        self.snapshot = next;
        self.nested.clear();
        self.commit = commit.into();
        self.index();
        Ok(())
    }
    pub fn check_snapshot(&self, commit: &str) -> Result<()> {
        if self.commit != commit {
            Err("Snapshot changed; refresh the selected project".into())
        } else {
            Ok(())
        }
    }
    pub fn inspect(&self, id: &str) -> Result<Value> {
        let a = self
            .snapshot
            .resolve_id(id)
            .map_err(|_| "Asset is not in the pinned namespace")?;
        let doc = value(&a);
        let fields:Vec<_>=self.enrollment.fields.iter().filter_map(|f|Some(json!({"label":f.label,"section":f.section,"value":brief(doc.pointer(&f.pointer)?,4096)}))).collect();
        let streams: Vec<_> = a
            .stream_bindings()
            .iter()
            .map(|s| json!({"slot":s.slot,"role":s.role,"group":s.group,"object":s.object}))
            .collect();
        Ok(
            json!({"card":self.cards.iter().find(|c|c.id==id),"commit":self.commit,"checkpoint":a.checkpoint(),"record":a.reference(),"definition":a.definition(),"fields":fields,"streams":streams,"dependency_profiles":self.enrollment.graph_profiles.len(),"storage_locations":self.profile.byte_source.as_ref().map_or(0,|p|p.places.len()),"payload_verified":false,"authorizes_eviction":false,"current_remote_content_verified":false}),
        )
    }
    pub fn dependencies(&mut self, id: &str) -> Result<Value> {
        let a = self.snapshot.resolve_id(id).map_err(|_| "Unknown asset")?;
        if self.enrollment.graph_profiles.is_empty() {
            return Ok(
                json!({"supported":false,"message":"No dependency interpretation is enrolled for this project. This is unknown coverage, not zero dependencies."}),
            );
        }
        let query = graph::Query {
            checkpoint: self.snapshot.checkpoint().clone(),
            roots: BTreeSet::from([a.reference().clone()]),
            kinds: BTreeSet::from([
                graph::EdgeKind::Production,
                graph::EdgeKind::Composition,
                graph::EdgeKind::Evidence,
                graph::EdgeKind::Publication,
                graph::EdgeKind::Authority,
            ]),
            limits: graph::Limits {
                nodes: 2048,
                edges: 8192,
                depth: 64,
                bytes: 16 * 1024 * 1024,
            },
        };
        let g = graph::resolve(&mut self.snapshot, &query, &self.enrollment.graph_profiles)
            .map_err(|_| "Dependency inspection refused")?;
        Ok(
            json!({"supported":true,"nodes":g.nodes().values().take(1000).collect::<Vec<_>>(),"edges":g.edges().iter().take(2000).collect::<Vec<_>>(),"gaps":g.gaps(),"truncated":g.nodes().len()>1000||g.edges().len()>2000,"complete_declared_coverage":g.complete_declared_coverage(),"complete_semantic_coverage":false,"authorizes_eviction":false}),
        )
    }
    pub fn verification(&self, id: &str, slot: Option<&str>) -> Result<(Object, ByteProfile)> {
        let a = self.snapshot.resolve_id(id).map_err(|_| "Unknown asset")?;
        let o = if let Some(slot) = slot {
            a.stream(slot)
        } else {
            a.selected()
        }
        .map_err(|_| "Selected bytes unavailable")?
        .clone();
        let p = self
            .profile
            .byte_source
            .clone()
            .ok_or("No byte locations enrolled")?;
        Ok((o, p))
    }
}
pub fn verify(object: Object, profile: ByteProfile) -> Result<Value> {
    let mut cache = ReadCache::new(profile).map_err(|_| "Byte reader enrollment refused")?;
    let held = cache
        .open_verified(&object)
        .map_err(|e| format!("Byte check: {e}; no copies or downloads attempted"))?;
    Ok(
        json!({"object":held.object(),"verified":true,"place_index":held.place_index(),"protection":held.protection(),"independent_custody_verified":false,"authorizes_eviction":false}),
    )
}
pub struct Catalogue {
    pub projects: BTreeMap<String, Project>,
    pub default_project: Option<String>,
    storage_domains: Vec<storage::global::Domain>,
}
impl Catalogue {
    pub fn load(config: Config) -> Result<Self> {
        storage::global::validate(&config.storage_domains)?;
        let mut projects = BTreeMap::new();
        for p in config.projects {
            projects.insert(p.id.clone(), Project::open(p)?);
        }
        Ok(Self {
            projects,
            default_project: config.default_project,
            storage_domains: config.storage_domains,
        })
    }
    pub fn summaries(&self) -> Value {
        json!({"projects":self.projects.values().map(Project::summary).collect::<Vec<_>>(),"default_project":self.default_project,"mode":"local-read-only","automatic_eviction":false})
    }
    pub fn list(
        &self,
        project: &str,
        category: &str,
        search: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        if !(1..=200).contains(&limit)
            || offset > 1_000_000
            || search.len() > 256
            || category.len() > 256
        {
            return Err("List request exceeds budget".into());
        }
        if project != "all" && !self.projects.contains_key(project) {
            return Err("Unknown project".into());
        }
        let q = search.to_lowercase();
        let mut count = 0;
        let mut rows = Vec::new();
        for p in self
            .projects
            .values()
            .filter(|p| project == "all" || p.enrollment.id == project)
        {
            for c in &p.cards {
                if (!category.is_empty() && c.category != category)
                    || (!q.is_empty()
                        && !format!("{} {} {}", c.id, c.title, c.definition)
                            .to_lowercase()
                            .contains(&q))
                {
                    continue;
                }
                if count >= offset && rows.len() < limit {
                    rows.push(c.clone());
                }
                count += 1;
            }
        }
        let snapshots: BTreeMap<_, _> = self
            .projects
            .iter()
            .filter(|(id, _)| project == "all" || id.as_str() == project)
            .map(|(id, p)| (id, &p.commit))
            .collect();
        Ok(json!({"total":count,"offset":offset,"limit":limit,"items":rows,"snapshots":snapshots}))
    }
}

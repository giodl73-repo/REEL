//! Explicit public synthetic fixtures. Never opens existing fixture/provider roots.
#[path = "git-asset-sample/age_preview.rs"]
mod age_preview;
#[path = "git-asset-sample/aged_execution.rs"]
mod aged_execution;
#[path = "git-asset-sample/category_preview.rs"]
mod category_preview;
#[path = "git-asset-sample/git_return.rs"]
mod git_return;
#[path = "git-asset-sample/paged_playback.rs"]
mod paged_playback;
#[path = "git-asset-sample/reacquisition_playback.rs"]
mod reacquisition_playback;
#[path = "git-asset-sample/recovery.rs"]
mod recovery;
#[cfg(feature = "upload")]
#[path = "git-asset-sample/remote_endpoint.rs"]
mod remote_endpoint;
#[cfg(feature = "upload")]
#[path = "git-asset-sample/remote_publication.rs"]
mod remote_publication;
#[path = "git-asset-sample/return_restore.rs"]
mod return_restore;
use git_asset::source::{GitId, Limits, Repository, Scope, native};
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{Tree, TreeLimits},
};
use git_asset_store::{
    Key,
    existing::{ByteSource, Layout, ReadCache},
    final_retirement::{bulk as final_bulk, claims as final_claims, provider as final_provider},
    ledger::CapacityLedger,
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    metadata::{Limits as MetadataLimits, MetadataStore},
    orchestration::Place,
    placement::{self, Authority, Mapping, Provider, Role},
    source_claims::{
        self, Command, Kind, Places,
        admission::{Namespace, Policy, Select},
    },
    store_protection::{Binding, Member},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command as Git, ExitCode},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, Copy)]
enum Mode {
    Ordinary,
    Exchange,
    Worktree,
    BackupOutage,
    StalePlan,
    Rename,
    FinalEnrollment,
    FinalManual,
    FinalGitReturn,
    ReturnRestore,
    RemotePublication,
    FinalCancel,
    FinalReacquire,
    FinalProcess,
    FinalAgePreview,
    FinalAged,
}
struct SampleSeed {
    reader: ReadCache,
    disabled: bool,
    attempts: u64,
}
impl ByteSource for SampleSeed {
    type Reader = <ReadCache as ByteSource>::Reader;
    fn open_verified(
        &mut self,
        object: &Object,
    ) -> git_asset_store::existing::Result<Self::Reader> {
        self.attempts += 1;
        if self.disabled {
            return Err(git_asset_store::existing::Error::Denied);
        }
        self.reader.open_verified(object)
    }
}
const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/catalogue/lifecycle.design-1.json"
));
fn key(s: &str) -> Key {
    Key::parse(s).expect("fixed fixture key")
}
fn hash(v: &impl serde::Serialize) -> Result<Digest> {
    Ok(Digest::blob(&identity::canonical_json(
        &serde_json::to_value(v)?,
    )?))
}
fn save(path: &Path, value: &impl serde::Serialize) -> Result<Object> {
    use std::io::Write;
    let raw = identity::canonical_json(&serde_json::to_value(value)?)?;
    if raw.len() > 65536 {
        return Err("fixture document budget".into());
    }
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(&raw)?;
    file.sync_all()?;
    Ok(Object::from_bytes(&raw))
}
fn git(exe: &Path, repo: &Path, args: &[&str]) -> Result<String> {
    git_transport(exe, repo, args, false)
}
// Only the fixed exchange fixture calls this with file transport enabled.
fn git_transport(exe: &Path, repo: &Path, args: &[&str], file: bool) -> Result<String> {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut command = Git::new(exe);
    for (name, _) in std::env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(name);
        }
    }
    let result = command
        .args([
            "--no-replace-objects",
            "-c",
            "core.hooksPath=NUL",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "tag.gpgSign=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
        ])
        .args([
            "-c",
            &format!("core.attributesFile={null}"),
            "-c",
            &format!("core.excludesFile={null}"),
        ])
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", if file { "file" } else { "" })
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .stdin(std::process::Stdio::null())
        .output()?;
    if !result.status.success() {
        return Err(format!(
            "fixture git failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}
fn git_executable() -> Result<PathBuf> {
    for dir in std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH unavailable")?) {
        let path = dir.join(if cfg!(windows) { "git.exe" } else { "git" });
        if path.is_file() {
            return Ok(fs::canonicalize(path)?);
        }
    }
    Err("Git executable unavailable".into())
}
fn policy() -> Policy {
    let normal = Places {
        current: vec![key("working"), key("backup")],
        pending: vec![key("working"), key("backup")],
        predecessor: vec![key("history"), key("backup")],
    };
    let classes = [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
        .into_iter()
        .map(|kind| (kind, normal.clone()))
        .collect::<BTreeMap<_, _>>();
    Policy {
        prefer_active: true,
        classes,
    }
}
fn limits() -> git_asset_store::protection::Limits {
    git_asset_store::protection::Limits {
        millis: 60000,
        roots: 64,
        bindings: 1024,
        associations: 4096,
        reads: 100000,
        read_bytes: 64 * 1024 * 1024,
    }
}
struct Demo {
    root: PathBuf,
    git: PathBuf,
    repository: Repository,
    metadata: MetadataStore,
    namespaces: Vec<Namespace>,
    stores: Vec<LocalStore>,
    ledgers: Vec<CapacityLedger>,
    config: placement::Config,
    seed: SampleSeed,
    seed_identity: Digest,
    fixture: Value,
    objects: BTreeMap<String, Object>,
    definition: Value,
    reports: Vec<Value>,
    final_roster: Option<final_claims::Roster>,
}
impl Demo {
    fn new(requested: &Path, mode: Mode) -> Result<Self> {
        if !requested.is_absolute() || requested.exists() {
            return Err("sample requires a fresh absolute root".into());
        }
        let parent = fs::canonicalize(requested.parent().ok_or("sample parent missing")?)?;
        if parent.ancestors().any(|p| p.join(".git").exists()) {
            return Err("sample output must be outside a Git checkout".into());
        }
        let root = parent.join(requested.file_name().ok_or("sample root name missing")?);
        fs::create_dir(&root)?;
        let fixture: Value = serde_json::from_str(FIXTURE)?;
        let git_exe = git_executable()?;
        let repo = root.join("project");
        fs::create_dir(&repo)?;
        let template = root.join("git-template");
        fs::create_dir(&template)?;
        let template_arg = format!(
            "--template={}",
            template.to_str().ok_or("sample path must be UTF-8")?
        );
        git(&git_exe, &repo, &["init", &template_arg, "-b", "main"])?;
        git(
            &git_exe,
            &repo,
            &["config", "user.name", "git-asset public sample"],
        )?;
        git(
            &git_exe,
            &repo,
            &["config", "user.email", "sample@example.invalid"],
        )?;
        for name in ["records", "definitions"] {
            fs::create_dir(repo.join(name))?;
        }
        fs::create_dir(root.join("reports"))?;
        fs::create_dir(root.join("lesson-evidence"))?;
        fs::create_dir(root.join("lesson-evidence/objects"))?;
        fs::create_dir(root.join("seed"))?;
        fs::create_dir(root.join("seed/objects"))?;
        let mut objects = BTreeMap::new();
        for name in ["shared", "original", "fixed"] {
            let v = &fixture["objects"][name];
            let raw = v["text"].as_str().ok_or("seed text missing")?.as_bytes();
            let object = Object::from_bytes(raw);
            if object.bytes != v["bytes"].as_u64().ok_or("seed length missing")?
                || object.sha256.as_str() != v["sha256"].as_str().ok_or("seed digest missing")?
            {
                return Err("independent seed pin mismatch".into());
            }
            fs::write(
                Layout::Flat.object_path(&root.join("seed"), &object.sha256),
                raw,
            )?;
            objects.insert(name.into(), object);
        }
        let authored = save(
            &repo.join("definitions/authored.json"),
            &json!({"type":"object","required":["title","version"],"properties":{"title":{"type":"string"},"version":{"type":"string"}},"additionalProperties":false}),
        )?;
        let definition = save(
            &repo.join("definitions/item.json"),
            &json!({"schema":"tig.definition.design-1","id":"type/study","authored_schema":authored,"authoring":{"properties":["title","version"],"required":["title","version"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        )?;
        let mut definition = serde_json::to_value(definition)?;
        definition["id"] = json!("type/study");
        let mut namespace = Namespace::create(
            &root.join("namespace"),
            key("sample"),
            key("sample-epoch"),
            policy(),
        )?;
        let metadata = MetadataStore::create(
            &root.join("metadata"),
            MetadataLimits {
                nodes: 10000,
                bytes: 16 * 1024 * 1024,
                database_bytes: 32 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )?;
        for (name, class) in [
            ("main", Kind::Branch),
            ("draft", Kind::Draft),
            ("job", Kind::Job),
        ] {
            namespace.apply(
                key(&format!("enroll-{name}")),
                Command::Enroll {
                    source: key(name),
                    owner: key("sample"),
                    class,
                },
                &metadata,
                &limits(),
            )?;
        }
        let final_roster = if matches!(
            mode,
            Mode::FinalEnrollment
                | Mode::FinalManual
                | Mode::FinalGitReturn
                | Mode::ReturnRestore
                | Mode::FinalCancel
                | Mode::FinalReacquire
                | Mode::FinalProcess
                | Mode::FinalAgePreview
                | Mode::FinalAged
        ) {
            Some(final_claims::Roster::flat(
                Digest::blob(b"public sample final claim authority"),
                Digest::blob(b"public sample final epoch"),
                Some(Digest::blob(b"complete synthetic sample content scope")),
                vec![final_claims::NamespacePin::current(&namespace)?],
                limits(),
            )?)
        } else {
            None
        };
        let mut stores = Vec::new();
        let mut ledgers = Vec::new();
        let mut providers = Vec::new();
        for (name, role) in [
            ("working", Role::Cache),
            ("history", Role::Cache),
            ("backup", Role::Retained),
        ] {
            let store = if let Some(roster) = &final_roster {
                LocalStore::create_final(
                    &root.join(name),
                    key(name),
                    Binding::new(
                        1,
                        Digest::blob(b"public sample scoped clients"),
                        vec![Member {
                            namespace: namespace.identity().clone(),
                            places: vec![key(name)],
                        }],
                    )?,
                    final_provider::Policy::new(
                        Digest::blob(b"public sample final authority"),
                        Digest::blob(b"public sample final epoch"),
                        roster.scope()?,
                        Digest::blob(b"public sample simulated clock"),
                        vec![key("sample")],
                        60000,
                    )?,
                )?
            } else if role == Role::Cache {
                LocalStore::create_managed(
                    &root.join(name),
                    key(name),
                    Binding::new(
                        1,
                        Digest::blob(b"public sample scoped clients"),
                        vec![Member {
                            namespace: namespace.identity().clone(),
                            places: vec![key(name)],
                        }],
                    )?,
                )?
            } else {
                LocalStore::create(&root.join(name), key(name))?
            };
            let mut ledger = CapacityLedger::create(
                &root.join(format!("ledger-{name}")),
                key(name),
                key("sample-epoch"),
                0,
            )?;
            LocalCapacityProbe::bind(&store, &mut ledger)?.observe(
                &store,
                &mut ledger,
                key("probe"),
                key("sample"),
            )?;
            let job = ledger
                .reserve(
                    key("sample-controller"),
                    key("sample"),
                    if final_roster.is_some() {
                        128 * 1024 * 1024
                    } else {
                        16 * 1024 * 1024
                    },
                )?
                .reference()
                .clone();
            providers.push(Provider {
                place: Place::local(&store, 1)?,
                role,
                // Synthetic admitted domains, as in the provider fixture suite.
                // Reports explicitly deny physical independence qualification.
                failure_domain: key(&format!("fixture-domain-{name}")),
                capacity: ledger.identity().clone(),
                job,
                assignments: vec![Mapping {
                    namespace: namespace.identity().clone(),
                    place: key(name),
                }],
            });
            stores.push(store);
            ledgers.push(ledger);
        }
        let byte_source = json!({"places":[{"root":root.join("seed"),"layout":"flat"}],"required":"cooperative-immutable"});
        let seed_identity = hash(&byte_source)?;
        let config = placement::Config {
            schema: "tig.placement-enrollment.design-1".into(),
            owner: key("sample"),
            authority: Digest::blob(b"explicit public fixture authority"),
            seed: seed_identity.clone(),
            providers,
            namespaces: vec![Authority {
                namespace: namespace.identity().clone(),
                policy: hash(namespace.policy())?,
            }],
            minimum_domains: 1,
            budget: placement::Budget {
                objects: 32,
                max_object_bytes: 1024,
                max_stream_bytes: 1024 * 1024,
                max_peak_bytes: if final_roster.is_some() {
                    64 * 1024 * 1024
                } else {
                    16 * 1024 * 1024
                },
                limits: limits(),
            },
        };
        let repository = Repository::new(git_exe.clone(), repo, Limits::default())?;
        Ok(Self {
            root,
            git: git_exe,
            repository,
            metadata,
            namespaces: vec![namespace],
            stores,
            ledgers,
            config,
            seed: SampleSeed {
                reader: ReadCache::new(serde_json::from_value(byte_source)?)?,
                disabled: false,
                attempts: 0,
            },
            seed_identity,
            fixture,
            objects,
            definition,
            reports: Vec::new(),
            final_roster,
        })
    }
    fn write_version(&self, version: &str) -> Result<()> {
        for (name, title, object) in [
            (
                "series",
                "Series study",
                if version == "v1" { "original" } else { "fixed" },
            ),
            ("reference", "Reference study", "shared"),
        ] {
            let mut streams = Vec::new();
            if name == "series" {
                streams.push(json!({"slot":"reference-grid","role":"input","group":"primary","object":self.objects["shared"]}));
                if version == "v2" {
                    streams.push(json!({"slot":"history/v1","role":"result","group":"history","object":self.objects["original"]}));
                }
            }
            let record = json!({"schema":"tig.instance.design-1","id":format!("work/{name}"),"definition":self.definition,"authored":{"title":title,"version":if name=="reference" { "v1" } else { version }},"selected":self.objects[object],"streams":streams,"status":{"lifecycle":"released"},"metadata":{}});
            fs::write(
                self.root.join(format!("project/records/{name}.asset")),
                identity::canonical_json(&record)?,
            )?;
        }
        Ok(())
    }
    fn commit(&self, message: &str) -> Result<String> {
        let repo = self.root.join("project");
        git(&self.git, &repo, &["add", "records", "definitions"])?;
        git(&self.git, &repo, &["commit", "-m", message])?;
        git(&self.git, &repo, &["rev-parse", "HEAD"])
    }
    fn select(&mut self, source: &str, commit: &str, operation: &str) -> Result<()> {
        let view = self.repository.commit(GitId::parse(commit)?)?;
        let records = Scope::new([LogicalPath::parse("records")?])?;
        let definitions = Scope::new([LogicalPath::parse("definitions")?])?;
        let snapshot =
            self.repository
                .native(&view, records, definitions, native::Limits::default())?;
        self.select_snapshot(source, &snapshot, operation)
    }
    fn flat_entries(
        snapshot: &native::AssetSnapshot,
        assets: Option<&BTreeSet<git_asset_model::graph::Reference>>,
    ) -> Result<BTreeMap<LogicalPath, Object>> {
        snapshot.validate_authored_schemas(git_asset_model::schema::Dialect::Draft202012)?;
        let mut entries = BTreeMap::new();
        for record in snapshot.selected_records() {
            if assets.is_some_and(|set| !set.contains(record.reference())) {
                continue;
            }
            entries.insert(
                LogicalPath::parse(&format!("{}/selected", record.reference().id))?,
                record.selected()?.clone(),
            );
            for stream in record.stream_bindings() {
                // The admitted public fixture distinguishes primary inputs from
                // history; this is not a general native closure implementation.
                if stream.group == "history" && stream.role == "result" {
                    continue;
                }
                if stream.group != "primary" || stream.role != "input" {
                    return Err("unsupported fixture stream meaning".into());
                }
                entries.insert(
                    LogicalPath::parse(&format!("{}/{}", record.reference().id, stream.slot))?,
                    stream.object.clone(),
                );
            }
        }
        Ok(entries)
    }
    fn select_snapshot(
        &mut self,
        source: &str,
        snapshot: &native::AssetSnapshot,
        operation: &str,
    ) -> Result<()> {
        let entries = Self::flat_entries(snapshot, None)?;
        let root = Tree::build(&entries, &mut self.metadata, TreeLimits::default())?
            .0
            .root()
            .clone();
        self.metadata.finish()?;
        let revision = self.namespaces[0]
            .source(&key(source))?
            .ok_or("source missing")?
            .revision;
        self.namespaces[0].select(
            &Select {
                prepare: key(&format!("{operation}-prepare")),
                adopt: key(&format!("{operation}-adopt")),
                source: key(source),
                owner: key("sample"),
                expected_revision: revision,
                selection: source_claims::Selection {
                    root,
                    context: snapshot.checkpoint().clone(),
                },
            },
            &self.metadata.finished_reader()?,
            &self.config.budget.limits,
        )?;
        Ok(())
    }
    fn withdraw(&mut self, source: &str, operation: &str) -> Result<()> {
        let revision = self.namespaces[0]
            .source(&key(source))?
            .ok_or("source missing")?
            .revision;
        self.namespaces[0].apply(
            key(operation),
            Command::Withdraw {
                source: key(source),
                owner: key("sample"),
                expected_revision: revision,
            },
            &self.metadata.finished_reader()?,
            &self.config.budget.limits,
        )?;
        Ok(())
    }
    fn with_placement<T>(
        &mut self,
        action: impl FnOnce(
            &mut placement::Execution<'_, '_, SampleSeed, git_asset_store::metadata::MetadataReader>,
        ) -> placement::Result<T>,
    ) -> Result<T> {
        let mut places = self
            .stores
            .iter()
            .zip(self.ledgers.iter_mut())
            .zip(&self.config.providers)
            .map(|((store, ledger), pin)| placement::Local {
                store,
                ledger,
                job: &pin.job,
            })
            .collect::<Vec<_>>();
        let mut reader = self.metadata.finished_reader()?;
        let mut executor = placement::Execution {
            config: &self.config,
            namespaces: &mut self.namespaces,
            nodes: &mut reader,
            places: &mut places,
            seed: &mut self.seed,
            seed_identity: &self.seed_identity,
            allow_copy: true,
            allow_eviction: true,
            allow_reacquisition: true,
            allow_cancellation: true,
        };
        Ok(action(&mut executor)?)
    }
    fn checkpoint(&mut self, index: usize, commit: &str) -> Result<()> {
        self.checkpoint_coverage(index, commit, false, false)
            .map(|_| ())
    }
    fn checkpoint_coverage(
        &mut self,
        index: usize,
        commit: &str,
        offline: bool,
        held: bool,
    ) -> Result<Digest> {
        let (plan, receipt) = self.with_placement(|executor| {
            let plan = executor.plan(None)?;
            let receipt = executor.apply(&plan)?;
            Ok((plan, receipt))
        })?;
        if (held
            && (receipt.page_converged
                || receipt.storage_converged != Some(false)
                || receipt.holds.is_empty()))
            || (!held
                && (!receipt.page_converged || receipt.next.is_some() || !receipt.holds.is_empty()))
        {
            return Err(format!("stage {index}: unresolved placement: {:?}", receipt.holds).into());
        }
        self.record_checkpoint(index, commit, &plan, &receipt, offline, None)
    }
    fn record_checkpoint(
        &mut self,
        index: usize,
        commit: &str,
        plan: &placement::Plan,
        receipt: &placement::Report,
        offline: bool,
        rejection: Option<Value>,
    ) -> Result<Digest> {
        let stage = &self.fixture["stages"][index];
        let id = stage["id"].as_str().ok_or("stage missing")?.to_owned();
        let mut observations = BTreeMap::new();
        for (name, object) in &self.objects {
            let mut actual = BTreeSet::new();
            let mut unknown = BTreeSet::new();
            for (role, store) in ["working", "history", "backup"]
                .into_iter()
                .zip(&self.stores)
            {
                if offline && role == "backup" {
                    if self.root.join("backup").exists() {
                        return Err("offline backup unexpectedly accessible".into());
                    }
                    unknown.insert(role.to_owned());
                    continue;
                }
                Place::local(store, 1)?;
                let path = Layout::Flat.object_path(&self.root.join(role), &object.sha256);
                match fs::symlink_metadata(&path) {
                    Ok(_) => {
                        store.read_object(object)?;
                        actual.insert(role.to_owned());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
            }
            let expected = stage["expected_copies"][name]
                .as_array()
                .ok_or("expected copies missing")?
                .iter()
                .map(|v| v.as_str().ok_or("expected role missing").map(str::to_owned))
                .collect::<std::result::Result<BTreeSet<_>, _>>()?;
            if actual != expected {
                return Err(
                    format!("{id}/{name}: expected {expected:?}, observed {actual:?}").into(),
                );
            }
            let expected_unknown = stage
                .get("unknown_copies")
                .and_then(|v| v.get(name))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|s| s.as_str().ok_or("unknown role missing").map(str::to_owned))
                        .collect::<std::result::Result<BTreeSet<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            if unknown != expected_unknown {
                return Err("unexpected unknown coverage".into());
            }
            let mut observation = json!({"object":object,"verified_copies":actual});
            if offline || rejection.is_some() {
                observation["unknown_copies"] = json!(unknown);
            }
            observations.insert(name, observation);
        }
        let plan_id = plan.id()?;
        let mut report = json!({"schema":if rejection.is_some(){"tig.teaching-observation.design-3"}else if offline {"tig.teaching-observation.design-2"}else{"tig.teaching-observation.design-1"},"lesson":id,"source_commit":commit,"source_checkpoint":self.namespaces[0].checkpoint(),"plan":plan,"receipt":receipt,"observations":observations,"declared_fixture_domains":3,"physical_domain_coverage":"not-established; portable fixture directories","independent_hardware_backup_proven":false});
        if let Some(rejected) = rejection {
            report["rejection"] = rejected;
        }
        let object = save(&self.root.join(format!("reports/{id}.json")), &report)?;
        fs::copy(
            self.root.join(format!("reports/{id}.json")),
            Layout::Flat.object_path(&self.root.join("lesson-evidence"), &object.sha256),
        )?;
        self.reports
            .push(json!({"lesson":id,"report":object,"file":format!("reports/{id}.json"),"title":id.replace('-'," "),"event":stage["event"]}));
        git(
            &self.git,
            &self.root.join("project"),
            &["tag", &format!("lesson-{id}"), commit],
        )?;
        Ok(plan_id)
    }
    fn catalogue(&self, current: &str, trace: &Object) -> Result<()> {
        let profile = self.root.join("resolver.json");
        save(
            &profile,
            &json!({"schema":"tig.asset-resolver-profile.experimental-1","git_executable":self.git,"repository":self.root.join("project"),"commit":current,"records":["records"],"definitions":["definitions"],"byte_source":{"places":[{"root":self.root.join("working"),"layout":"flat"},{"root":self.root.join("history"),"layout":"flat"},{"root":self.root.join("backup"),"layout":"flat"}],"required":"cooperative-immutable"}}),
        )?;
        let enrollment = self.root.join("catalogue.json");
        save(
            &enrollment,
            &json!({"schema":"tig.catalogue-enrollment.experimental-1","teaching":{"schema":"tig.catalogue-teaching-profile.design-1","project":"sample","trace":trace,"roles":[{"id":"working","label":"Working / F"},{"id":"history","label":"History / D"},{"id":"backup","label":"Backup / M"}],"source":{"places":[{"root":self.root.join("lesson-evidence"),"layout":"flat"}],"required":"cooperative-immutable","limits":{"object_bytes":65536,"total_bytes":2097152,"requests":if self.reports.iter().any(|r|r["lesson"]=="aged-category-final-loss"){1024}else{32},"verify_millis":30000,"live_readers":1}}},"default_project":"sample","projects":[{"id":"sample","label":"git-asset public teaching sample","profile":profile,"display":[{"category":"Studies","title_pointers":["/authored/title"]}],"fields":[{"label":"Version","pointer":"/authored/version","section":"source"}],"collections":[{"id":"studies","label":"Studies","icon":"folder","roots":[{"pattern":"^work/(.+)$","key":"${1}"}],"stages":[{"label":"Released","pointer":"/status/lifecycle","pattern":"^released$"}],"count_state":"Released","version_pointer":"/authored/version","version_role":"result","version_slot_prefix":"history/"}],"storage":{"schema":"tig.catalogue-storage.experimental-1","inventory":null,"encoding":"json","max_decoded_bytes":1048576,"places":[{"label":"Working (F)","place_index":0,"report_tier":null,"live_probe":true},{"label":"History (D)","place_index":1,"report_tier":null,"live_probe":true},{"label":"Backup (M)","place_index":2,"report_tier":null,"live_probe":true}],"default_family":"Studies","default_cache":"Working (F)"}}]}),
        )?;
        let mut catalogue = git_asset::catalogue::Catalogue::load(
            git_asset::catalogue::Config::load(&enrollment)?,
        )?;
        git_asset::catalogue::teaching::execute(catalogue.teaching_request()?)?;
        catalogue.observe_registered("sample", current, "")?;
        let inventory =
            catalogue.object_inventory(&git_asset::catalogue::storage::objects::Query {
                project: "sample",
                kind: "",
                usage: "",
                anomaly: "",
                primary: "",
                observed: "",
                q: "",
                sort: "bytes",
                offset: 0,
                sha: "",
            })?;
        save(&self.root.join("catalogue-observations.json"), &inventory)?;
        Ok(())
    }
    fn run(mut self, mode: Mode) -> Result<Value> {
        self.write_version("v1")?;
        let first = self.commit("Sample: release v1")?;
        self.checkpoint(0, &first)?;
        self.select("main", &first, "release-v1")?;
        self.checkpoint(1, &first)?;
        self.select("job", &first, "start-old-job")?;
        git(
            &self.git,
            &self.root.join("project"),
            &["switch", "-c", "fix-series"],
        )?;
        self.write_version("v2")?;
        let second = self.commit("Sample: fix the measurement series")?;
        self.select("draft", &second, "stage-v2")?;
        self.checkpoint(2, &second)?;
        git(&self.git, &self.root.join("project"), &["switch", "main"])?;
        git(
            &self.git,
            &self.root.join("project"),
            &[
                "merge",
                "--no-ff",
                "fix-series",
                "-m",
                "Sample: release fixed version",
            ],
        )?;
        let merged = git(
            &self.git,
            &self.root.join("project"),
            &["rev-parse", "HEAD"],
        )?;
        self.select("main", &merged, "merge-v2")?;
        // End the merged branch before releasing its draft obligation. Otherwise
        // the later revert would still have another live branch using v2.
        git(
            &self.git,
            &self.root.join("project"),
            &["branch", "-d", "fix-series"],
        )?;
        self.withdraw("draft", "settle-draft")?;
        let mut stale_evidence = None;
        let backup_evidence = if matches!(mode, Mode::BackupOutage) {
            Some(self.backup_outage(&merged)?)
        } else {
            self.checkpoint(3, &merged)?;
            self.withdraw("job", "finish-job")?;
            if matches!(mode, Mode::StalePlan) {
                stale_evidence = Some(self.stale_plan(&merged, &first)?);
            }
            None
        };
        self.checkpoint(4, &merged)?;
        git(
            &self.git,
            &self.root.join("project"),
            &["revert", "-m", "1", "--no-edit", &merged],
        )?;
        let reverted = git(
            &self.git,
            &self.root.join("project"),
            &["rev-parse", "HEAD"],
        )?;
        self.select("main", &reverted, "revert-v1")?;
        self.checkpoint(5, &reverted)?;
        git(
            &self.git,
            &self.root.join("project"),
            &["revert", "--no-edit", &reverted],
        )?;
        let mut current = git(
            &self.git,
            &self.root.join("project"),
            &["rev-parse", "HEAD"],
        )?;
        self.select("main", &current, "return-v2")?;
        self.checkpoint(6, &current)?;
        let rename_evidence = if matches!(mode, Mode::Rename) {
            let (head, evidence) = self.rename(&current)?;
            current = head;
            Some(evidence)
        } else {
            None
        };
        let exchange_evidence = if matches!(mode, Mode::Exchange | Mode::RemotePublication) {
            let (head, evidence) = self.exchange(&current)?;
            current = head;
            Some(evidence)
        } else {
            None
        };
        let worktree_evidence = if matches!(mode, Mode::Worktree) {
            Some(self.worktree(&current, &first)?)
        } else {
            None
        };
        let mut result = json!({"schema":"tig.sample-result.design-1","root":self.root,"completed_checkpoints":self.reports.len(),"trace":"trace.json","catalogue":"catalogue.json","complete_course":false});
        if let Some(evidence) = exchange_evidence {
            result["exchange_evidence"] = serde_json::to_value(evidence)?;
        }
        if let Some(evidence) = worktree_evidence {
            result["worktree_evidence"] = serde_json::to_value(evidence)?;
        }
        if let Some(evidence) = backup_evidence {
            result["backup_evidence"] = serde_json::to_value(evidence)?;
        }
        if let Some(evidence) = stale_evidence {
            result["stale_evidence"] = serde_json::to_value(evidence)?;
        }
        if let Some(evidence) = rename_evidence {
            result["rename_evidence"] = serde_json::to_value(evidence)?;
        }
        if matches!(mode, Mode::FinalAged) {
            let evidence = aged_execution::run(&mut self, &first, &reverted)?;
            result["aged_execution_evidence"] = serde_json::to_value(evidence)?;
            result["final_deletion_exercised"] = true.into();
        } else if let Some(roster) = self.final_roster.clone() {
            if matches!(mode, Mode::FinalAgePreview) {
                let before = self.copy_snapshot()?;
                let view = self.repository.commit(GitId::parse(&first)?)?;
                let mut native = self.repository.native(
                    &view,
                    Scope::new([LogicalPath::parse("records")?])?,
                    Scope::new([LogicalPath::parse("definitions")?])?,
                    native::Limits::default(),
                )?;
                let category = category_preview::run(&self.root, &mut native, &mut self.metadata)?;
                result["native_category_evidence"] = serde_json::to_value(category)?;
                let evidence =
                    age_preview::run(&self.root, &mut self.namespaces[0], &mut self.metadata)?;
                if self.copy_snapshot()? != before {
                    return Err("age capture changed payload copies".into());
                }
                result["retained_age_evidence"] = serde_json::to_value(evidence)?;
            }
            let observed = self.copy_snapshot()?;
            let mut copies = Vec::new();
            for ((store, ledger), pin) in self
                .stores
                .iter()
                .zip(&self.ledgers)
                .zip(&self.config.providers)
            {
                for (name, object) in &self.objects {
                    if observed[pin.assignments[0].place.as_str()]
                        .get(name)
                        .is_some()
                    {
                        copies.push(git_asset_store::final_retirement::bulk::Copy {
                            store: store.identity().clone(),
                            capacity: ledger.identity().clone(),
                            publication: placement::publication(&self.config, pin, store, object)?,
                        });
                    }
                }
            }
            copies.sort_by(|a, b| {
                (&a.store, &a.publication.object.sha256)
                    .cmp(&(&b.store, &b.publication.object.sha256))
            });
            let (preview, loss) = self.final_preview(&roster, copies.clone(), mode)?;
            let evidence = save(
                &self.root.join("final-enrollment.json"),
                &json!({
                    "schema":"tig.sample-final-enrollment.design-1","roster":roster,
                    "scope":roster.scope()?,"providers":self.config.providers,"copies":copies,
                    "enrollment_only":true,"final_deletion_exercised":false,
                    "independent_hardware_backup_proven":false
                }),
            )?;
            result["final_enrollment_evidence"] = serde_json::to_value(evidence)?;
            result["final_preview_evidence"] = serde_json::to_value(preview)?;
            result["final_deletion_exercised"] = false.into();
            if let Some(loss) = loss {
                result["final_loss_evidence"] = serde_json::to_value(loss)?;
                result["final_deletion_exercised"] = true.into();
            }
        }
        if matches!(mode, Mode::FinalGitReturn | Mode::ReturnRestore) {
            let (head, evidence) = git_return::run(&mut self, &first, &current)?;
            current = head;
            result["git_return_evidence"] = serde_json::to_value(evidence)?;
        }
        if matches!(mode, Mode::ReturnRestore) {
            result["joined_restoration_evidence"] = return_restore::run(&mut self, &current)?;
        }
        #[cfg(feature = "upload")]
        if matches!(mode, Mode::RemotePublication) {
            result["remote_publication_evidence"] = remote_publication::run(&mut self, &current)?;
        }
        let pending = if matches!(mode, Mode::Exchange | Mode::RemotePublication) {
            json!([
                "full command-by-command Git curriculum",
                "remote byte publication and offline backup",
                "failure rehearsals",
                "final retirement",
                "metadata lifetime"
            ])
        } else {
            json!([
                "full command-by-command Git curriculum",
                "publish/fetch/pull",
                "failure rehearsals",
                "final retirement",
                "metadata lifetime"
            ])
        };
        let trace = save(
            &self.root.join("trace.json"),
            &json!({"schema":"tig.teaching-trace.design-2","providers":self.config.providers.iter().map(|p|json!({"instance":p.place.instance,"id":p.assignments[0].place.as_str()})).collect::<Vec<_>>(),"completed":self.reports,"pending":pending,"fixture_native_scope":"selected objects and fixture primary inputs; historical streams retain historical source obligations"}),
        )?;
        fs::copy(
            self.root.join("trace.json"),
            Layout::Flat.object_path(&self.root.join("lesson-evidence"), &trace.sha256),
        )?;
        self.catalogue(&current, &trace)?;
        result["completed_checkpoints"] = self.reports.len().into();
        Ok(result)
    }

    fn final_preview(
        &mut self,
        roster: &final_claims::Roster,
        copies: Vec<final_bulk::Copy>,
        mode: Mode,
    ) -> Result<(Object, Option<Object>)> {
        let before = self.copy_snapshot()?;
        let checkpoint = self.namespaces[0].checkpoint();
        let witness = LocalStore::create(&self.root.join("final-witness"), key("final-witness"))?;
        let mut ledger = CapacityLedger::create(
            &self.root.join("final-witness-ledger"),
            key("final-witness-capacity"),
            key("sample-epoch"),
            0,
        )?;
        LocalCapacityProbe::bind(&witness, &mut ledger)?.observe(
            &witness,
            &mut ledger,
            key("probe"),
            key("sample"),
        )?;
        let witness_job = ledger
            .reserve(key("final-witness-job"), key("sample"), 64 * 1024 * 1024)?
            .reference()
            .clone();
        let enrollment = final_bulk::Enrollment {
            authority: Digest::blob(b"public sample batch authority"),
            epoch: Digest::blob(b"public sample final epoch"),
            owner: key("sample"),
            context: Digest::blob(b"public sample final context"),
            scope: roster.scope()?,
            clock: Digest::blob(b"public sample simulated clock"),
            witness: Place::local(&witness, 1)?,
            capacity: ledger.identity().clone(),
            max_window: 60000,
            max_metadata_bytes: 32 * 1024 * 1024,
            sources: vec![source_claims::subset::Enrollment {
                authority: Digest::blob(b"public sample subset authority"),
                namespace: self.namespaces[0].identity().clone(),
                policy: hash(self.namespaces[0].policy())?,
                owner: key("sample"),
                classes: vec![Kind::Branch, Kind::Draft],
                limits: limits(),
                writes: 10000,
                write_bytes: 8 * 1024 * 1024,
            }],
        };
        let mut selections = Vec::new();
        for name in ["draft", "main"] {
            let source = self.namespaces[0]
                .source(&key(name))?
                .ok_or("missing sample source")?;
            for version in source.predecessors {
                let (entries, _) = Tree::from_root(version.selection.root)
                    .entries(&mut self.metadata.finished_reader()?, TreeLimits::default())?;
                selections.push(final_bulk::Selection {
                    namespace: self.namespaces[0].identity().clone(),
                    request: source_claims::subset::Request {
                        source: key(name),
                        predecessor_revision: version.revision,
                        paths: entries.into_keys().collect(),
                    },
                });
            }
        }
        selections.sort_by(|a, b| {
            (
                &a.namespace,
                a.request.source.as_str(),
                a.request.predecessor_revision,
            )
                .cmp(&(
                    &b.namespace,
                    b.request.source.as_str(),
                    b.request.predecessor_revision,
                ))
        });
        let request = final_bulk::Request {
            operation: key("public-final-preview"),
            selections,
            copies,
            window: final_provider::Window {
                not_before: 50,
                expires: 500,
            },
        };
        let stores = self.stores.iter().collect::<Vec<_>>();
        let plan = final_bulk::plan(
            &enrollment,
            request,
            roster,
            &mut self.namespaces,
            &self.metadata.finished_reader()?,
            &stores,
        )?;
        if self.copy_snapshot()? != before || self.namespaces[0].checkpoint() != checkpoint {
            return Err("final preview changed source or copies".into());
        }
        let preview = save(
            &self.root.join("final-preview.json"),
            &json!({
                "schema":"tig.sample-final-preview.design-1","plan":serde_json::from_slice::<Value>(&plan.to_json()?)?,"plan_id":plan.id()?,
                "witness_job":witness_job,"source_checkpoint":checkpoint,"copies_before":before,"copies_after":self.copy_snapshot()?,
                "simulated_clock":true,"preview_only":true,"final_deletion_exercised":false,"removal_authority":false,
                "scope_limit":"exact manual historical block; category and retained age selection pending"
            }),
        )?;
        if matches!(mode, Mode::FinalEnrollment | Mode::FinalAgePreview) {
            return Ok((preview, None));
        }
        if plan
            .subjects()
            .iter()
            .any(|s| s.hold != Some(final_bulk::Hold::Protected))
            || plan.actions().len() != 0
        {
            return Err("initial final preview lost protection".into());
        }
        let job = self.namespaces[0]
            .source(&key("job"))?
            .ok_or("missing job")?;
        if job.current.is_some() || job.pending.is_some() {
            return Err("job still active".into());
        }
        for version in job.predecessors {
            let revision = self.namespaces[0]
                .source(&key("job"))?
                .ok_or("missing job")?
                .revision;
            self.namespaces[0].apply(
                key(&format!("explicit-retire-job-history-{}", version.revision)),
                Command::Retire {
                    source: key("job"),
                    owner: key("sample"),
                    expected_revision: revision,
                    predecessor_revision: version.revision,
                },
                &self.metadata.finished_reader()?,
                &limits(),
            )?;
        }
        let settled_job = self.namespaces[0].checkpoint();
        if self.copy_snapshot()? != before {
            return Err("job history retirement changed bytes".into());
        }
        let mut request = plan.request().clone();
        request.operation = key("public-explicit-final-loss");
        let fresh = final_bulk::plan(
            &enrollment,
            request,
            roster,
            &mut self.namespaces,
            &self.metadata.finished_reader()?,
            &stores,
        )?;
        if fresh
            .subjects()
            .iter()
            .filter(|s| s.hold.is_none())
            .map(|s| &s.object)
            .collect::<Vec<_>>()
            != vec![&self.objects["original"]]
            || fresh.actions().len() != 2
        {
            return Err("unexpected final eligibility".into());
        }
        struct SimulatedClock(Digest, u64);
        impl final_provider::Clock for SimulatedClock {
            fn now(&self, policy: &Digest) -> final_provider::Result<u64> {
                if policy != &self.0 {
                    return Err(final_provider::Error::Clock);
                }
                Ok(self.1)
            }
        }
        let clock = SimulatedClock(enrollment.clock.clone(), 100);
        let jobs = fresh
            .actions()
            .map(|(store, _)| {
                self.config
                    .providers
                    .iter()
                    .find(|p| &p.place.instance == store)
                    .map(|p| p.job.clone())
                    .ok_or("unknown action store")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let auth = fresh.authorize()?; // explicit owner decision in this fresh synthetic mode only
        let process_exit = if matches!(mode, Mode::FinalProcess) {
            let frame = recovery::Frame {
                root: self.root.clone(),
                plan: serde_json::from_slice(&fresh.to_json()?)?,
                enrollment: enrollment.clone(),
                roster: roster.clone(),
                providers: self.config.providers.clone(),
                namespace: settled_job.clone(),
                metadata: self.metadata.checkpoint(),
                ledgers: self
                    .ledgers
                    .iter()
                    .map(CapacityLedger::checkpoint)
                    .collect(),
                witness: ledger.checkpoint(),
                witness_job: witness_job.clone(),
            };
            let raw = identity::canonical_json(&serde_json::to_value(&frame)?)?;
            if raw.len() > 65536 {
                return Err("fixture frame budget exceeded".into());
            }
            save(
                &self.root.join("final-child-frame.json"),
                &serde_json::to_value(&frame)?,
            )?;
            use std::io::Write;
            let mut child = Git::new(std::env::current_exe()?)
                .args(["__final-fixture-child", Digest::blob(&raw).as_str()])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;
            let write = child
                .stdin
                .take()
                .ok_or("missing child stdin")?
                .write_all(&raw);
            if let Err(error) = write {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
            let output = child.wait_with_output()?;
            if output.status.code() != Some(94)
                || !output.stdout.is_empty()
                || !output.stderr.is_empty()
            {
                return Err("fixture child did not exit at durable intent".into());
            }
            let reopened = Namespace::open(&self.root.join("namespace"), &settled_job)?;
            if self.copy_snapshot()? != before || reopened.checkpoint() != settled_job {
                return Err("child intent changed claims or copies".into());
            }
            self.namespaces[0] = reopened;
            self.metadata = MetadataStore::open(&self.root.join("metadata"), &frame.metadata)?;
            for (at, role) in ["working", "history", "backup"].into_iter().enumerate() {
                self.stores[at] = LocalStore::open(&self.root.join(role))?;
                self.ledgers[at] = CapacityLedger::open(
                    &self.root.join(format!("ledger-{role}")),
                    &frame.ledgers[at],
                )?;
            }
            ledger = CapacityLedger::open(&self.root.join("final-witness-ledger"), &frame.witness)?;
            Some(94)
        } else {
            None
        };
        let (report, retry, copies_after_intent, copies_after_completion) = {
            let mut places = self
                .stores
                .iter()
                .zip(&mut self.ledgers)
                .zip(&self.config.providers)
                .map(|((store, ledger), p)| placement::Local {
                    store,
                    ledger,
                    job: &p.job,
                })
                .collect::<Vec<_>>();
            let mut execution = final_bulk::Execution {
                enrollment: &enrollment,
                roster,
                namespaces: &mut self.namespaces,
                metadata: &mut self.metadata,
                places: &mut places,
                action_jobs: &jobs,
                witness: placement::Local {
                    store: &witness,
                    ledger: &mut ledger,
                    job: &witness_job,
                },
                clock: &clock,
                admitted_context: &enrollment.context,
            };
            if process_exit.is_none() {
                let interrupted = execution.apply_hook(&fresh, &auth, |stage| {
                    if stage == final_bulk::Stage::Intent {
                        Err(final_bulk::Error::Injected)
                    } else {
                        Ok(())
                    }
                });
                if !matches!(interrupted, Err(final_bulk::Error::Injected)) {
                    return Err("intent interruption was not observed".into());
                }
            }
            let copies_after_intent = Self::snapshot_at(&self.root, &self.objects)?;
            if copies_after_intent != before || execution.namespaces[0].checkpoint() != settled_job
            {
                return Err("intent interruption changed claims or copies".into());
            }
            let cancelled = matches!(mode, Mode::FinalCancel);
            let report = if cancelled {
                let partial = execution.apply_hook(&fresh, &auth, |stage| {
                    if stage == final_bulk::Stage::Removed(0) {
                        Err(final_bulk::Error::Injected)
                    } else {
                        Ok(())
                    }
                });
                if !matches!(partial, Err(final_bulk::Error::Injected)) {
                    return Err("first removal interruption missing".into());
                }
                execution.cancel(&fresh, &auth)?
            } else {
                execution.apply(&fresh, &auth)?
            };
            let copies_after_completion = Self::snapshot_at(&self.root, &self.objects)?;
            let retry = execution.apply(&fresh, &auth)?;
            if retry.phase
                != if cancelled {
                    final_bulk::Phase::Cancelled
                } else {
                    final_bulk::Phase::Completed
                }
                || retry.new_logical_copy_bytes_removed != 0
                || retry.historical_deleted_copy_bytes != if cancelled { 29 } else { 58 }
                || Self::snapshot_at(&self.root, &self.objects)? != copies_after_completion
            {
                return Err("exact retry repeated final loss".into());
            }
            (report, retry, copies_after_intent, copies_after_completion)
        };
        let after = self.copy_snapshot()?;
        let cancelled = matches!(mode, Mode::FinalCancel);
        let remaining = ["working", "history", "backup"]
            .iter()
            .filter(|role| after[**role].get("original").is_some())
            .count();
        if remaining != usize::from(cancelled) {
            return Err("unexpected original copies after final operation".into());
        }
        for name in ["fixed", "shared"] {
            for role in ["working", "backup"] {
                if after[role][name] != before[role][name] {
                    return Err("final loss harmed protected bytes".into());
                }
            }
        }
        if report.phase
            != if cancelled {
                final_bulk::Phase::Cancelled
            } else {
                final_bulk::Phase::Completed
            }
            || !report.holds.is_empty()
            || report.new_logical_copy_bytes_removed != if cancelled { 0 } else { 58 }
        {
            return Err("final report did not complete exact two-copy loss".into());
        }
        let final_source = self.namespaces[0].checkpoint();
        let mut reacquisition_observation = None;
        let reacquisition = if matches!(mode, Mode::FinalReacquire) {
            let acquisition_commit = git(
                &self.git,
                &self.root.join("project"),
                &["rev-parse", "HEAD"],
            )?;
            let at = self
                .config
                .providers
                .iter()
                .position(|p| p.assignments[0].place == key("backup"))
                .ok_or("backup missing")?;
            let (_, parent) = fresh
                .actions()
                .find(|(id, _)| *id == self.stores[at].identity())
                .ok_or("backup action missing")?;
            let publication = git_asset_store::Request {
                operation: key("fresh-same-original"),
                ..parent.request().publication.clone()
            };
            let context = Digest::blob(b"public sample explicit reacquisition");
            let rp = final_provider::reacquire::plan(
                &self.stores[at],
                parent,
                final_provider::reacquire::Request {
                    publication: publication.clone(),
                    capacity: self.ledgers[at].identity().clone(),
                    context: context.clone(),
                    window: final_provider::Window {
                        not_before: 50,
                        expires: 500,
                    },
                },
            )?;
            let bytes = self.fixture["objects"]["original"]["text"]
                .as_str()
                .ok_or("original seed missing")?
                .as_bytes();
            let mut input = std::io::Cursor::new(bytes);
            let publication_receipt = final_provider::reacquire::Execution {
                store: &self.stores[at],
                ledger: &mut self.ledgers[at],
                job: &self.config.providers[at].job,
                clock: &clock,
                admitted_context: &context,
            }
            .apply(&rp, &rp.authorize()?, &mut input)?;
            let publication_observation =
                reacquisition_playback::publication(&publication_receipt, input.position())?;
            if input.position() != publication_receipt.publication().request().object.bytes {
                return Err("fresh acquisition did not read exact original bytes".into());
            }
            let retained = git_asset_store::retention::plan(
                &self.stores[at],
                &git_asset_store::retention::RetentionRequest {
                    operation: key("fresh-original-retention"),
                    owner: key("sample"),
                    publication,
                    minimum_ack: git_asset_store::AckLevel::ProcessRestart,
                },
            )?;
            let grant = git_asset_store::retention::retain(
                &mut self.ledgers[at],
                &self.stores[at],
                &self.config.providers[at].job,
                &retained,
            )?
            .grant;
            let acquired = self.copy_snapshot()?;
            let prior_ledgers = self
                .ledgers
                .iter()
                .map(CapacityLedger::checkpoint)
                .collect::<Vec<_>>();
            let prior_witness = ledger.checkpoint();
            let prior_source = self.namespaces[0].checkpoint();
            let expired_clock = SimulatedClock(enrollment.clock.clone(), 700);
            let replay = {
                let mut places = self
                    .stores
                    .iter()
                    .zip(&mut self.ledgers)
                    .zip(&self.config.providers)
                    .map(|((store, ledger), p)| placement::Local {
                        store,
                        ledger,
                        job: &p.job,
                    })
                    .collect::<Vec<_>>();
                final_bulk::Execution {
                    enrollment: &enrollment,
                    roster,
                    namespaces: &mut self.namespaces,
                    metadata: &mut self.metadata,
                    places: &mut places,
                    action_jobs: &jobs,
                    witness: placement::Local {
                        store: &witness,
                        ledger: &mut ledger,
                        job: &witness_job,
                    },
                    clock: &expired_clock,
                    admitted_context: &enrollment.context,
                }
                .apply(&fresh, &auth)?
            };
            if replay.phase != final_bulk::Phase::Completed
                || replay.new_logical_copy_bytes_removed != 0
                || replay.historical_deleted_copy_bytes != 58
                || self.copy_snapshot()? != acquired
                || self
                    .ledgers
                    .iter()
                    .map(CapacityLedger::checkpoint)
                    .collect::<Vec<_>>()
                    != prior_ledgers
                || ledger.checkpoint() != prior_witness
                || self.namespaces[0].checkpoint() != prior_source
                || git(
                    &self.git,
                    &self.root.join("project"),
                    &["rev-parse", "HEAD"],
                )? != acquisition_commit
            {
                return Err("old deletion retry changed fresh same-hash acquisition".into());
            }
            git_asset_store::retention::verify(&mut self.ledgers[at], &self.stores[at], &grant)?;
            reacquisition_observation = Some(json!({
                "schema":"tig.teaching-reacquisition-observation.design-1","lesson":"same-hash-reacquired",
                "source_commit":acquisition_commit,"source_checkpoint":prior_source,
                "providers":self.config.providers.iter().map(|p|&p.place.instance).collect::<Vec<_>>(),
                "old_bulk_plan":serde_json::from_slice::<Value>(&fresh.to_json()?)?,
                "plan":serde_json::from_slice::<Value>(&rp.to_json()?)?,"retention":grant,
                "publication":publication_observation,"target_role":"backup",
                "old_retry":{"plan":replay.plan,"phase":replay.phase,"intent_acknowledged":replay.intent_acknowledged,
                    "selected_roots":replay.selected_roots,"selected_bindings":replay.selected_bindings,"unique_objects":replay.unique_objects,
                    "planned_copy_bytes":replay.planned_copy_bytes,"new_removed_bytes":replay.new_logical_copy_bytes_removed,
                    "historical_removed_bytes":replay.historical_deleted_copy_bytes,"holds":replay.holds,"removal_authority":replay.removal_authority},
                "before_replay":{"source":prior_source,"ledgers":prior_ledgers,"witness":prior_witness},
                "after_replay":{"source":self.namespaces[0].checkpoint(),"ledgers":self.ledgers.iter().map(CapacityLedger::checkpoint).collect::<Vec<_>>(),"witness":ledger.checkpoint()},
                "observations":reacquisition_playback::copies(self,&after,&acquired),"acquisition_time":100,"retry_time":700,
                "synthetic_owner_consent":true,"simulated_clock":true,"retention_verified_at_capture":true,
                "independent_hardware_backup_proven":false,"authorizes_storage":false
            }));
            Some(save(
                &self.root.join("final-reacquisition.json"),
                &json!({
                    "schema":"tig.sample-final-reacquisition.design-1","plan":serde_json::from_slice::<Value>(&rp.to_json()?)?,"plan_id":rp.id()?,
                    "old_plan":fresh.id()?,"old_plan_retry":replay,"retention":grant,"copies_before":after,"copies_after":acquired,
                    "retry_clock_after_old_window":700,"synthetic_owner_consent":true,"simulated_clock":true
                }),
            )?)
        } else {
            None
        };
        let loss = save(
            &self.root.join("final-loss.json"),
            &json!({
                "schema":"tig.sample-final-loss.design-1","witness_checkpoint":ledger.checkpoint(),"held_preview":preview,"source_after_job_history_retirement":settled_job,
                "plan":serde_json::from_slice::<Value>(&fresh.to_json()?)?,"report":report,"copies_before":before,"copies_after":after,
                "interruption":if process_exit.is_some(){"actual child process exit at durable intent"}else{"injected SDK intent boundary; not process death"},"copies_after_intent":copies_after_intent,
                "actual_child_exit_code":process_exit,
                "copies_after_completion":copies_after_completion,"retry":retry,
                "cancelled_after_first_removal":cancelled,
                "reacquisition":reacquisition,
                "synthetic_owner_consent":true,"simulated_clock":true,"final_deletion_exercised":true,
                "scope_limit":"synthetic manual exact block; native authority/provider/clock and metadata lifetime acceptance remain open"
            }),
        )?;
        let id = if cancelled {
            "final-loss-cancelled"
        } else {
            "final-loss-completed"
        };
        let mut observations = BTreeMap::new();
        for (name, object) in &self.objects {
            let roles = |snapshot: &Value| {
                ["backup", "history", "working"]
                    .into_iter()
                    .filter(|role| snapshot[*role].get(name).is_some())
                    .collect::<Vec<_>>()
            };
            observations.insert(name,json!({"object":object,"before_copies":roles(&before),"verified_copies":roles(&after),"unknown_copies":[]}));
        }
        let commit = git(
            &self.git,
            &self.root.join("project"),
            &["rev-parse", "HEAD"],
        )?;
        let file = format!("reports/{id}.json");
        let recorded = save(
            &self.root.join(&file),
            &json!({
                "schema":"tig.teaching-final-observation.design-1","lesson":id,"source_commit":commit,"source_checkpoint":final_source,
                "plan":serde_json::from_slice::<Value>(&fresh.to_json()?)?,"receipt":report,
                "providers":self.config.providers.iter().map(|p|&p.place.instance).collect::<Vec<_>>(),"observations":observations,
                "synthetic_owner_consent":true,"simulated_clock":true,"independent_hardware_backup_proven":false
            }),
        )?;
        fs::copy(
            self.root.join(&file),
            Layout::Flat.object_path(&self.root.join("lesson-evidence"), &recorded.sha256),
        )?;
        self.reports.push(json!({"lesson":id,"report":recorded,"file":file,"title":if cancelled {"Final loss cancelled"}else{"Explicit final loss completed"},
            "event":"Manual exact historical block with separate synthetic owner consent and simulated clock. Claim retirement and final-copy deletion are separate. Recorded observation only."}));
        if let Some(observation) = reacquisition_observation {
            reacquisition_playback::record(self, observation)?;
        }
        Ok((preview, Some(loss)))
    }

    fn rename(&mut self, before_commit: &str) -> Result<(String, Object)> {
        let repo = self.root.join("project");
        let old = "records/series.asset";
        let new = "records/renamed-series.asset";
        let bytes = fs::read(repo.join(old))?;
        let occupied = fs::read(repo.join("records/reference.asset"))?;
        let copies = self.copy_snapshot()?;
        let index = git(&self.git, &repo, &["write-tree"])?;
        if git(&self.git, &repo, &["mv", old, "records/reference.asset"]).is_ok()
            || fs::read(repo.join(old))? != bytes
            || fs::read(repo.join("records/reference.asset"))? != occupied
            || git(&self.git, &repo, &["rev-parse", "HEAD"])? != before_commit
            || git(&self.git, &repo, &["write-tree"])? != index
            || self.copy_snapshot()? != copies
        {
            return Err("conflicting rename did not preserve exact state".into());
        }
        let blob = git(&self.git, &repo, &["rev-parse", &format!("HEAD:{old}")])?;
        git(&self.git, &repo, &["mv", old, new])?;
        let head = self.commit("Sample: rename a record without changing its bytes")?;
        if fs::read(repo.join(new))? != bytes
            || repo.join(old).exists()
            || git(&self.git, &repo, &["rev-parse", &format!("HEAD:{new}")])? != blob
        {
            return Err("rename changed authored bytes".into());
        }
        self.select("main", &head, "same-byte-rename")?;
        let (plan, receipt) = self.with_placement(|e| {
            let plan = e.plan(None)?;
            let receipt = e.apply(&plan)?;
            Ok((plan, receipt))
        })?;
        if !receipt.page_converged
            || !receipt.basis_current
            || receipt.next.is_some()
            || !receipt.holds.is_empty()
            || !receipt.effects.is_empty()
            || receipt.stream_bytes != 0
            || self.copy_snapshot()? != copies
        {
            return Err("same-byte rename changed placement".into());
        }
        self.record_checkpoint(22, &head, &plan, &receipt, false, None)?;
        let evidence = save(
            &self.root.join("rename.json"),
            &json!({
                "schema":"tig.sample-rename.design-1","before_commit":before_commit,
                "after_commit":head,"old_path":old,"new_path":new,"git_blob":blob,
                "record_object":Object::from_bytes(&bytes),"conflicting_destination_refused":true,
                "copies_before":copies,"copies_after":self.copy_snapshot()?,
                "stream_bytes":receipt.stream_bytes,"effects":receipt.effects
            }),
        )?;
        Ok((head, evidence))
    }

    fn stale_plan(&mut self, commit: &str, first: &str) -> Result<Object> {
        let old = self.with_placement(|e| e.plan(None))?;
        let planning_source = self.namespaces[0].checkpoint();
        self.select("job", first, "stale-plan-new-job")?;
        let protected_source = self.namespaces[0].checkpoint();
        let before = self.copy_snapshot()?;
        let rejected = self.with_placement(|e| Ok(e.apply(&old)))?;
        let after = self.copy_snapshot()?;
        if !matches!(rejected, Err(placement::Error::Stale))
            || after != before
            || self.namespaces[0].checkpoint() != protected_source
            || protected_source == planning_source
        {
            return Err("stale plan did not refuse before effects".into());
        }
        let status = self.with_placement(|e| e.status(&old))?;
        if status.basis_current
            || status.page_converged
            || status.storage_converged != Some(false)
            || !status.effects.is_empty()
            || status.stream_bytes != 0
            || status.next.is_some()
            || status.observed_basis == old.basis
        {
            return Err("stale status did not preserve the rejected basis".into());
        }
        let old_id = self.record_checkpoint(
            19,
            commit,
            &old,
            &status,
            false,
            Some(json!({"error":"stale","observed_basis":status.observed_basis})),
        )?;
        let fresh = self.checkpoint_coverage(20, commit, false, false)?;
        if fresh == old_id {
            return Err("fresh protection reused the stale plan".into());
        }
        self.withdraw("job", "stale-plan-job-ended")?;
        let ended = self.checkpoint_coverage(21, commit, false, false)?;
        if ended == old_id || ended == fresh {
            return Err("job end revived old plan authority".into());
        }
        save(
            &self.root.join("stale.json"),
            &json!({"schema":"tig.sample-stale-plan.design-1","error":"stale","rejected_plan":old_id,"planning_basis":old.basis,"observed_basis":status.observed_basis,"planning_source":planning_source,"protected_source":protected_source,"copies_before_rejection":before,"copies_after_rejection":after,"fresh_job_plan":fresh,"fresh_job_ended_plan":ended,"status_stream_bytes":status.stream_bytes,"status_effects":status.effects,"new_job_admission_is_explicit_fixture":true}),
        )
    }

    fn backup_outage(&mut self, commit: &str) -> Result<Object> {
        let backup = self.root.join("backup");
        let offline = self.root.join("offline-backup");
        let verified = fs::canonicalize(&backup)?;
        if verified.parent() != Some(self.root.as_path())
            || verified.file_name() != Some(std::ffi::OsStr::new("backup"))
            || offline.parent() != Some(self.root.as_path())
            || offline.exists()
        {
            return Err("outage move outside fresh root".into());
        }
        let before = self.copy_snapshot()?;
        fs::rename(&backup, &offline)?;
        self.checkpoint_coverage(16, commit, true, true)?;
        self.withdraw("job", "finish-job")?;
        let source = self.namespaces[0].checkpoint();
        let held = self.checkpoint_coverage(17, commit, true, true)?;
        let offline_verified = fs::canonicalize(&offline)?;
        if backup.exists()
            || offline_verified.parent() != Some(self.root.as_path())
            || offline_verified.file_name() != Some(std::ffi::OsStr::new("offline-backup"))
            || backup.parent() != Some(self.root.as_path())
        {
            return Err("outage restore outside fresh root".into());
        }
        fs::rename(&offline, &backup)?;
        if fs::canonicalize(&backup)? != verified {
            return Err("restored a different backup path".into());
        }
        let attempts = self.seed.attempts;
        self.seed.disabled = true;
        let recovered = self.checkpoint_coverage(18, commit, false, false)?;
        if recovered != held
            || self.namespaces[0].checkpoint() != source
            || self.seed.attempts != attempts
        {
            return Err("catchup changed source/plan or used the seed".into());
        }
        self.seed.disabled = false;
        save(
            &self.root.join("backup.json"),
            &json!({"schema":"tig.sample-backup-outage.design-1","held_plan":held,"retry_plan":recovered,"source_checkpoint":source,"seed_attempts_during_retry":self.seed.attempts-attempts,"configured_path_restored":true,"before_outage":before,"physical_hardware_fault_qualified":false}),
        )
    }

    fn worktree(&mut self, current: &str, first: &str) -> Result<Object> {
        let project = self.root.join("project");
        let study = self.root.join("study");
        let primary_index = self.repository.index()?.checkpoint().listing.clone();
        git(
            &self.git,
            &project,
            &["worktree", "add", "--detach", "../study", first],
        )?;
        let verified = fs::canonicalize(&study)?;
        if verified.parent() != Some(self.root.as_path())
            || verified.file_name() != Some(std::ffi::OsStr::new("study"))
        {
            return Err("worktree is outside the fresh fixture root".into());
        }
        if git(&self.git, &study, &["rev-parse", "HEAD"])? != first {
            return Err("worktree selected a different commit".into());
        }
        self.select("draft", first, "worktree-open")?;
        self.select("job", first, "worktree-start-job")?;
        self.checkpoint(11, current)?;

        let repo = Repository::new(self.git.clone(), study.clone(), Limits::default())?;
        let capture = |view: &git_asset::source::Snapshot| -> Result<native::AssetSnapshot> {
            Ok(repo.native(
                view,
                Scope::new([LogicalPath::parse("records")?])?,
                Scope::new([LogicalPath::parse("definitions")?])?,
                native::Limits::default(),
            )?)
        };
        let series = |snapshot: &native::AssetSnapshot| -> Result<Object> {
            Ok(snapshot
                .selected_records()
                .find(|r| r.reference().id == "work/series")
                .ok_or("series missing")?
                .selected()?
                .clone())
        };
        let record = study.join("records/series.asset");
        let old_record = fs::read(&record)?;
        fs::write(&record, fs::read(project.join("records/series.asset"))?)?;
        git(&self.git, &study, &["add", "records/series.asset"])?;
        let staged_view = repo.index()?;
        let staged = capture(&staged_view)?;
        if series(&staged)? != self.objects["fixed"] {
            return Err("index did not stage fixed bytes".into());
        }
        let cached_diff = git(&self.git, &study, &["diff", "--cached", "--name-only"])?;
        fs::write(&record, &old_record)?;
        let working_diff = git(&self.git, &study, &["diff", "--name-only"])?;
        let dirty_status = git(&self.git, &study, &["status", "--porcelain=v1"])?;
        if cached_diff != "records/series.asset"
            || working_diff != cached_diff
            || dirty_status != "MM records/series.asset"
            || series(&capture(&staged_view)?)? != self.objects["fixed"]
        {
            return Err("captured index was replaced by working-file state".into());
        }
        let copies_before = self.copy_snapshot()?;
        let dirty_remove_refused = git(&self.git, &project, &["worktree", "remove", "../study"])
            .err()
            .is_some_and(|e| {
                e.to_string()
                    .contains("contains modified or untracked files")
            });
        if !dirty_remove_refused
            || fs::read(&record)? != old_record
            || self.copy_snapshot()? != copies_before
        {
            return Err("dirty worktree removal did not preserve fixture state".into());
        }
        self.select_snapshot("draft", &staged, "worktree-stage-index")?;
        self.checkpoint(12, current)?;

        git(
            &self.git,
            &study,
            &[
                "restore",
                "--staged",
                "--source=HEAD",
                "--",
                "records/series.asset",
            ],
        )?;
        let unstaged_view = repo.index()?;
        let unstaged = capture(&unstaged_view)?;
        if series(&unstaged)? != self.objects["original"]
            || series(&staged)? != self.objects["fixed"]
            || series(&capture(&staged_view)?)? != self.objects["fixed"]
            || !git(&self.git, &study, &["status", "--porcelain=v1"])?.is_empty()
            || staged_view.checkpoint().listing == unstaged_view.checkpoint().listing
        {
            return Err("unstaging changed an old snapshot or selected wrong bytes".into());
        }
        self.select_snapshot("draft", &unstaged, "worktree-unstage-index")?;
        self.checkpoint(13, current)?;
        if self.repository.index()?.checkpoint().listing != primary_index
            || git(&self.git, &project, &["rev-parse", "HEAD"])? != current
            || git(&self.git, &study, &["rev-parse", "HEAD"])? != first
        {
            return Err("linked worktree changed the primary HEAD or index".into());
        }
        // Verify the canonical target immediately before normal, non-forced removal.
        if fs::canonicalize(&study)? != verified || verified.parent() != Some(self.root.as_path()) {
            return Err("worktree target changed".into());
        }
        git(&self.git, &project, &["worktree", "remove", "../study"])?;
        if study.exists() {
            return Err("worktree was not removed".into());
        }
        self.withdraw("draft", "worktree-close-draft")?;
        self.checkpoint(14, current)?;
        self.withdraw("job", "worktree-end-job")?;
        self.checkpoint(15, current)?;
        save(
            &self.root.join("worktree.json"),
            &json!({"schema":"tig.sample-worktree.design-1","main_commit":current,"worktree_commit":first,"primary_index":primary_index,"staged_index":staged_view.checkpoint().listing,"unstaged_index":unstaged_view.checkpoint().listing,"staged_native":staged.checkpoint(),"unstaged_native":unstaged.checkpoint(),"staged_selected":series(&staged)?,"unstaged_selected":series(&unstaged)?,"cached_diff":cached_diff,"working_diff":working_diff,"dirty_status":dirty_status,"dirty_remove_refused":dirty_remove_refused,"copy_snapshot_during_refusal":copies_before,"worktree_removed":true,"job_ended_after_worktree":true,"automatic_writer_enrollment":false}),
        )
    }

    fn exchange(&mut self, first: &str) -> Result<(String, Object)> {
        let project = self.root.join("project");
        let observer = self.root.join("observer");
        // These paths are generated under a fresh root, never caller-supplied URLs.
        git(
            &self.git,
            &self.root,
            &[
                "init",
                "--bare",
                "--template=git-template",
                "-b",
                "main",
                "exchange.git",
            ],
        )?;
        git(
            &self.git,
            &project,
            &["remote", "add", "sample", "../exchange.git"],
        )?;
        git_transport(
            &self.git,
            &project,
            &["push", "sample", "refs/heads/main:refs/heads/main"],
            true,
        )?;
        git_transport(
            &self.git,
            &self.root,
            &[
                "clone",
                "--no-local",
                "--no-hardlinks",
                "--template=git-template",
                "--branch",
                "main",
                "exchange.git",
                "observer",
            ],
            true,
        )?;
        if git(&self.git, &observer, &["rev-parse", "HEAD"])? != first {
            return Err("clone selected a different commit".into());
        }
        // Native payload seeds must not have been smuggled into the Git tree.
        let files = git(
            &self.git,
            &observer,
            &["ls-tree", "-r", "--name-only", "HEAD"],
        )?;
        for file in files.lines() {
            let bytes = fs::read(observer.join(file))?;
            if self
                .objects
                .values()
                .any(|object| *object == Object::from_bytes(&bytes))
            {
                return Err("payload was transported as a Git blob".into());
            }
        }
        self.checkpoint(8, first)?;

        git(&self.git, &project, &["revert", "--no-edit", first])?;
        let old = git(&self.git, &project, &["rev-parse", "HEAD"])?;
        git_transport(
            &self.git,
            &project,
            &["push", "sample", "refs/heads/main:refs/heads/main"],
            true,
        )?;
        git_transport(
            &self.git,
            &observer,
            &[
                "fetch",
                "--no-tags",
                "origin",
                "refs/heads/main:refs/remotes/origin/main",
            ],
            true,
        )?;
        let after_fetch = git(&self.git, &observer, &["rev-parse", "HEAD"])?;
        if after_fetch != first
            || git(
                &self.git,
                &observer,
                &["rev-parse", "refs/remotes/origin/main"],
            )? != old
        {
            return Err("fetch changed the checkout or selected a different ref".into());
        }
        let record_path = observer.join("records/series.asset");
        let clean = fs::read(&record_path)?;
        let mut dirty: Value = serde_json::from_slice(&clean)?;
        dirty["authored"]["version"] = "uncommitted-learner-edit".into();
        let dirty = identity::canonical_json(&dirty)?;
        fs::write(&record_path, &dirty)?;
        let before = self.copy_snapshot()?;
        let refused = git_transport(
            &self.git,
            &observer,
            &["pull", "--ff-only", "origin", "main"],
            true,
        )
        .err()
        .is_some_and(|error| error.to_string().contains("would be overwritten by merge"));
        if !refused
            || fs::read(&record_path)? != dirty
            || git(&self.git, &observer, &["rev-parse", "HEAD"])? != first
            || self.copy_snapshot()? != before
        {
            return Err("dirty pull did not preserve the checkout and managed copies".into());
        }
        // Repair only the exact synthetic edit made above; no user checkout is admitted.
        fs::write(&record_path, clean)?;
        git_transport(
            &self.git,
            &observer,
            &["pull", "--ff-only", "origin", "main"],
            true,
        )?;
        self.check_observer(&observer, &old, "original")?;
        self.select("main", &old, "exchange-v1")?;
        self.checkpoint(9, &old)?;

        git(&self.git, &project, &["revert", "--no-edit", &old])?;
        let current = git(&self.git, &project, &["rev-parse", "HEAD"])?;
        git_transport(
            &self.git,
            &project,
            &["push", "sample", "refs/heads/main:refs/heads/main"],
            true,
        )?;
        git_transport(
            &self.git,
            &observer,
            &[
                "fetch",
                "--no-tags",
                "origin",
                "refs/heads/main:refs/remotes/origin/main",
            ],
            true,
        )?;
        if git(&self.git, &observer, &["rev-parse", "HEAD"])? != old {
            return Err("fetch advanced observer HEAD".into());
        }
        git_transport(
            &self.git,
            &observer,
            &["pull", "--ff-only", "origin", "main"],
            true,
        )?;
        self.check_observer(&observer, &current, "fixed")?;
        self.select("main", &current, "exchange-v2")?;
        self.checkpoint(10, &current)?;
        let evidence = save(
            &self.root.join("exchange.json"),
            &json!({"schema":"tig.sample-exchange.design-1","published":first,"after_fetch":after_fetch,"pulled_v1":old,"returned_v2":current,"dirty_pull_refused":refused,"dirty_pull_reason":"local changes would be overwritten by merge","dirty_copy_snapshot":before,"selected_payloads_in_cloned_tree":false,"observer_is_enrolled_writer":false,"remote_transport":"fresh-root-local-file-only"}),
        )?;
        Ok((current, evidence))
    }
    fn check_observer(&self, observer: &Path, commit: &str, name: &str) -> Result<()> {
        let record: Value =
            serde_json::from_slice(&fs::read(observer.join("records/series.asset"))?)?;
        if git(&self.git, observer, &["rev-parse", "HEAD"])? != commit
            || record["selected"] != serde_json::to_value(&self.objects[name])?
            || !git(&self.git, observer, &["status", "--porcelain=v1"])?.is_empty()
        {
            return Err("observer did not select exact clean records".into());
        }
        Ok(())
    }
    fn copy_snapshot(&self) -> Result<Value> {
        Self::snapshot_at(&self.root, &self.objects)
    }
    fn snapshot_at(root: &Path, objects: &BTreeMap<String, Object>) -> Result<Value> {
        let mut observed = BTreeMap::new();
        for role in ["working", "history", "backup"] {
            let mut copies = BTreeMap::new();
            for (name, object) in objects {
                let path = Layout::Flat.object_path(&root.join(role), &object.sha256);
                match fs::read(path) {
                    Ok(bytes) => {
                        if Object::from_bytes(&bytes) != *object {
                            return Err("copy changed during sample rehearsal".into());
                        }
                        copies.insert(name, object);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
            }
            observed.insert(role, copies);
        }
        Ok(serde_json::to_value(observed)?)
    }
}
fn run() -> Result<Value> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 2 && args[0] == "__final-fixture-child" {
        return recovery::child(args[1].to_str().ok_or("invalid fixture digest")?);
    }
    if args.len() == 1 && args[0] == "describe" {
        let mut value = json!({"schema":"tig.sample-interface.design-1","operations":["describe","rehearse-ordinary NEW_ABSOLUTE_ROOT","rehearse-exchange NEW_ABSOLUTE_ROOT","rehearse-worktree NEW_ABSOLUTE_ROOT","rehearse-backup-outage NEW_ABSOLUTE_ROOT","rehearse-stale-plan NEW_ABSOLUTE_ROOT","rehearse-rename NEW_ABSOLUTE_ROOT","rehearse-final-enrollment NEW_ABSOLUTE_ROOT","rehearse-final-manual NEW_ABSOLUTE_ROOT","rehearse-final-git-return NEW_ABSOLUTE_ROOT","rehearse-return-restore NEW_ABSOLUTE_ROOT","rehearse-final-cancel NEW_ABSOLUTE_ROOT","rehearse-final-reacquire NEW_ABSOLUTE_ROOT","rehearse-final-process NEW_ABSOLUTE_ROOT","rehearse-retained-age NEW_ABSOLUTE_ROOT","rehearse-aged-category NEW_ABSOLUTE_ROOT"],"fixture_only":true,"opens_existing_roots":false,"complete_course":false});
        if cfg!(feature = "upload") {
            value["operations"]
                .as_array_mut()
                .ok_or("sample operations")?
                .push("rehearse-remote-publication NEW_ABSOLUTE_ROOT".into());
        }
        return Ok(value);
    }
    if args.len() != 2 {
        return Err("usage: git-asset-sample describe | OPERATION NEW_ABSOLUTE_ROOT (see describe for operations)".into());
    }
    let mode = match args[0].to_str() {
        Some("rehearse-ordinary") => Mode::Ordinary,
        Some("rehearse-exchange") => Mode::Exchange,
        Some("rehearse-worktree") => Mode::Worktree,
        Some("rehearse-backup-outage") => Mode::BackupOutage,
        Some("rehearse-stale-plan") => Mode::StalePlan,
        Some("rehearse-rename") => Mode::Rename,
        Some("rehearse-final-enrollment") => Mode::FinalEnrollment,
        Some("rehearse-final-manual") => Mode::FinalManual,
        Some("rehearse-final-git-return") => Mode::FinalGitReturn,
        Some("rehearse-return-restore") => Mode::ReturnRestore,
        Some("rehearse-remote-publication") => {
            if !cfg!(feature = "upload") {
                return Err("remote publication sample requires schema,upload".into());
            }
            #[cfg(feature = "upload")]
            remote_publication::preflight()?;
            Mode::RemotePublication
        }
        Some("rehearse-final-cancel") => Mode::FinalCancel,
        Some("rehearse-final-reacquire") => Mode::FinalReacquire,
        Some("rehearse-final-process") => Mode::FinalProcess,
        Some("rehearse-retained-age") => Mode::FinalAgePreview,
        Some("rehearse-aged-category") => Mode::FinalAged,
        _ => return Err("unknown sample operation".into()),
    };
    Demo::new(Path::new(&args[1]), mode)?.run(mode)
}
fn main() -> ExitCode {
    match run() {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!(
                "{}",
                json!({"schema":"tig.sample-error.design-1","error":e.to_string()})
            );
            ExitCode::FAILURE
        }
    }
}

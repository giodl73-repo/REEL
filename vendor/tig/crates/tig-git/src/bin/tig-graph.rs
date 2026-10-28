//! Explicit read-only declared-reference inspection; no payload or placement IO.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeSet,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
use tig_assets::{
    graph::{self, EdgeKind, Graph, ProductionRequirements},
    resolve::AssetResolver,
};
use tig_core::{Digest, LogicalPath, Object, identity::parse_json};
use tig_git::source::{GitId, Limits as GitLimits, Repository, Scope, native::Limits};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[path = "tig_graph/verification.rs"]
mod verification;
const MAX_REPORT: usize = 16 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    git_executable: PathBuf,
    repository: PathBuf,
    commit: String,
    records: Vec<String>,
    definitions: Vec<String>,
    #[serde(default)]
    limits: Limits,
    roots: Vec<String>,
    kinds: BTreeSet<EdgeKind>,
    profiles: Vec<graph::Profile>,
    query_limits: graph::Limits,
    max_report_bytes: usize,
}
fn scope(values: Vec<String>) -> Result<Scope> {
    Ok(Scope::new(
        values
            .iter()
            .map(|s| LogicalPath::parse(s))
            .collect::<std::result::Result<Vec<_>, _>>()?,
    )?)
}
struct Resolved {
    input: Object,
    graph: Graph,
    requirements: Option<ProductionRequirements>,
    report_limit: usize,
}
fn resolve(input: &Path) -> Result<Resolved> {
    let mut bytes = Vec::new();
    File::open(input)?.take(65_537).read_to_end(&mut bytes)?;
    let profile: Profile = serde_json::from_value(parse_json(&bytes)?)?;
    if profile.schema != "tig.graph-inspection-profile.experimental-1"
        || profile.roots.is_empty()
        || profile.roots.len() > 128
        || profile.roots.iter().collect::<BTreeSet<_>>().len() != profile.roots.len()
        || profile.max_report_bytes == 0
        || profile.max_report_bytes > MAX_REPORT
        || !profile.repository.is_absolute()
        || !profile.git_executable.is_absolute()
    {
        return Err("invalid graph profile".into());
    }
    let repository = Repository::new(
        profile.git_executable,
        profile.repository,
        GitLimits::default(),
    )?;
    let view = repository.commit(GitId::parse(&profile.commit)?)?;
    let mut assets = repository.native(
        &view,
        scope(profile.records)?,
        scope(profile.definitions)?,
        profile.limits,
    )?;
    let roots = profile
        .roots
        .iter()
        .map(|id| assets.resolve_id(id).map(|a| a.reference().clone()))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    let query = graph::Query {
        checkpoint: assets.checkpoint().clone(),
        roots,
        kinds: profile.kinds,
        limits: profile.query_limits,
    };
    let graph = graph::resolve(&mut assets, &query, &profile.profiles)?;
    let requirements = graph.production_requirements().ok();
    Ok(Resolved {
        input: Object::from_bytes(&bytes),
        graph,
        requirements,
        report_limit: profile.max_report_bytes,
    })
}
#[derive(Serialize)]
struct Requirements<'a> {
    primary_objects: &'a BTreeSet<Object>,
    control_objects: &'a BTreeSet<Object>,
    interpretation_objects: &'a BTreeSet<Object>,
}
#[derive(Serialize)]
struct View<'a> {
    source_checkpoint: &'a Digest,
    admission_sha256: &'a Digest,
    binding_sha256: Option<&'a Digest>,
    roots: &'a BTreeSet<graph::Reference>,
    kinds: &'a BTreeSet<EdgeKind>,
    nodes: Vec<&'a graph::Node>,
    edges: &'a [graph::Edge],
    gaps: &'a [graph::Gap],
    returned_metadata_bytes: usize,
    complete_declared_coverage: bool,
    complete_semantic_coverage: bool,
    authored_schemas_validated: bool,
    production_requirements: Option<Requirements<'a>>,
}
impl<'a> View<'a> {
    fn of(resolved: &'a Resolved) -> Self {
        let graph = &resolved.graph;
        Self {
            source_checkpoint: graph.checkpoint(),
            admission_sha256: graph.admission(),
            binding_sha256: graph.binding_digest(),
            roots: graph.roots(),
            kinds: graph.kinds(),
            nodes: graph.nodes().values().collect(),
            edges: graph.edges(),
            gaps: graph.gaps(),
            returned_metadata_bytes: graph.bytes_read(),
            complete_declared_coverage: graph.complete_declared_coverage(),
            complete_semantic_coverage: false,
            authored_schemas_validated: false,
            production_requirements: resolved.requirements.as_ref().map(|r| Requirements {
                primary_objects: r.primary(),
                control_objects: r.controls(),
                interpretation_objects: r.metadata(),
            }),
        }
    }
}
#[derive(Serialize)]
struct Inspection<'a> {
    schema: &'static str,
    input_profile: &'a Object,
    graph: View<'a>,
    payloads_verified: bool,
    complete_claim_coverage: bool,
    authorizes_eviction: bool,
}
#[derive(Serialize)]
struct Change<'a> {
    before: &'a Digest,
    after: &'a Digest,
    admission: &'a Digest,
    added: &'a BTreeSet<Object>,
    shared: &'a BTreeSet<Object>,
    unreferenced_by_new_query: &'a BTreeSet<Object>,
}
#[derive(Serialize)]
struct Comparison<'a> {
    schema: &'static str,
    before_profile: &'a Object,
    after_profile: &'a Object,
    before: View<'a>,
    after: View<'a>,
    change: Option<Change<'a>>,
    payloads_verified: bool,
    complete_claim_coverage: bool,
    authorizes_eviction: bool,
}
struct Bounded {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("report budget exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn check_output(output: &Path) -> Result<()> {
    if !output.is_absolute()
        || output.symlink_metadata().is_ok()
        || !output
            .parent()
            .is_some_and(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()))
    {
        return Err("new absolute output under trusted existing parent required".into());
    }
    Ok(())
}
fn publish(value: &impl Serialize, output: &Path, limit: usize) -> Result<()> {
    let mut writer = Bounded {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value)?;
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce)?;
    let temporary = output
        .parent()
        .ok_or("parent missing")?
        .join(format!(".tig-graph-{}.tmp", Digest::blob(&nonce).as_str()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let owned = Temporary(temporary);
    file.write_all(&writer.bytes)?;
    file.sync_all()?;
    drop(file);
    fs::hard_link(&owned.0, output)?;
    drop(owned);
    Ok(())
}
fn inspect(input: &Path, output: &Path) -> Result<bool> {
    check_output(output)?;
    let resolved = resolve(input)?;
    let production = resolved.graph.kinds() == &BTreeSet::from([EdgeKind::Production]);
    let complete = resolved.graph.complete_declared_coverage()
        && (!production || resolved.requirements.is_some());
    publish(
        &Inspection {
            schema: "tig.graph-inspection.experimental-1",
            input_profile: &resolved.input,
            graph: View::of(&resolved),
            payloads_verified: false,
            complete_claim_coverage: false,
            authorizes_eviction: false,
        },
        output,
        resolved.report_limit,
    )?;
    println!(
        "{}",
        json!({"schema":"tig.graph-inspection-summary.experimental-1","nodes":resolved.graph.nodes().len(),
        "edges":resolved.graph.edges().len(),"gaps":resolved.graph.gaps().len(),"complete_declared_coverage":resolved.graph.complete_declared_coverage(),
        "production_requirements_available":resolved.requirements.is_some(),"complete_semantic_coverage":false,
        "authored_schemas_validated":false,"authorizes_eviction":false})
    );
    Ok(complete)
}
fn compare(before: &Path, after: &Path, output: &Path) -> Result<bool> {
    check_output(output)?;
    let old = resolve(before)?;
    let new = resolve(after)?;
    let change = if old.requirements.is_some() && new.requirements.is_some() {
        graph::primary_change(&old.graph, &new.graph).ok()
    } else {
        None
    };
    publish(
        &Comparison {
            schema: "tig.graph-comparison.experimental-1",
            before_profile: &old.input,
            after_profile: &new.input,
            before: View::of(&old),
            after: View::of(&new),
            change: change.as_ref().map(|c| Change {
                before: &c.before,
                after: &c.after,
                admission: &c.admission,
                added: &c.added,
                shared: &c.shared,
                unreferenced_by_new_query: &c.unreferenced_by_new_query,
            }),
            payloads_verified: false,
            complete_claim_coverage: false,
            authorizes_eviction: false,
        },
        output,
        old.report_limit.min(new.report_limit),
    )?;
    println!(
        "{}",
        json!({"schema":"tig.graph-comparison-summary.experimental-1","comparison_available":change.is_some(),
        "added":change.as_ref().map(|c|c.added.len()),"shared":change.as_ref().map(|c|c.shared.len()),
        "unreferenced_by_new_query":change.as_ref().map(|c|c.unreferenced_by_new_query.len()),
        "authored_schemas_validated":false,"authorizes_eviction":false})
    );
    Ok(change.is_some())
}
fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).take(5).collect();
    let result = if args.len() == 3 && args[0] == "inspect" {
        inspect(Path::new(&args[1]), Path::new(&args[2]))
    } else if args.len() == 4 && args[0] == "compare" {
        compare(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
        )
    } else if args.len() == 4 && args[0] == "verify" {
        verification::run(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
        )
    } else {
        eprintln!(
            "usage: tig-graph inspect GRAPH_PROFILE NEW_REPORT; compare BEFORE_PROFILE AFTER_PROFILE NEW_REPORT; verify GRAPH_PROFILE BYTE_PROFILE NEW_REPORT"
        );
        return ExitCode::FAILURE;
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("graph operation incomplete; see private report when available");
            ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!(
                "graph operation refused; verify exact source, admission, byte policy, bounds and new output"
            );
            ExitCode::FAILURE
        }
    }
}

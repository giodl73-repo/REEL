//! Historical role preservation for salvage only. No compiler/recipe replay,
//! producer currency, output success, execution or removal authority.
use super::*;
use serde::Deserialize;
use std::{collections::BTreeSet, fs::File, path::PathBuf};
use tig_assets::graph::recipe::Operation;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    artifact_root: PathBuf,
    workspace: PathBuf,
    request: Object,
    max_workspace_bytes: u64,
    original_input: Selection,
    cache: Config,
    coordination_root: PathBuf,
    journal_root: PathBuf,
    journal_minimum: tig_store::source_claims::Checkpoint,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source_id: String,
    owner: String,
    expected_revision: u64,
    retain_operation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobHeader {
    schema: String,
    source: Selection,
    checkpoint: Digest,
    admission: Digest,
    binding: Digest,
    consumption: Digest,
    recipe: Digest,
    operation: Operation,
    runtime: usize,
    claim: Option<Object>,
    evidence: usize,
    variants: Option<Selection>,
    composition: Option<usize>,
    input_evidence: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextHeader {
    schema: String,
    checkpoint: Digest,
    admission: Digest,
    binding: Digest,
    archive: Object,
    policy: Object,
    descriptor: Object,
    roots: usize,
    primary: usize,
    controls: usize,
    interpretation: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantHeader {
    schema: String,
    source: Selection,
    selections: usize,
    policies: usize,
    controls: usize,
}
fn variant_objects(
    selection: &Selection,
    source: &Selection,
    reader: &mut MetadataReader,
) -> Result<BTreeSet<Object>> {
    let mut entries = Tree::from_root(selection.root.clone())
        .entries(reader, TreeLimits::default())?
        .0;
    let identity = get(&entries, "packet/identity")?.clone();
    if identity.sha256 != selection.context {
        bail!("variant identity differs");
    }
    let header: VariantHeader = serde_json::from_value(document(reader, &identity)?)?;
    if header.schema != "tig.native-variant-packet.experimental-1" || &header.source != source {
        bail!("variant source differs");
    }
    let mut objects: BTreeSet<_> = entries.values().cloned().collect();
    objects.insert(selection.root.clone());
    take(&mut entries, "packet/identity", &identity)?;
    take(&mut entries, "source/context", &source.root)?;
    collect(&mut entries, "policy/", header.policies, false)?;
    collect(&mut entries, "witness/", header.selections, false)?;
    collect(&mut entries, "control/", header.controls, true)?;
    if !entries.is_empty() {
        bail!("unknown historical variant entries");
    }
    Ok(objects)
}
struct OwnedInputs {
    selection: Selection,
    roles: BTreeMap<Role, BTreeSet<Object>>,
}
impl ResourceView for OwnedInputs {
    fn selection(&self) -> &Selection {
        &self.selection
    }
    fn of(&self, role: Role) -> &BTreeSet<Object> {
        &self.roles[&role]
    }
}
fn document(reader: &mut MetadataReader, pin: &Object) -> Result<Value> {
    if pin.bytes > 65536 {
        bail!("historical role header exceeds bound");
    }
    let bytes = reader.get(pin)?.context("historical role header absent")?;
    if Object::from_bytes(&bytes) != *pin {
        bail!("historical header differs");
    }
    Ok(identity::parse_json(&bytes)?)
}
fn take(entries: &mut BTreeMap<LogicalPath, Object>, name: &str, pin: &Object) -> Result<()> {
    if entries.remove(&LogicalPath::parse(name)?).as_ref() != Some(pin) {
        bail!("historical metadata link differs");
    }
    Ok(())
}
fn collect(
    entries: &mut BTreeMap<LogicalPath, Object>,
    prefix: &str,
    count: usize,
    hashed: bool,
) -> Result<BTreeSet<Object>> {
    let names: Vec<_> = entries
        .keys()
        .filter(|n| n.as_str().starts_with(prefix))
        .cloned()
        .collect();
    if names.len() != count {
        bail!("historical role count differs");
    }
    let mut objects = BTreeSet::new();
    for name in names {
        let object = entries.remove(&name).unwrap();
        if hashed && name.as_str().strip_prefix(prefix) != Some(object.sha256.as_str()) {
            bail!("historical role name differs");
        }
        objects.insert(object);
    }
    Ok(objects)
}
/// Read only the stored role roster of the exact owned input. This type cannot
/// enter a production run API; graph/recipe/compiler admission is not asserted.
fn owned_inputs(selection: &Selection, reader: &mut MetadataReader) -> Result<OwnedInputs> {
    let mut entries = Tree::from_root(selection.root.clone())
        .entries(reader, TreeLimits::default())?
        .0;
    let identity = get(&entries, "job/identity")?.clone();
    if identity.sha256 != selection.context {
        bail!("owned job identity differs");
    }
    let value = document(reader, &identity)?;
    if value.get("claim") != Some(&Value::Null) {
        bail!("salvage requires an input-only job");
    }
    let header: JobHeader = serde_json::from_value(value)?;
    let version = match header.schema.as_str() {
        "tig.native-job-context.experimental-1" => 1,
        "tig.native-job-context.experimental-2" => 2,
        "tig.native-job-context.experimental-3" => 3,
        _ => bail!("unsupported historical job schema"),
    };
    if header.claim.is_some()
        || header.evidence != 0
        || header.runtime > 64
        || (version == 1 && header.variants.is_some())
        || (version == 2 && header.variants.is_none())
        || (version != 3 && (header.composition.is_some() || header.input_evidence.is_some()))
        || (version == 3
            && (header.composition.is_none()
                || header.input_evidence.is_none()
                || header.composition == Some(0) && header.input_evidence == Some(0)))
    {
        bail!("invalid input-only historical job shape");
    }
    take(&mut entries, "job/identity", &identity)?;
    take(&mut entries, "source/context", &header.source.root)?;
    take(
        &mut entries,
        "operation/producer",
        &header.operation.producer,
    )?;
    take(
        &mut entries,
        "operation/parameters",
        &header.operation.parameters,
    )?;
    if let Some(variants) = &header.variants {
        take(&mut entries, "source/variants", &variants.root)?;
    }
    let mut runtime = collect(&mut entries, "runtime/", header.runtime, false)?;
    runtime.insert(header.operation.producer);
    let composition = collect(
        &mut entries,
        "composition/",
        header.composition.unwrap_or(0),
        false,
    )?;
    let evidence = collect(
        &mut entries,
        "input-evidence/",
        header.input_evidence.unwrap_or(0),
        false,
    )?;
    if !entries.is_empty() {
        bail!("unknown historical job entries");
    }
    let mut entries = Tree::from_root(header.source.root.clone())
        .entries(reader, TreeLimits::default())?
        .0;
    let context_identity = get(&entries, "context/identity")?.clone();
    if context_identity.sha256 != header.source.context {
        bail!("historical source identity differs");
    }
    let context: ContextHeader = serde_json::from_value(document(reader, &context_identity)?)?;
    if context.schema != "tig.native-production-context.experimental-1"
        || context.checkpoint != header.checkpoint
        || context.admission != header.admission
        || context.binding != header.binding
    {
        bail!("historical source binding differs");
    }
    take(&mut entries, "context/identity", &context_identity)?;
    take(&mut entries, "source/archive", &context.archive)?;
    take(&mut entries, "source/policy", &context.policy)?;
    take(&mut entries, "operation/descriptor", &context.descriptor)?;
    collect(&mut entries, "roots/", context.roots, true)?;
    let primary = collect(&mut entries, "primary/", context.primary, true)?;
    let controls = collect(&mut entries, "controls/", context.controls, true)?;
    let mut interpretation = collect(
        &mut entries,
        "interpretation/",
        context.interpretation,
        true,
    )?;
    if !entries.is_empty() {
        bail!("unknown historical context entries");
    }
    interpretation.extend([
        identity,
        context_identity,
        selection.root.clone(),
        header.source.root.clone(),
        context.archive,
        context.policy,
        context.descriptor,
        header.operation.parameters,
    ]);
    if let Some(variants) = header.variants {
        interpretation.extend(variant_objects(&variants, &header.source, reader)?);
    }
    let roles = BTreeMap::from([
        (Role::Primary, primary),
        (Role::Control, controls),
        (Role::Runtime, runtime),
        (Role::Interpretation, interpretation),
        (Role::Composition, composition),
        (Role::Evidence, evidence),
        (Role::ClaimedOutput, BTreeSet::new()),
    ]);
    let mut lengths = BTreeMap::new();
    for object in roles.values().flatten() {
        if object.bytes > 9_007_199_254_740_991
            || lengths
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|old| old != object.bytes)
        {
            bail!("historical role lengths conflict");
        }
    }
    // These are retained historical facts, deliberately not new plan/recipe
    // fingerprints. Deserialize them to enforce their identity shape only.
    let _ = (header.consumption, header.recipe);
    Ok(OwnedInputs {
        selection: selection.clone(),
        roles,
    })
}
fn run(path: &Path, apply: bool) -> Result<Value> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 {
        bail!("salvage admission exceeds bound");
    }
    let a: Admission = serde_json::from_value(identity::parse_json(&bytes)?)?;
    if a.schema != "reel.tig-workspace-salvage-profile.experimental-1"
        || !a.artifact_root.is_absolute()
        || !a.artifact_root.is_dir()
        || !a.coordination_root.is_absolute()
        || !a.coordination_root.is_dir()
        || a.cache.coordination_root.as_ref() != Some(&a.coordination_root)
        || !a.journal_root.is_absolute()
        || !a.metadata_root.is_absolute()
        || a.cache.cache_roots.is_empty()
        || a.cache
            .cache_roots
            .iter()
            .any(|root| !root.is_absolute() || !root.join("objects/sha256").is_dir())
        || a.request.bytes > 256 * 1024
    {
        bail!("explicit existing salvage stores required");
    }
    let coordination = Coordination {
        root: a.coordination_root,
    };
    let profile = Profile {
        config: &a.cache,
        coordination: &coordination,
        identity: Identity {
            source: Key::parse(&a.source_id)?,
            owner: Key::parse(&a.owner)?,
        },
        expected_revision: a.expected_revision,
        operation: Key::parse(&a.retain_operation)?,
        metadata_root: &a.metadata_root,
        metadata_minimum: &a.metadata_minimum,
        tree_limits: TreeLimits::default(),
    };
    let mut reader = MetadataReader::open(&a.metadata_root, &a.metadata_minimum)?;
    let resources = owned_inputs(&a.original_input, &mut reader)?;
    let manifest = a.workspace.join("job.json");
    if std::fs::symlink_metadata(&manifest)?.len() != a.request.bytes {
        bail!("salvage request length differs");
    }
    let mut journal = SourceJournal::open(&a.journal_root, &a.journal_minimum)?;
    let result = salvage_with_resources(
        &a.artifact_root,
        jobs::Salvage {
            workspace: &a.workspace,
            request_sha256: a.request.sha256.as_str(),
            max_workspace_bytes: a.max_workspace_bytes,
            apply,
        },
        &resources,
        &mut journal,
        &profile,
    )?;
    Ok(
        json!({"schema":"reel.tig-workspace-salvage-summary.experimental-1","applied":result["applied"],"succeeded":false,"declared_candidates":result["declared_candidates"],"source_revision":result["source_revision"],"workspace_retained":true,"producer_currency_asserted":false,"production_claim":null,"authorizes_execution":false,"authorizes_workspace_removal":false,"authorizes_eviction":false}),
    )
}
pub fn salvage_workspace(path: &Path, apply: bool) -> Result<Value> {
    run(path,apply).map_err(|_|anyhow::anyhow!("workspace salvage refused; verify original input ownership, request, stores and quiescent workspace"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tig_store::metadata::Limits;
    fn fixture(case: &str) -> (tempfile::TempDir, Selection, Checkpoint, Object, Object) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("metadata");
        let mut store = MetadataStore::create(
            &root,
            Limits {
                nodes: 1024,
                bytes: 4 * 1024 * 1024,
                database_bytes: 8 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let pin = |text: &str| Object::from_bytes(text.as_bytes());
        let primary = pin("primary candidate bytes");
        let control = pin("control document bytes");
        let checkpoint = pin("checkpoint").sha256;
        let admission = pin("admission").sha256;
        let binding = pin("binding").sha256;
        let archive = pin("archive");
        let policy = pin("policy");
        let descriptor = pin("descriptor");
        let mut ch = json!({"schema":"tig.native-production-context.experimental-1","checkpoint":checkpoint,"admission":admission,"binding":binding,"archive":archive,"policy":policy,"descriptor":descriptor,"roots":0,"primary":1,"controls":1,"interpretation":0});
        if case == "context-count" {
            ch["primary"] = json!(2);
        }
        if case == "context-field" {
            ch["unknown"] = json!(true);
        }
        let context_identity = store.put(&identity::canonical_json(&ch).unwrap()).unwrap();
        let mut entries = BTreeMap::from([
            (
                LogicalPath::parse("context/identity").unwrap(),
                context_identity.clone(),
            ),
            (LogicalPath::parse("source/archive").unwrap(), archive),
            (LogicalPath::parse("source/policy").unwrap(), policy),
            (
                LogicalPath::parse("operation/descriptor").unwrap(),
                descriptor,
            ),
            (
                LogicalPath::parse(&format!("primary/{}", primary.sha256.as_str())).unwrap(),
                primary.clone(),
            ),
            (
                LogicalPath::parse(&format!(
                    "controls/{}",
                    if case == "role-key" {
                        "wrong"
                    } else {
                        control.sha256.as_str()
                    }
                ))
                .unwrap(),
                control.clone(),
            ),
        ]);
        if case == "context-entry" {
            entries.insert(LogicalPath::parse("future/entry").unwrap(), pin("future"));
        }
        let context = Selection {
            root: Tree::build(&entries, &mut store, TreeLimits::default())
                .unwrap()
                .0
                .root()
                .clone(),
            context: context_identity.sha256,
        };
        let producer = pin("old producer");
        let parameters = pin("old parameters");
        let mut header = json!({"schema":"tig.native-job-context.experimental-1","source":context,"checkpoint":checkpoint,"admission":admission,"binding":binding,"consumption":pin("consumption").sha256,"recipe":pin("recipe").sha256,"operation":{"name":"old-operation","semantics":pin("old semantics").sha256,"producer":producer,"parameters":parameters},"runtime":0,"claim":null,"evidence":0});
        let mut job_entries = BTreeMap::from([
            (
                LogicalPath::parse("source/context").unwrap(),
                context.root.clone(),
            ),
            (LogicalPath::parse("operation/producer").unwrap(), producer),
            (
                LogicalPath::parse("operation/parameters").unwrap(),
                parameters,
            ),
        ]);
        if case == "variant" || case == "variant-source" {
            let variant_header=store.put(&identity::canonical_json(&json!({"schema":"tig.native-variant-packet.experimental-1","source":if case=="variant-source" {Selection {root:context.root.clone(),context:pin("foreign").sha256}} else {context.clone()},"selections":0,"policies":0,"controls":1})).unwrap()).unwrap();
            let v = BTreeMap::from([
                (
                    LogicalPath::parse("packet/identity").unwrap(),
                    variant_header.clone(),
                ),
                (
                    LogicalPath::parse("source/context").unwrap(),
                    context.root.clone(),
                ),
                (
                    LogicalPath::parse(&format!("control/{}", control.sha256.as_str())).unwrap(),
                    control.clone(),
                ),
            ]);
            let variant = Selection {
                root: Tree::build(&v, &mut store, TreeLimits::default())
                    .unwrap()
                    .0
                    .root()
                    .clone(),
                context: variant_header.sha256,
            };
            header["schema"] = json!("tig.native-job-context.experimental-2");
            header["variants"] = json!(variant);
            job_entries.insert(LogicalPath::parse("source/variants").unwrap(), variant.root);
        }
        match case {
            "claim" => header["claim"] = json!(pin("claim")),
            "missing-claim" => {
                header.as_object_mut().unwrap().remove("claim");
            }
            "runtime-count" => header["runtime"] = json!(1),
            "source-binding" => header["binding"] = json!(pin("other binding").sha256),
            "job-field" => header["future"] = json!(true),
            "schema" => header["schema"] = json!("future-job-schema"),
            "output-entry" => {
                job_entries.insert(LogicalPath::parse("output/foreign").unwrap(), pin("output"));
            }
            _ => (),
        }
        let header = store
            .put(&identity::canonical_json(&header).unwrap())
            .unwrap();
        job_entries.insert(LogicalPath::parse("job/identity").unwrap(), header.clone());
        let selection = Selection {
            root: Tree::build(&job_entries, &mut store, TreeLimits::default())
                .unwrap()
                .0
                .root()
                .clone(),
            context: header.sha256,
        };
        let minimum = store.finish().unwrap();
        (temp, selection, minimum, primary, control)
    }
    #[test]
    fn historical_role_roster_preserves_inputs_and_variants_without_claiming_a_new_recipe() {
        for case in ["base", "variant"] {
            let (temp, selection, minimum, primary, control) = fixture(case);
            let mut reader = MetadataReader::open(&temp.path().join("metadata"), &minimum).unwrap();
            let inputs = owned_inputs(&selection, &mut reader).unwrap();
            assert_eq!(inputs.selection(), &selection);
            assert!(inputs.of(Role::Primary).contains(&primary));
            assert!(inputs.of(Role::Control).contains(&control));
            assert!(inputs.of(Role::ClaimedOutput).is_empty());
            if case == "variant" {
                assert!(inputs.of(Role::Interpretation).contains(&control));
            }
        }
    }
    #[test]
    fn malformed_or_output_bearing_historical_contexts_refuse() {
        for case in [
            "context-count",
            "context-field",
            "context-entry",
            "role-key",
            "claim",
            "missing-claim",
            "runtime-count",
            "source-binding",
            "job-field",
            "schema",
            "output-entry",
            "variant-source",
        ] {
            let (temp, selection, minimum, _, _) = fixture(case);
            let mut reader = MetadataReader::open(&temp.path().join("metadata"), &minimum).unwrap();
            assert!(owned_inputs(&selection, &mut reader).is_err(), "{case}");
        }
    }
}

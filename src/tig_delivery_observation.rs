//! Private metadata reconciliation, never remote currency, byte custody or approval.
//! Every match starts with the existing Tig retained-receipt checker. Filename,
//! timestamp and equal-length guesses never establish a target or byte identity.
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_assets::{
    graph,
    native::Instance,
    publication::{Checker, ParsedReceipt},
    schema::{Dialect, Schema},
};
use tig_core::{Digest, LogicalPath, Object, identity};
use tig_git::source::{GitId, Repository, Scope};
use tig_store::existing::{ByteSource, ReadCache};

const MAX_INPUT: usize = 16 * 1024 * 1024;
const MAX_ROWS: usize = 65_536;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalFile {
    path: PathBuf,
    sha256: Digest,
    bytes: u64,
}
impl LocalFile {
    fn read(&self, max: usize) -> Result<Vec<u8>> {
        if !self.path.is_absolute() || self.bytes > max as u64 {
            bail!("local metadata budget");
        }
        let mut raw = Vec::new();
        fs::File::open(&self.path)?
            .take(self.bytes + 1)
            .read_to_end(&mut raw)?;
        if Object::from_bytes(&raw)
            != (Object {
                sha256: self.sha256.clone(),
                bytes: self.bytes,
            })
        {
            bail!("local metadata pin mismatch");
        }
        Ok(raw)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    provider: String,
    scope: String,
    definition_id: String,
    /// Exact receipt policies already admitted by the producing project.
    policies: Vec<String>,
    /// Literal prefixes. Neither case folding nor basename matching is performed.
    observation_prefix: String,
    receipt_prefix: String,
    #[serde(default)]
    backslash_separator: bool,
    #[serde(default)]
    opaque_locator: bool,
    #[serde(default)]
    correspondence: Option<Correspondence>,
    #[serde(skip)]
    exact_targets: Option<BTreeMap<String, Digest>>,
}
/// A pinned external receipt supplies exact path-to-target associations. It
/// supplies no publication authority; the ordinary receipt checker still runs.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Correspondence {
    table: LocalFile,
    rows_pointer: String,
    observed_path_pointer: String,
    receipt_target_pointer: String,
    observed_path_prefix: String,
    #[serde(default)]
    backslash_separator: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    source: crate::tig_inputs::Profile,
    inventory: LocalFile,
    policies: BTreeMap<String, LocalFile>,
    mappings: Vec<Mapping>,
    /// Exact basenames only; project support files are not implicitly excluded.
    client_metadata_basenames: Vec<String>,
    max_report_bytes: usize,
}
#[derive(Deserialize)]
struct Observed {
    relative_path: String,
    bytes: u64,
}
struct Binding {
    id: String,
    target: Digest,
    subject: Object,
    report: Value,
}

fn target_identity(mapping: &Mapping, path: &str) -> Result<Digest> {
    let value = if mapping.opaque_locator {
        if path.trim().is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
            bail!("invalid opaque target");
        }
        json!({"schema":"tig.declared-publication-locator.experimental-1", "provider":mapping.provider,"scope":mapping.scope,"locator":path})
    } else {
        LogicalPath::parse(path)?;
        json!({"schema":"tig.declared-publication-target.experimental-1", "provider":mapping.provider,"scope":mapping.scope,"path":path})
    };
    Ok(Object::from_bytes(&identity::canonical_json(&value)?).sha256)
}
fn pointer_valid(value: &str) -> bool {
    if value.len() > 1024 || (!value.is_empty() && !value.starts_with('/')) {
        return false;
    }
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c.is_control() || (c == '~' && !matches!(chars.next(), Some('0' | '1'))) {
            return false;
        }
    }
    true
}
fn bound_table(value: &Value, depth: usize, nodes: &mut usize) -> Result<()> {
    *nodes += 1;
    if depth > 32 || *nodes > 262_144 {
        bail!("correspondence structural budget");
    }
    match value {
        Value::Array(items) => {
            for value in items {
                bound_table(value, depth + 1, nodes)?;
            }
        }
        Value::Object(items) => {
            for value in items.values() {
                bound_table(value, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn prepare_correspondence(
    mapping: &mut Mapping,
    remaining_bytes: &mut u64,
    remaining_rows: &mut usize,
) -> Result<()> {
    let Some(c) = &mapping.correspondence else {
        return Ok(());
    };
    if !mapping.observation_prefix.is_empty()
        || !mapping.receipt_prefix.is_empty()
        || mapping.backslash_separator
        || c.observed_path_prefix.len() > 4096
        || c.observed_path_prefix.chars().any(char::is_control)
        || [
            &c.rows_pointer,
            &c.observed_path_pointer,
            &c.receipt_target_pointer,
        ]
        .iter()
        .any(|p| !pointer_valid(p))
        || c.table.bytes > *remaining_bytes
    {
        bail!("invalid correspondence mapping");
    }
    let raw = c.table.read(4 * 1024 * 1024)?;
    *remaining_bytes -= raw.len() as u64;
    crate::approval_attestation::ensure_no_duplicate_keys(&raw)
        .map_err(|_| anyhow::anyhow!("ambiguous correspondence JSON"))?;
    let value: Value = serde_json::from_slice(&raw)?;
    bound_table(&value, 0, &mut 0)?;
    let rows = value
        .pointer(&c.rows_pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("correspondence rows must be an array"))?;
    if rows.is_empty() || rows.len() > *remaining_rows {
        bail!("correspondence row budget");
    }
    *remaining_rows -= rows.len();
    let mut targets = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for row in rows {
        let path = row
            .pointer(&c.observed_path_pointer)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("correspondence path missing"))?;
        let path = path
            .strip_prefix(&c.observed_path_prefix)
            .ok_or_else(|| anyhow::anyhow!("correspondence path outside declared prefix"))?;
        let path = if c.backslash_separator {
            path.replace('\\', "/")
        } else {
            path.to_owned()
        };
        LogicalPath::parse(&path)?;
        let locator = row
            .pointer(&c.receipt_target_pointer)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("correspondence target missing"))?;
        let digest = target_identity(mapping, locator)?;
        if targets.insert(path, digest.clone()).is_some() || !identities.insert(digest) {
            bail!("duplicate correspondence path or target");
        }
    }
    mapping.exact_targets = Some(targets);
    Ok(())
}
fn target(mapping: &Mapping, observed: &str) -> Result<Option<Digest>> {
    if mapping.correspondence.is_some() {
        return Ok(mapping
            .exact_targets
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("correspondence not prepared"))?
            .get(observed)
            .cloned());
    }
    let Some(suffix) = observed.strip_prefix(&mapping.observation_prefix) else {
        return Ok(None);
    };
    let suffix = if mapping.backslash_separator {
        suffix.replace('/', "\\")
    } else {
        suffix.to_owned()
    };
    let path = format!("{}{suffix}", mapping.receipt_prefix);
    Ok(Some(target_identity(mapping, &path)?))
}
fn reconcile(
    observed: &[Observed],
    mappings: &[Mapping],
    bindings: &[Binding],
    excluded: &BTreeSet<String>,
) -> Result<Value> {
    let mut by_target: BTreeMap<&Digest, Vec<&Binding>> = BTreeMap::new();
    for binding in bindings {
        by_target.entry(&binding.target).or_default().push(binding);
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut matched_ids = BTreeSet::new();
    let mut rows = Vec::new();
    for row in observed {
        let basename = row.relative_path.rsplit('/').next().unwrap_or("");
        let mut matches = BTreeMap::new();
        if !excluded.contains(basename) {
            for mapping in mappings {
                if let Some(digest) = target(mapping, &row.relative_path)? {
                    for binding in by_target.get(&digest).into_iter().flatten() {
                        matches.insert(&binding.id, *binding);
                    }
                }
            }
        }
        let state = if excluded.contains(basename) {
            "client-metadata"
        } else if matches.is_empty() {
            "unmatched"
        } else if matches.len() > 1 {
            "ambiguous"
        } else if matches.values().next().unwrap().subject.bytes != row.bytes {
            "length-differs"
        } else {
            "retained-target-length-matches"
        };
        *counts.entry(state).or_default() += 1;
        let ids: Vec<_> = matches.keys().copied().collect();
        matched_ids.extend(ids.iter().map(|id| (*id).clone()));
        rows.push(json!({"relative_path":row.relative_path,"observed_bytes":row.bytes,"classification":state,"candidate_ids":ids}));
    }
    let absent: Vec<_> = bindings
        .iter()
        .filter(|b| !matched_ids.contains(&b.id))
        .map(|b| &b.id)
        .collect();
    Ok(json!({"counts":counts,"rows":rows,"unobserved_binding_ids":absent}))
}

pub fn run(profile_path: &Path, output: &Path) -> Result<Value> {
    if output.exists() {
        bail!("report already exists");
    }
    let mut raw = Vec::new();
    fs::File::open(profile_path)?
        .take(65_537)
        .read_to_end(&mut raw)?;
    if raw.len() > 65_536 {
        bail!("profile budget");
    }
    let mut p: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    if p.schema != "reel.tig-delivery-observation.experimental-1"
        || p.mappings.is_empty()
        || p.mappings.len() > 128
        || p.policies.is_empty()
        || p.policies.len() > 128
        || !(1024..=64 * 1024 * 1024).contains(&p.max_report_bytes)
        || p.source.schema != crate::tig_inputs::PROFILE_SCHEMA
        || !p.source.admitted.is_empty()
        || p.client_metadata_basenames.len() > 32
    {
        bail!("unsupported reconciliation profile");
    }
    let mut remaining_bytes = MAX_INPUT as u64;
    let mut remaining_rows = MAX_ROWS;
    for mapping in &mut p.mappings {
        prepare_correspondence(mapping, &mut remaining_bytes, &mut remaining_rows)?;
    }
    let excluded: BTreeSet<_> = p.client_metadata_basenames.iter().cloned().collect();
    if excluded.len() != p.client_metadata_basenames.len()
        || excluded
            .iter()
            .any(|s| s.is_empty() || s.contains(['/', '\\']) || s.chars().any(char::is_control))
    {
        bail!("invalid client metadata policy");
    }
    let inventory = p.inventory.read(MAX_INPUT)?;
    let mut paths = BTreeSet::new();
    let mut observations = Vec::new();
    for line in inventory.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        if line.len() > 65_536 || observations.len() >= MAX_ROWS {
            bail!("inventory budget");
        }
        let row: Observed = serde_json::from_value(identity::parse_json(line)?)?;
        LogicalPath::parse(&row.relative_path)?;
        if !paths.insert(row.relative_path.clone()) {
            bail!("duplicate observed path");
        }
        observations.push(row);
    }
    if observations.is_empty() {
        bail!("empty observation");
    }
    let scope = |paths: Vec<String>| -> Result<Scope> {
        if paths.is_empty() || paths.len() > 64 {
            bail!("source scope budget");
        }
        Ok(Scope::new(
            paths
                .iter()
                .map(|s| LogicalPath::parse(s))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        )?)
    };
    let repository = Repository::new(
        p.source.git_executable,
        p.source.repository,
        Default::default(),
    )?;
    let view = repository.commit(GitId::parse(&p.source.commit)?)?;
    let mut snapshot = repository.native(
        &view,
        scope(p.source.records)?,
        scope(p.source.definitions)?,
        p.source.limits,
    )?;
    let checkpoint = snapshot.checkpoint().clone();
    let mut cache = ReadCache::new(p.source.byte_source)?;
    let policy_files = p
        .policies
        .iter()
        .map(|(name, file)| Ok((name.clone(), file.read(65_536)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut policies = Vec::new();
    for mapping in &p.mappings {
        if mapping.provider.is_empty()
            || mapping.scope.is_empty()
            || mapping.definition_id.is_empty()
            || mapping.policies.is_empty()
            || mapping.policies.len() > 32
            || mapping.observation_prefix.len() > 4096
            || mapping.receipt_prefix.len() > 4096
        {
            bail!("mapping budget");
        }
        policies.push(
            mapping
                .policies
                .iter()
                .map(|name| {
                    policy_files
                        .get(name)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("unknown receipt policy"))
                })
                .collect::<Result<Vec<_>>>()?,
        );
    }
    let records: Vec<_> = snapshot.selected_records().cloned().collect();
    let mut bindings = Vec::new();
    let mut failures = Vec::new();
    let mut receipts: BTreeMap<Object, Vec<u8>> = BTreeMap::new();
    let mut parsed_receipts: BTreeMap<Object, ParsedReceipt> = BTreeMap::new();
    for record in records {
        let Some(authored_target) = record.authored().get("target") else {
            continue;
        };
        let Some(provider) = authored_target.get("provider").and_then(Value::as_str) else {
            continue;
        };
        let Some(scope) = authored_target.get("scope").and_then(Value::as_str) else {
            continue;
        };
        let applicable: Vec<_> = p
            .mappings
            .iter()
            .zip(&policies)
            .filter(|(m, _)| {
                m.provider == provider
                    && m.scope == scope
                    && m.definition_id == record.definition().id
            })
            .collect();
        if applicable.is_empty() {
            failures.push(json!({"id":record.reference().id,"reason":"unmapped-target-policy"}));
            continue;
        }
        let lookup = graph::Source::lookup(&mut snapshot, record.reference(), 3 * 65_536);
        let graph::Data::Record {
            instance,
            definition,
            schema,
        } = lookup.data
        else {
            bail!("captured metadata unavailable");
        };
        let bound = Instance::parse(&instance)?.bind(&definition, &schema)?;
        let engine = Schema::compile(bound.definition(), &schema, Dialect::Draft202012)?;
        let checked = engine.check(&bound)?;
        let mut reports = BTreeMap::new();
        let mut attempt_errors = BTreeSet::new();
        // Several explicit locator mappings may share one receipt policy. They
        // affect inventory matching, not how often its evidence must be parsed.
        let candidates: BTreeMap<_, _> = applicable
            .into_iter()
            .flat_map(|(_, candidates)| candidates)
            .map(|policy| (Object::from_bytes(policy), policy))
            .collect();
        for policy in candidates.values() {
            let checker = match Checker::new(&bound, &checked, policy) {
                Ok(checker) => checker,
                Err(error) => {
                    attempt_errors.insert(format!("policy:{error}"));
                    continue;
                }
            };
            let pin = checker.receipt().clone();
            if !receipts.contains_key(&pin) {
                if pin.bytes > MAX_INPUT as u64 || receipts.len() >= 4096 {
                    bail!("receipt budget");
                }
                let mut bytes = Vec::new();
                match cache.open_verified(&pin).and_then(|held| {
                    held.take(pin.bytes + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| tig_store::existing::Error::Io)?;
                    Ok(())
                }) {
                    Ok(()) if Object::from_bytes(&bytes) == pin => {
                        receipts.insert(pin.clone(), bytes);
                    }
                    Ok(()) => {
                        attempt_errors.insert("receipt:Pin".to_owned());
                        continue;
                    }
                    Err(error) => {
                        attempt_errors.insert(format!("receipt:{error}"));
                        continue;
                    }
                }
            }
            if !parsed_receipts.contains_key(&pin) {
                let parsed = match checker.parse_receipt(&receipts[&pin]) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        attempt_errors.insert(format!("parse:{error}"));
                        continue;
                    }
                };
                parsed_receipts.insert(pin.clone(), parsed);
            }
            // Tig rechecks every checker-specific structural limit and pin
            // against the immutable parsed receipt, including on reuse.
            match checker.check_parsed(&parsed_receipts[&pin]) {
                Ok(report) => {
                    reports.insert(report.declared_target_identity.clone(), report);
                }
                Err(error) => {
                    attempt_errors.insert(format!("binding:{error}"));
                }
            }
        }
        if reports.len() != 1 {
            failures.push(json!({"id":record.reference().id,"reason":if reports.is_empty(){"receipt-binding-unverified"}else{"ambiguous-receipt-policies"},"attempt_errors":attempt_errors}));
            continue;
        }
        let (target, report) = reports.into_iter().next().unwrap();
        bindings.push(Binding {
            id: record.reference().id.clone(),
            target,
            subject: report.subject.clone(),
            report: serde_json::to_value(report)?,
        });
    }
    let result = reconcile(&observations, &p.mappings, &bindings, &excluded)?;
    let summary = json!({"observed_files":observations.len(),"verified_retained_bindings":bindings.len(),"binding_gaps":failures.len(),"counts":result["counts"],"current_remote_content_verified":false,"payload_verified":false,"authorizes_eviction":false});
    let report = json!({"schema":"reel.tig-delivery-observation-report.experimental-1","profile":Object::from_bytes(&raw),"source_checkpoint":checkpoint,"source_commit":p.source.commit,"inventory":Object::from_bytes(&inventory),"summary":summary,"reconciliation":result,"binding_gaps":failures,"bindings":bindings.iter().map(|b| json!({"id":b.id,"check":b.report})).collect::<Vec<_>>(),"private_report":true});
    let encoded = serde_json::to_vec(&report)?;
    if encoded.len() > p.max_report_bytes {
        bail!("report budget");
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mapping() -> Mapping {
        Mapping {
            provider: "files".into(),
            scope: "review".into(),
            definition_id: "output".into(),
            policies: vec![],
            observation_prefix: "Shelf/".into(),
            receipt_prefix: "".into(),
            backslash_separator: false,
            opaque_locator: false,
            correspondence: None,
            exact_targets: None,
        }
    }
    fn binding(id: &str, mapping: &Mapping, path: &str, bytes: u64) -> Binding {
        Binding {
            id: id.into(),
            target: target(mapping, path).unwrap().unwrap(),
            subject: Object {
                sha256: Digest::blob(b"bytes"),
                bytes,
            },
            report: json!({}),
        }
    }
    #[test]
    fn correspondence_preserves_opaque_ids_exact_paths_and_shared_budgets() {
        let temp = tempfile::tempdir().unwrap();
        let raw = br#"{"rows":[{"path":"Root/display.png","id":"opaque-provider-id"}]}"#;
        let path = temp.path().join("receipt.json");
        fs::write(&path, raw).unwrap();
        let pin = Object::from_bytes(raw);
        let mut m = mapping();
        m.observation_prefix.clear();
        m.opaque_locator = true;
        m.correspondence = Some(Correspondence {
            table: LocalFile {
                path,
                sha256: pin.sha256,
                bytes: pin.bytes,
            },
            rows_pointer: "/rows".into(),
            observed_path_pointer: "/path".into(),
            receipt_target_pointer: "/id".into(),
            observed_path_prefix: "Root/".into(),
            backslash_separator: false,
        });
        assert!(target(&m, "display.png").is_err());
        assert!(prepare_correspondence(&mut m, &mut (pin.bytes - 1), &mut 1).is_err());
        let mut available_bytes = pin.bytes;
        assert!(prepare_correspondence(&mut m, &mut available_bytes, &mut 0).is_err());
        let mut bytes = pin.bytes;
        let mut rows = 1;
        prepare_correspondence(&mut m, &mut bytes, &mut rows).unwrap();
        assert_eq!((bytes, rows), (0, 0));
        let expected = Object::from_bytes(
            &identity::canonical_json(&json!({
                "schema":"tig.declared-publication-locator.experimental-1",
                "provider":"files","scope":"review","locator":"opaque-provider-id"
            }))
            .unwrap(),
        )
        .sha256;
        assert_eq!(target(&m, "display.png").unwrap(), Some(expected));
        assert!(target(&m, "other/display.png").unwrap().is_none());
        assert!(target(&m, "Display.png").unwrap().is_none());
    }
    #[test]
    fn exact_target_not_basename_or_length_establishes_match() {
        let m = mapping();
        let bindings = vec![binding("film", &m, "Shelf/a/film.mp4", 9)];
        let rows = vec![
            Observed {
                relative_path: "Shelf/a/film.mp4".into(),
                bytes: 9,
            },
            Observed {
                relative_path: "Shelf/b/film.mp4".into(),
                bytes: 9,
            },
        ];
        let r = reconcile(&rows, &[m], &bindings, &BTreeSet::new()).unwrap();
        assert_eq!(r["counts"]["retained-target-length-matches"], 1);
        assert_eq!(r["counts"]["unmatched"], 1);
    }
    #[test]
    fn replacement_length_and_competing_anchors_remain_gaps() {
        let m = mapping();
        let mut bindings = vec![binding("a", &m, "Shelf/film.mp4", 9)];
        let rows = vec![Observed {
            relative_path: "Shelf/film.mp4".into(),
            bytes: 10,
        }];
        assert_eq!(
            reconcile(&rows, std::slice::from_ref(&m), &bindings, &BTreeSet::new()).unwrap()["counts"]
                ["length-differs"],
            1
        );
        bindings.push(binding("b", &m, "Shelf/film.mp4", 10));
        assert_eq!(
            reconcile(&rows, &[m], &bindings, &BTreeSet::new()).unwrap()["counts"]["ambiguous"],
            1
        );
    }
    #[test]
    fn only_explicit_client_metadata_is_excluded_and_case_is_preserved() {
        let m = mapping();
        let rows = vec![
            Observed {
                relative_path: "Shelf/desktop.ini".into(),
                bytes: 1,
            },
            Observed {
                relative_path: "Shelf/README.txt".into(),
                bytes: 1,
            },
            Observed {
                relative_path: "shelf/film.mp4".into(),
                bytes: 1,
            },
        ];
        let r = reconcile(&rows, &[m], &[], &BTreeSet::from(["desktop.ini".into()])).unwrap();
        assert_eq!(r["counts"]["client-metadata"], 1);
        assert_eq!(r["counts"]["unmatched"], 2);
    }
}

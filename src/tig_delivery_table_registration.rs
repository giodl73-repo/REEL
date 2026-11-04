//! Initial retained-file anchor candidates from an explicitly mapped immutable
//! table. Not a version-update, payload-availability or production adapter.
use crate::tig_native_registration::{DefinitionFiles, Prepared, read_local, read_metadata};
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};
use tig_assets::{
    native::{Definition, Instance},
    publication::Checker,
    schema::{Dialect, Schema},
};
use tig_core::{LogicalPath, Object, identity};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

#[derive(Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RowLayout {
    #[default]
    Array,
    Object,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    #[default]
    LogicalPath,
    OpaqueLocator,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub byte_source: ByteProfile,
    pub receipt: Object,
    pub rows_pointer: String,
    #[serde(default)]
    pub row_layout: RowLayout,
    pub asset_pointer: String,
    pub path_pointer: String,
    /// Object member names can themselves be the declared target paths.
    #[serde(default)]
    pub target_from_member_key: bool,
    #[serde(default)]
    pub target_kind: TargetKind,
    pub provider: String,
    pub scope: String,
    pub id_prefix: String,
    pub definition: DefinitionFiles,
    pub receipt_policy: crate::scene_delivery::FileRef,
    pub authored: Map<String, Value>,
    /// Exact original source/package evidence. No rights/approval is inferred.
    pub provenance: BTreeMap<String, Object>,
    pub max_rows: usize,
    pub max_output_bytes: u64,
}
fn pointer(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if !value.starts_with('/') || value.len() > 1024 {
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
fn token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

pub fn prepare(p: &Profile, source: &mut impl ByteSource) -> Result<Prepared> {
    if p.schema != "reel.tig-delivery-table-registration.experimental-1"
        || !(1..=4096).contains(&p.max_rows)
        || !(1..=16 * 1024 * 1024).contains(&p.max_output_bytes)
        || !token(&p.provider)
        || !token(&p.scope)
        || p.id_prefix.len() > 180
        || !pointer(&p.rows_pointer)
        || !pointer(&p.asset_pointer)
        || !pointer(&p.path_pointer)
        || p.authored.contains_key("target")
        || p.provenance.len() > 16
        || (p.target_from_member_key && p.row_layout != RowLayout::Object)
    {
        bail!("invalid delivery table profile");
    }
    LogicalPath::parse(&p.id_prefix)?;
    for name in p.provenance.keys() {
        LogicalPath::parse(name)?;
    }
    let definition = read_local(&p.definition.definition)?;
    let authored_schema = read_local(&p.definition.authored_schema)?;
    let definition_value = Definition::parse(&definition)?;
    let engine = Schema::compile(&definition_value, &authored_schema, Dialect::Draft202012)?;
    let policy = read_local(&p.receipt_policy)?;
    let mut budget = 4 * 1024 * 1024;
    let raw = read_metadata(source, &p.receipt, &mut budget)?;
    // External ledgers have a separate 4-MiB intake bound. Do not relax Tig's
    // 64-KiB native-record limit just to read one retained receipt table.
    crate::approval_attestation::ensure_no_duplicate_keys(&raw)
        .map_err(|_| anyhow::anyhow!("ambiguous delivery table JSON"))?;
    let receipt: Value = serde_json::from_slice(&raw)?;
    let table = receipt
        .pointer(&p.rows_pointer)
        .ok_or_else(|| anyhow::anyhow!("declared delivery table missing"))?;
    let rows: Vec<(String, &Value)> = match (&p.row_layout, table) {
        (RowLayout::Array, Value::Array(rows)) if rows.len() <= p.max_rows => rows
            .iter()
            .enumerate()
            .map(|(i, row)| (i.to_string(), row))
            .collect(),
        (RowLayout::Object, Value::Object(rows)) if rows.len() <= p.max_rows => {
            rows.iter().map(|(key, row)| (key.clone(), row)).collect()
        }
        _ => bail!("declared row layout or budget differs"),
    };
    if rows.is_empty() {
        bail!("delivery row budget");
    }
    let mut records = BTreeMap::new();
    let mut targets = BTreeSet::new();
    let mut object_lengths = BTreeMap::new();
    let mut total = 0u64;
    let mut parsed_receipt = None;
    for (member, row) in &rows {
        let path = if p.target_from_member_key {
            member.as_str()
        } else {
            row.pointer(&p.path_pointer)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("declared target path missing"))?
        };
        let identity = match p.target_kind {
            TargetKind::LogicalPath => {
                LogicalPath::parse(path)?;
                json!({"schema":"tig.declared-publication-target.experimental-1","provider":p.provider,"scope":p.scope,"path":path})
            }
            TargetKind::OpaqueLocator => {
                if path.trim().is_empty() || path.len() > 4096 || path.chars().any(char::is_control)
                {
                    bail!("invalid opaque target locator");
                }
                json!({"schema":"tig.declared-publication-locator.experimental-1","provider":p.provider,"scope":p.scope,"locator":path})
            }
        };
        if !targets.insert(path) {
            bail!("duplicate delivery target path");
        }
        let value = row
            .pointer(&p.asset_pointer)
            .ok_or_else(|| anyhow::anyhow!("declared subject missing"))?;
        let asset: Object =
            serde_json::from_value(json!({"sha256":value["sha256"],"bytes":value["bytes"]}))?;
        if object_lengths
            .insert(asset.sha256.clone(), asset.bytes)
            .is_some_and(|length| length != asset.bytes)
        {
            bail!("one content hash has conflicting declared lengths");
        }
        if let Some(uri) = value.get("cache_uri")
            && uri.as_str() != Some(format!("cache://sha256/{}", asset.sha256.as_str()).as_str())
        {
            bail!("source cache URI differs from subject");
        }
        let target = Object::from_bytes(&identity::canonical_json(&identity)?).sha256;
        // Identity follows the declared target, never row number or media bytes.
        let id = format!("{}/{}", p.id_prefix, target.as_str());
        if id.len() > 256 {
            bail!("generated ID budget");
        }
        let mut authored = p.authored.clone();
        let selector_member = member.replace('~', "~0").replace('/', "~1");
        authored.insert("target".into(),json!({"provider":p.provider,"scope":p.scope,"receipt_slot":"delivery-receipt","receipt_selector":format!("{}/{selector_member}",p.rows_pointer)}));
        let mut streams = vec![
            json!({"slot":"retained-readback","role":"delivery/file","group":"retained","object":asset}),
            json!({"slot":"delivery-receipt","role":"evidence/delivery-receipt","group":"source","object":p.receipt}),
        ];
        for (name, object) in &p.provenance {
            streams.push(json!({"slot":format!("evidence/{name}"),"role":"evidence/provenance","group":"source","object":object}));
        }
        let record = json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":definition_value.value().id,"sha256":p.definition.definition.sha256,"bytes":p.definition.definition.bytes},"authored":authored,"selected":asset,"streams":streams,"status":{},"metadata":{"initial_delivery_table_registration":{"receipt":p.receipt,"policy":Object::from_bytes(&policy),"production_claim":null,"approval_claim":null,"payload_verified":false,"current_remote_content_verified":false,"authorizes_eviction":false}}});
        let bytes = serde_json::to_vec(&record)?;
        let bound = Instance::parse(&bytes)?.bind(&definition, &authored_schema)?;
        let checked = engine.check(&bound)?;
        let checker = Checker::new(&bound, &checked, &policy)?;
        if parsed_receipt.is_none() {
            parsed_receipt = Some(checker.parse_receipt(&raw)?);
        }
        let report = checker.check_parsed(parsed_receipt.as_ref().unwrap())?;
        if report.declared_target_identity != target {
            bail!("receipt policy target differs from declared row path");
        }
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("output budget overflow"))?;
        if total > p.max_output_bytes {
            bail!("output byte budget");
        }
        if records
            .insert(format!("{}.tig", target.as_str()), bytes)
            .is_some()
        {
            bail!("target identity collision");
        }
    }
    Ok(Prepared {
        records,
        bytes: total,
    })
}
pub fn run(profile: &Path, output: &Path) -> Result<Value> {
    let mut raw = Vec::new();
    fs::File::open(profile)?
        .take(65_537)
        .read_to_end(&mut raw)?;
    if raw.len() > 65_536 {
        bail!("profile byte budget");
    }
    let p: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    let mut source = ReadCache::new(p.byte_source.clone())?;
    prepare(&p, &mut source)?.write(output)
}

//! Stable component output records from exact native conform metadata.
use crate::{
    episode_conform::{Manifest, SegmentKind},
    tig_native_registration::{DefinitionFiles, Prepared, filename, read_local, read_metadata},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};
use tig_assets::native::{Definition, Instance};
use tig_core::{Object, identity};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

pub const SCHEMA: &str = "reel.tig-conform-components-registration.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub filename: String,
    pub language: String,
    pub segment_id: String,
    pub kind: SegmentKind,
    pub definition: DefinitionFiles,
    pub authored: Map<String, Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub episode_id: String,
    pub byte_source: ByteProfile,
    pub conforms: BTreeMap<String, Object>,
    pub entries: BTreeMap<String, Entry>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    pub max_output_bytes: u64,
}
fn token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn pin(value: &crate::scene_delivery::FileRef) -> Result<Object> {
    Ok(serde_json::from_value(
        json!({"sha256":value.sha256,"bytes":value.bytes}),
    )?)
}
fn remember(sizes: &mut BTreeMap<tig_core::Digest, u64>, object: &Object) -> Result<()> {
    if sizes
        .insert(object.sha256.clone(), object.bytes)
        .is_some_and(|bytes| bytes != object.bytes)
    {
        bail!("component references have contradictory object sizes");
    }
    Ok(())
}
pub fn prepare(profile: &Profile, source: &mut impl ByteSource) -> Result<Prepared> {
    if profile.schema != SCHEMA
        || !token(&profile.episode_id)
        || profile.conforms.is_empty()
        || profile.conforms.len() > 32
        || profile.entries.is_empty()
        || profile.entries.len() > 128
        || profile.max_output_bytes == 0
        || profile.max_output_bytes > 16 * 1024 * 1024
        || profile
            .metadata
            .contains_key("conform_components_registration")
    {
        bail!("unsupported conform registration admission");
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut subjects = BTreeSet::new();
    for (name, entry) in &profile.entries {
        filename(&entry.filename)?;
        if !token(name)
            || !token(&entry.id)
            || !token(&entry.segment_id)
            || !profile.conforms.contains_key(&entry.language)
            || !ids.insert(entry.id.clone())
            || !names.insert(entry.filename.to_ascii_lowercase())
            || !subjects.insert((entry.language.clone(), entry.segment_id.clone()))
            || ["language", "source_segment_id", "component_kind"]
                .iter()
                .any(|key| entry.authored.contains_key(*key))
        {
            bail!("invalid or conflicting component mapping");
        }
    }
    let mut remaining = 16 * 1024 * 1024;
    let mut conforms = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    for (language, object) in &profile.conforms {
        if language.is_empty()
            || language.len() > 32
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            bail!("unsupported conform language tag");
        }
        remember(&mut sizes, object)?;
        let raw = read_metadata(source, object, &mut remaining)?;
        let manifest: Manifest = serde_json::from_value(identity::parse_json(&raw)?)?;
        manifest.validate_clock_and_range()?;
        if manifest.schema != crate::episode_conform::SCHEMA
            || manifest.episode_id != profile.episode_id
            || &manifest.language != language
            || manifest.segments.is_empty()
            || manifest.segments.len() > 128
        {
            bail!("conform scope or segment count differs");
        }
        let mut segments = BTreeSet::new();
        for reference in [
            &manifest.catalog,
            &manifest.master_template_definition,
            &manifest.episode,
            &manifest.season_bindings,
            &manifest.episode_bindings,
        ]
        .into_iter()
        .chain(manifest.source_master_template.iter())
        {
            remember(&mut sizes, &pin(reference)?)?;
        }
        for segment in &manifest.segments {
            if !token(&segment.id) || !segments.insert(segment.id.clone()) {
                bail!("duplicate or invalid conform segment identity");
            }
            for reference in std::iter::once(&segment.master)
                .chain(std::iter::once(&segment.source_receipt))
                .chain(segment.delivery_job.iter())
                .chain(segment.delivery_receipt.iter())
                .chain(segment.semantic_delivery.iter())
                .chain(segment.master_proof.iter())
                .chain(segment.master_proof_validator.iter())
                .chain(segment.adoption_manifest.iter())
                .chain(segment.source_text_evidence.iter())
                .chain(segment.imported_source_manifest.iter())
            {
                remember(&mut sizes, &pin(reference)?)?;
            }
        }
        conforms.insert(language, manifest);
    }
    let mut records = BTreeMap::new();
    let mut total = 0u64;
    for entry in profile.entries.values() {
        let conform = &conforms[&entry.language];
        let segment = conform
            .segments
            .iter()
            .find(|s| s.id == entry.segment_id)
            .context("mapped component segment missing")?;
        if segment.kind != entry.kind {
            bail!("mapped component kind differs");
        }
        let definition = read_local(&entry.definition.definition)?;
        let schema = read_local(&entry.definition.authored_schema)?;
        let declared = Definition::parse(&definition)?;
        let kind = match segment.kind {
            SegmentKind::Scene => "scene",
            SegmentKind::EpisodePresentation => "episode-presentation",
        };
        let mut authored = entry.authored.clone();
        authored.insert("language".into(), json!(entry.language));
        authored.insert("source_segment_id".into(), json!(segment.id));
        authored.insert("component_kind".into(), json!(kind));
        let conform_pin = &profile.conforms[&entry.language];
        let mut streams = vec![
            json!({"slot":"source-conform","role":"evidence/source-conform","group":"source","object":conform_pin}),
        ];
        for (slot, reference) in [
            ("source-receipt", Some(&segment.source_receipt)),
            ("delivery-job", segment.delivery_job.as_ref()),
            ("delivery-receipt", segment.delivery_receipt.as_ref()),
            ("semantic-delivery", segment.semantic_delivery.as_ref()),
            ("master-proof", segment.master_proof.as_ref()),
            (
                "master-proof-validator",
                segment.master_proof_validator.as_ref(),
            ),
            ("adoption-manifest", segment.adoption_manifest.as_ref()),
            (
                "source-text-evidence",
                segment.source_text_evidence.as_ref(),
            ),
            (
                "imported-source-manifest",
                segment.imported_source_manifest.as_ref(),
            ),
        ] {
            if let Some(reference) = reference {
                streams.push(json!({"slot":slot,"role":"evidence/component-source","group":"original-production","object":pin(reference)?}));
            }
        }
        let mut metadata = profile.metadata.clone();
        metadata.insert("conform_components_registration".into(), json!({"conform":conform_pin,
            "episode_id":conform.episode_id,"language":entry.language,"segment_id":segment.id,"component_kind":kind,
            "segment_position":conform.segments.iter().position(|s| s.id == segment.id).unwrap(),
            "source_relationship":"caller-declared desired references; not a captured producer assertion"}));
        let raw = serde_json::to_vec_pretty(
            &json!({"schema":"tig.instance.design-1","id":entry.id,
            "definition":{"id":declared.value().id,"sha256":Object::from_bytes(&definition).sha256,"bytes":definition.len()},
            "authored":authored,"selected":pin(&segment.master)?,"streams":streams,
            "status":{"lifecycle":"registered-native-component","production_state":"unclassified","new_approval":false},"metadata":metadata}),
        )?;
        Instance::parse(&raw)?.bind(&definition, &schema)?;
        total = total
            .checked_add(raw.len() as u64)
            .context("component output size overflow")?;
        if total > profile.max_output_bytes {
            bail!("component candidate set exceeds budget");
        }
        records.insert(format!("inputs/{}", entry.filename), raw);
    }
    Ok(Prepared {
        records,
        bytes: total,
    })
}
fn run_inner(input: &Path, output: &Path) -> Result<Value> {
    let mut raw = Vec::new();
    fs::File::open(input)?.take(65_537).read_to_end(&mut raw)?;
    if raw.len() > 65_536
        || !output.is_absolute()
        || output.symlink_metadata().is_ok()
        || !output.parent().is_some_and(|p| p.is_dir())
    {
        bail!("invalid bounded conform registration destination");
    }
    let profile: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    let mut source = ReadCache::new(profile.byte_source.clone())?;
    let mut result = prepare(&profile, &mut source)?.write(output)?;
    result["schema"] = json!("reel.tig-conform-components-registration-result.experimental-1");
    result["mapped_components"] = json!(profile.entries.len());
    result["payloads_verified"] = json!(false);
    result["authorizes_eviction"] = json!(false);
    Ok(result)
}
pub fn run(input: &Path, output: &Path) -> Result<Value> {
    run_inner(input, output).map_err(|_| anyhow::anyhow!("conform component registration refused; verify exact scopes, mappings, references, definitions, budgets and new destination"))
}

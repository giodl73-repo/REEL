//! Metadata-only intake of retained native still-sequence plans. No media copy,
//! cache publication, Git operation, selection change or execution authority.
use crate::{
    presentation_still_sequence,
    scene_delivery::{self, FileRef, InputFiles},
    tig_inputs::Source,
    tig_native_registration::{self as registration, DefinitionFiles, Prepared},
    tig_presentation::{self, Request},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tig_assets::native::{Definition, Instance};
use tig_assets::resolve::{AssetResolver, Error as ResolveError};
use tig_core::tree::{MemoryNodes, TreeLimits};
use tig_core::{Digest, Object, identity};
use tig_git::source::native::AssetSnapshot;
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

pub const SCHEMA: &str = "reel.tig-presentation-registration.experimental-1";
const ROLES: [&str; 5] = ["source", "template", "timing", "clock", "evidence"];
fn token(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        bail!("invalid presentation intake identity");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub filename: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: String,
    /// Required for a new record; forbidden for an existing immutable binding.
    pub filename: Option<String>,
    #[serde(default)]
    pub existing: bool,
    #[serde(default)]
    pub authored: Map<String, Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub byte_source: ByteProfile,
    pub manifest: Object,
    /// Exactly source/template/timing/clock/evidence/primary. Definition IDs differ.
    pub definitions: BTreeMap<String, DefinitionFiles>,
    /// Exactly the five control roles; unused v1 timing roles emit no records.
    pub nodes: BTreeMap<String, Node>,
    /// Exact used keys: picture/0..N, layer, audio if present, font/0..N.
    pub bindings: BTreeMap<String, Binding>,
    pub primary_selection_state: String,
    #[serde(default)]
    pub existing_source: Option<crate::tig_inputs::Profile>,
    #[serde(default)]
    pub existing_primary_definitions: BTreeSet<String>,
    /// A NEW v1 desired contract can add audio to an original picture-only plan.
    /// Historical manifest/receipt objects remain unchanged. No audio bytes open.
    pub extra_audio: Option<Object>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    pub max_output_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativePin {
    path: PathBuf,
    sha256: String,
    bytes: u64,
    cache_uri: String,
}
fn pin(value: &Value) -> Result<FileRef> {
    let p: NativePin = serde_json::from_value(value.clone())?;
    let digest = Digest::parse(&p.sha256)?;
    if p.cache_uri != format!("cache://sha256/{}", digest.as_str()) {
        bail!("native reference URI differs");
    }
    scene_delivery::control_name(&p.path)?;
    Ok(FileRef {
        path: p.path,
        sha256: p.sha256,
        bytes: p.bytes,
    })
}
fn object(pin: &FileRef) -> Result<Object> {
    Ok(Object {
        sha256: Digest::parse(&pin.sha256)?,
        bytes: pin.bytes,
    })
}
struct Controls(BTreeMap<PathBuf, (FileRef, Vec<u8>)>);
impl InputFiles for Controls {
    fn verified_path(&self, _: &FileRef) -> Result<PathBuf> {
        bail!("intake metadata has no path fallback")
    }
    fn control_bytes(&self, pin: &FileRef) -> Result<Vec<u8>> {
        let (expected, raw) = self
            .0
            .get(&pin.path)
            .context("native metadata is not enrolled")?;
        if expected.sha256 != pin.sha256 || expected.bytes != pin.bytes {
            bail!("native metadata binding differs");
        }
        Ok(raw.clone())
    }
}
fn load(
    source: &mut impl ByteSource,
    controls: &mut Controls,
    value: &Value,
    remaining: &mut u64,
) -> Result<Value> {
    enroll(source, controls, value, remaining)?;
    identity::parse_json(&controls.control_bytes(&pin(value)?)?).map_err(Into::into)
}
fn enroll(
    source: &mut impl ByteSource,
    controls: &mut Controls,
    value: &Value,
    remaining: &mut u64,
) -> Result<Object> {
    let pin = pin(value)?;
    let expected = object(&pin)?;
    let raw = registration::read_metadata(source, &expected, remaining)?;
    if let Some((old, bytes)) = controls.0.get(&pin.path) {
        if old.sha256 != pin.sha256 || old.bytes != pin.bytes || bytes != &raw {
            bail!("native metadata aliases conflict");
        }
    } else {
        controls.0.insert(pin.path.clone(), (pin, raw));
    }
    Ok(expected)
}
struct Type {
    id: String,
    definition: Vec<u8>,
    schema: Vec<u8>,
}
struct Builder<'a> {
    profile: &'a Profile,
    types: BTreeMap<String, Type>,
    existing: Option<Existing>,
    prepared: Prepared,
    ids: BTreeMap<String, (Object, Map<String, Value>, String)>,
    filenames: BTreeSet<String>,
    used: BTreeSet<String>,
}
struct Existing {
    source: Source,
    identities: AssetSnapshot,
}
impl Existing {
    fn open(profile: crate::tig_inputs::Profile) -> Result<Self> {
        let limits = profile.limits;
        let source = Source::open(profile)?;
        let mut nodes = MemoryNodes::default();
        let tree = TreeLimits::default();
        let archive = source.archive_metadata(&mut nodes, tree)?;
        let identities = AssetSnapshot::from_metadata_archive(&mut nodes, &archive, limits, tree)?;
        if identities.checkpoint() != source.checkpoint() {
            bail!("existing identity view changed");
        }
        Ok(Self { source, identities })
    }
    fn contains(&self, id: &str) -> Result<bool> {
        match self.identities.resolve_id(id) {
            Ok(_) => Ok(true),
            Err(ResolveError::Missing) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}
impl Builder<'_> {
    fn record(
        &mut self,
        role: &str,
        id: &str,
        filename: &str,
        authored: Value,
        selected: Option<Object>,
    ) -> Result<()> {
        token(id)?;
        registration::filename(filename)?;
        let path = if role == "primary" {
            format!("inputs/{filename}")
        } else {
            filename.into()
        };
        if !self.filenames.insert(path.to_ascii_lowercase()) {
            bail!("presentation record filenames collide");
        }
        let kind = &self.types[role];
        let definition = Object::from_bytes(&kind.definition);
        let mut metadata = self.profile.metadata.clone();
        metadata.insert("native_presentation_registration".into(), json!({"source_manifest":self.profile.manifest,"audio_extension":self.profile.extra_audio}));
        let status = if role == "primary" {
            json!({"lifecycle":"registered-native-input","selection_state":self.profile.primary_selection_state})
        } else {
            json!({})
        };
        let bytes = serde_json::to_vec_pretty(
            &json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":kind.id,"sha256":definition.sha256,"bytes":definition.bytes},"authored":authored,"selected":selected,"streams":[],"status":status,"metadata":metadata}),
        )?;
        if bytes.len() > 65_536 {
            bail!("literal presentation record exceeds 64 KiB; retain large metadata as an asset");
        }
        Instance::parse(&bytes)?.bind(&kind.definition, &kind.schema)?;
        self.prepared.bytes = self
            .prepared
            .bytes
            .checked_add(bytes.len() as u64)
            .context("intake byte overflow")?;
        if self.prepared.bytes > self.profile.max_output_bytes {
            bail!("presentation records exceed budget");
        }
        self.prepared.records.insert(path, bytes);
        Ok(())
    }
    fn control(&mut self, role: &str, document: Value) -> Result<Value> {
        let node = &self.profile.nodes[role];
        let id = node.id.clone();
        let filename = node.filename.clone();
        self.record(role, &id, &filename, json!({"document":document}), None)?;
        Ok(json!({"id":id}))
    }
    fn evidence(&mut self, original: Object) -> Result<Value> {
        let node = &self.profile.nodes["evidence"];
        let id = node.id.clone();
        let filename = node.filename.clone();
        self.record("evidence", &id, &filename, json!({"document":{"schema":"reel.presentation-clock-evidence-reference.v1","original":original}}), Some(original))?;
        Ok(json!({"id":id}))
    }
    fn primary(&mut self, key: &str, value: &Value) -> Result<Value> {
        self.used.insert(key.into());
        let binding = self
            .profile
            .bindings
            .get(key)
            .context("missing explicit primary identity")?;
        token(&binding.id)?;
        scene_delivery::control_name(Path::new(&binding.id))?;
        if self
            .profile
            .nodes
            .values()
            .any(|node| node.id == binding.id)
        {
            bail!("primary and control identities collide");
        }
        let selected = object(&pin(value)?)?;
        if binding.existing {
            if binding.filename.is_some() || !binding.authored.is_empty() {
                bail!("existing primary cannot carry replacement fields");
            }
            let source = self
                .existing
                .as_ref()
                .context("existing primary requires captured source")?;
            let record = source.source.record(&binding.id)?;
            let asset = source.source.resolve_asset(&binding.id)?;
            if !self
                .profile
                .existing_primary_definitions
                .contains(&record.definition().id)
                || asset.sha256 != selected.sha256.as_str()
                || asset.bytes != selected.bytes
                || asset.selection_state != self.profile.primary_selection_state
            {
                bail!("existing primary differs from explicit admission");
            }
        } else {
            if self
                .existing
                .as_ref()
                .map(|source| source.contains(&binding.id))
                .transpose()?
                .unwrap_or(false)
            {
                bail!("new primary identity already exists");
            }
            let filename = binding
                .filename
                .as_ref()
                .context("new primary requires portable filename")?;
            let data = (selected.clone(), binding.authored.clone(), filename.clone());
            if let Some(previous) = self.ids.get(&binding.id) {
                if previous != &data {
                    bail!("shared primary identity has conflicting data");
                }
            } else {
                self.ids.insert(binding.id.clone(), data);
                self.record(
                    "primary",
                    &binding.id,
                    filename,
                    Value::Object(binding.authored.clone()),
                    Some(selected),
                )?;
            }
        }
        Ok(json!({"id":binding.id}))
    }
}

/// Read only bounded native controls. Bind every candidate to exact supplied
/// definitions; primary pins come solely from the original native references.
pub fn prepare(profile: &Profile, source: &mut impl ByteSource) -> Result<(Prepared, Request)> {
    let expected = ROLES
        .into_iter()
        .chain(["primary"])
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if profile.schema != SCHEMA
        || profile.definitions.keys().cloned().collect::<BTreeSet<_>>() != expected
        || profile.nodes.keys().cloned().collect::<BTreeSet<_>>()
            != ROLES.into_iter().map(str::to_owned).collect()
        || profile.bindings.len() > 138
        || profile.max_output_bytes == 0
        || profile.max_output_bytes > 16 * 1024 * 1024
        || profile
            .metadata
            .contains_key("native_presentation_registration")
    {
        bail!("invalid presentation intake admission");
    }
    token(&profile.primary_selection_state)?;
    if profile.primary_selection_state.len() > 128 {
        bail!("selection state exceeds budget");
    }
    let mut types = BTreeMap::new();
    let mut type_ids = BTreeSet::new();
    for (role, files) in &profile.definitions {
        let definition = registration::read_local(&files.definition)?;
        let schema = registration::read_local(&files.authored_schema)?;
        let id = Definition::parse(&definition)?.value().id.clone();
        if !type_ids.insert(id.clone()) {
            bail!("presentation intake definitions overlap");
        }
        types.insert(
            role.clone(),
            Type {
                id,
                definition,
                schema,
            },
        );
    }
    let mut node_ids = BTreeSet::new();
    for node in profile.nodes.values() {
        token(&node.id)?;
        registration::filename(&node.filename)?;
        if !node_ids.insert(&node.id) {
            bail!("presentation control identities collide");
        }
    }
    if profile.existing_primary_definitions.len() > 31
        || profile
            .existing_primary_definitions
            .iter()
            .any(|id| type_ids.contains(id) && id != &types["primary"].id)
    {
        bail!("existing primary definitions overlap controls");
    }
    for id in &profile.existing_primary_definitions {
        token(id)?;
    }
    let existing = profile
        .existing_source
        .clone()
        .map(Existing::open)
        .transpose()?;
    if let Some(source) = &existing {
        for node in profile.nodes.values() {
            if source.contains(&node.id)? {
                bail!("new control identity already exists");
            }
        }
    }
    let mut remaining = 16 * 1024 * 1024;
    let raw = registration::read_metadata(source, &profile.manifest, &mut remaining)?;
    let mut native = identity::parse_json(&raw)?;
    if let Some(audio) = &profile.extra_audio {
        if native["schema"] != "reel.presentation-still-sequence.v1"
            || native.get("audio").is_some_and(|value| !value.is_null())
        {
            bail!("audio extension requires an original picture-only v1 plan");
        }
        native["audio"] = json!({"path":"added/audio","sha256":audio.sha256,"bytes":audio.bytes,"cache_uri":format!("cache://sha256/{}",audio.sha256.as_str())});
    }
    let mut controls = Controls(BTreeMap::new());
    let template = load(source, &mut controls, &native["template"], &mut remaining)?;
    let timing = native
        .get("picture_timing")
        .map(|value| load(source, &mut controls, value, &mut remaining))
        .transpose()?;
    let clock = timing
        .as_ref()
        .map(|value| load(source, &mut controls, &value["clock"], &mut remaining))
        .transpose()?;
    let evidence = clock
        .as_ref()
        .map(|value| enroll(source, &mut controls, &value["evidence"], &mut remaining))
        .transpose()?;
    presentation_still_sequence::validate_metadata(&identity::canonical_json(&native)?, &controls)?;
    token(
        native["episode_id"]
            .as_str()
            .context("native episode identity missing")?,
    )?;
    token(native["role"].as_str().context("native role missing")?)?;
    let mut builder = Builder {
        profile,
        types,
        existing,
        prepared: Prepared {
            records: BTreeMap::new(),
            bytes: 0,
        },
        ids: BTreeMap::new(),
        filenames: BTreeSet::new(),
        used: BTreeSet::new(),
    };
    let template = builder.control("template", template)?;
    let pictures = native["pictures"]
        .as_array()
        .context("native pictures missing")?
        .iter()
        .enumerate()
        .map(|(i, value)| builder.primary(&format!("picture/{i}"), value))
        .collect::<Result<Vec<_>>>()?;
    let layer = builder.primary("layer", &native["editable_layer"])?;
    let audio = native
        .get("audio")
        .map(|value| builder.primary("audio", value))
        .transpose()?;
    let fonts = native
        .get("fonts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, value)| builder.primary(&format!("font/{i}"), value))
        .collect::<Result<Vec<_>>>()?;
    let mut document = json!({"schema":"reel.tig-still-sequence-source.v1","episode_id":native["episode_id"],"language":native["language"],"role":native["role"],"template":template,"pictures":pictures,"editable_layer":layer,"fonts":fonts});
    if let Some(audio) = audio {
        document["audio"] = audio;
    }
    if let Some(treatment) = native.get("audio_treatment") {
        document["audio_treatment"] = treatment.clone();
    }
    if let (Some(timing), Some(clock), Some(evidence)) = (timing, clock, evidence) {
        let evidence = builder.evidence(evidence)?;
        let clock = builder.control("clock", json!({"schema":clock["schema"],"source":document["audio"],"source_at_measurement":object(&pin(&native["audio"])?)?,"sample_rate":clock["sample_rate"],"total_samples":clock["total_samples"],"evidence":evidence,"anchors":clock["anchors"]}))?;
        document["picture_timing"] = builder.control(
            "timing",
            json!({"schema":timing["schema"],"clock":clock,"spans":timing["spans"]}),
        )?;
    }
    builder.control("source", document)?;
    if builder.used != profile.bindings.keys().cloned().collect() {
        bail!("unused primary binding in presentation intake");
    }
    let mut primary_definitions = profile.existing_primary_definitions.clone();
    primary_definitions.insert(builder.types["primary"].id.clone());
    let request = Request {
        schema: tig_presentation::SCHEMA.into(),
        presentation_id: profile.nodes["source"].id.clone(),
        source_definition: builder.types["source"].id.clone(),
        template_definition: builder.types["template"].id.clone(),
        timing_definition: builder.types["timing"].id.clone(),
        clock_definition: builder.types["clock"].id.clone(),
        evidence_definition: builder.types["evidence"].id.clone(),
        primary_definitions,
        allowed_primary_states: BTreeSet::from([profile.primary_selection_state.clone()]),
    };
    Ok((builder.prepared, request))
}

pub fn run(profile_path: &Path, output: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(profile_path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        bail!("presentation intake profile exceeds budget");
    }
    let profile: Profile = serde_json::from_value(identity::parse_json(&bytes)?)?;
    let mut source = ReadCache::new(profile.byte_source.clone())?;
    let (prepared, request) = prepare(&profile, &mut source)?;
    let mut result = prepared.write(output)?;
    result["schema"] = "reel.tig-presentation-registration-result.experimental-1".into();
    result["source_request"] = serde_json::to_value(request)?;
    Ok(result)
}

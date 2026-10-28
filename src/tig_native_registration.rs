//! Bounded native source registration. Does not read/copy primary media or select it.
use crate::scene_delivery::FileRef;
use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{self, AssetRef, Scene};
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
};
use tig_assets::native::{Definition, Instance, Stream};
use tig_core::{Digest, Object};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

pub const SCHEMA: &str = "reel.tig-native-registration.experimental-1";
const DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
const TOTAL_BYTES: u64 = 16 * 1024 * 1024;
mod scopes;
mod variants;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingSource {
    pub asset: String,
    pub scope_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionFiles {
    pub definition: FileRef,
    pub authored_schema: FileRef,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub byte_source: ByteProfile,
    pub package: Object,
    pub scene_asset: String,
    pub bindings_asset: String,
    #[serde(default)]
    pub language_bindings_assets: BTreeMap<String, String>,
    #[serde(default)]
    pub language_filenames: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub binding_ancestors: Vec<BindingSource>,
    #[serde(default)]
    pub language_binding_ancestors: BTreeMap<String, Vec<BindingSource>>,
    pub scene_definition: DefinitionFiles,
    pub primary_definition: DefinitionFiles,
    pub controls: Vec<Stream>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    /// Project-specific extra fields, for example a declared media kind.
    #[serde(default)]
    pub primary_annotations: BTreeMap<String, Map<String, Value>>,
    /// Explicit one-component filenames for binding keys not portable as names.
    #[serde(default)]
    pub filenames: BTreeMap<String, String>,
    pub max_output_bytes: u64,
}
fn unique_map<'de, D, T>(d: D) -> std::result::Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Visitor<T> {
        type Value = BTreeMap<String, T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("bounded map with unique binding keys")
        }
        fn visit_map<A: de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, T>()? {
                if values.len() == 4096 || values.insert(key, value).is_some() {
                    return Err(de::Error::custom(
                        "duplicate key or binding budget exceeded",
                    ));
                }
            }
            Ok(values)
        }
    }
    d.deserialize_map(Visitor(std::marker::PhantomData))
}
#[derive(Deserialize)]
pub(crate) struct Package {
    pub(crate) schema: String,
    pub(crate) artifact_id: String,
    pub(crate) version_id: String,
    #[serde(deserialize_with = "unique_map")]
    pub(crate) assets: BTreeMap<String, PackagePin>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackagePin {
    sha256: Digest,
    bytes: u64,
    cache_uri: String,
}
impl PackagePin {
    pub(crate) fn object(&self) -> Result<Object> {
        if self.cache_uri != format!("cache://sha256/{}", self.sha256.as_str()) {
            bail!("package asset URI differs from identity");
        }
        Ok(Object {
            sha256: self.sha256.clone(),
            bytes: self.bytes,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    schema: String,
    scope_id: String,
    #[serde(deserialize_with = "unique_map")]
    assets: BTreeMap<String, AssetRef>,
}
pub(crate) fn read_local(file: &FileRef) -> Result<Vec<u8>> {
    if !file.path.is_absolute() || file.bytes > 65_536 {
        bail!("explicit bounded absolute definition file required");
    }
    let mut bytes = Vec::new();
    fs::File::open(&file.path)?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    if Object::from_bytes(&bytes)
        != (Object {
            sha256: Digest::parse(&file.sha256)?,
            bytes: file.bytes,
        })
    {
        bail!("definition file identity differs");
    }
    Ok(bytes)
}
pub(crate) fn read_metadata(
    source: &mut impl ByteSource,
    pin: &Object,
    remaining: &mut u64,
) -> Result<Vec<u8>> {
    if pin.bytes > DOCUMENT_BYTES || pin.bytes > *remaining {
        bail!("registration metadata exceeds budget");
    }
    *remaining -= pin.bytes;
    let mut bytes = Vec::new();
    source
        .open_verified(pin)?
        .take(pin.bytes + 1)
        .read_to_end(&mut bytes)?;
    if Object::from_bytes(&bytes) != *pin {
        bail!("registration metadata identity differs");
    }
    Ok(bytes)
}
pub(crate) fn filename(name: &str) -> Result<()> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if name.is_empty()
        || name.len() > 128
        || !name.ends_with(".tig")
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || reserved
    {
        bail!("binding requires an explicit portable .tig filename");
    }
    Ok(())
}
pub struct Prepared {
    pub(crate) records: BTreeMap<String, Vec<u8>>,
    pub(crate) bytes: u64,
}
impl Prepared {
    /// Candidate source files only; no overwrite, Git, cache publication or adoption.
    pub fn write(&self, output: &Path) -> Result<Value> {
        fs::create_dir(output).context("registration requires a new output directory")?;
        fs::create_dir(output.join("inputs"))?;
        for (path, bytes) in &self.records {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output.join(path))?
                .write_all(bytes)?;
        }
        Ok(
            json!({"schema":"reel.tig-native-registration-result.experimental-1",
            "records":self.records.len(),"record_bytes":self.bytes,"primary_media_opened":0,
            "complete_semantic_coverage":false,"production_claim":null,"adopted":false}),
        )
    }
}
fn record(value: Value, definition: &[u8], schema: &[u8]) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(&value)?;
    // Bind also checks exact definition/schema pins and the declared authored fields.
    Instance::parse(&bytes)?.bind(definition, schema)?;
    Ok(bytes)
}
/// Reuse a supplied verified provider; only package, scene and binding bytes open.
pub fn prepare(profile: &Profile, source: &mut impl ByteSource) -> Result<Prepared> {
    if profile.schema != SCHEMA
        || profile.max_output_bytes == 0
        || profile.max_output_bytes > TOTAL_BYTES
    {
        bail!("unsupported registration profile or output budget");
    }
    let scene_definition = read_local(&profile.scene_definition.definition)?;
    let scene_schema = read_local(&profile.scene_definition.authored_schema)?;
    let primary_definition = read_local(&profile.primary_definition.definition)?;
    let primary_schema = read_local(&profile.primary_definition.authored_schema)?;
    let scene_type = Definition::parse(&scene_definition)?;
    let primary_type = Definition::parse(&primary_definition)?;
    if scene_type.value().id == primary_type.value().id {
        bail!("scene and primary definitions must differ");
    }
    let mut remaining = TOTAL_BYTES;
    let package: Package =
        serde_json::from_slice(&read_metadata(source, &profile.package, &mut remaining)?)?;
    if package.schema != "caminos.artifact-package.v1"
        || package.artifact_id.is_empty()
        || package.version_id.is_empty()
    {
        bail!("unsupported native source package");
    }
    let pin = |key: &str| {
        package
            .assets
            .get(key)
            .context("source package asset missing")?
            .object()
    };
    let scene_pin = pin(&profile.scene_asset)?;
    let bindings_pin = pin(&profile.bindings_asset)?;
    let scene: Scene = serde_json::from_slice(&read_metadata(source, &scene_pin, &mut remaining)?)?;
    let mut bindings: Bindings =
        serde_json::from_slice(&read_metadata(source, &bindings_pin, &mut remaining)?)?;
    let inherited = scopes::merge(
        &profile.binding_ancestors,
        &package,
        &mut bindings,
        source,
        &mut remaining,
    )?;
    if ![
        scene_authoring::SCENE_SCHEMA,
        scene_authoring::SCENE_SCHEMA_V2,
    ]
    .contains(&scene.schema.as_str())
        || bindings.schema != scene_authoring::BINDINGS_SCHEMA
        || bindings.scope_id != scene.scene_id
        || bindings.assets.is_empty()
    {
        bail!("native scene/binding scope differs");
    }
    if profile
        .primary_annotations
        .keys()
        .chain(profile.filenames.keys())
        .any(|key| !bindings.assets.contains_key(key))
    {
        bail!("registration annotation names an unknown binding");
    }
    if profile.metadata.contains_key("native_registration") {
        bail!("registration provenance is reserved");
    }
    let mut metadata = profile.metadata.clone();
    metadata.insert(
        "native_registration".into(),
        json!({"package":profile.package,
        "artifact_id":package.artifact_id,"version_id":package.version_id,"bindings":bindings_pin}),
    );
    if !inherited.is_empty() {
        metadata["native_registration"]["binding_ancestors"] = json!(inherited);
    }
    let definition_pin =
        |id: &str, bytes: &[u8]| json!({"id":id,"sha256":Digest::blob(bytes),"bytes":bytes.len()});
    let mut records = BTreeMap::new();
    let mut references = Map::new();
    let mut ids = BTreeSet::from([scene.scene_id.clone()]);
    let mut names = BTreeSet::new();
    let mut sizes = BTreeMap::new();
    let mut total = 0u64;
    for (key, asset) in &bindings.assets {
        if !ids.insert(asset.logical_id.clone()) {
            bail!("duplicate native asset identity");
        }
        let object = Object {
            sha256: Digest::parse(&asset.sha256)?,
            bytes: asset.bytes,
        };
        if asset.cache_uri != format!("cache://sha256/{}", asset.sha256)
            || asset.selection_state.is_empty()
            || sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|n| n != object.bytes)
        {
            bail!("native asset identity or selection state is inconsistent");
        }
        let name = profile
            .filenames
            .get(key)
            .cloned()
            .unwrap_or_else(|| format!("{key}.tig"));
        filename(&name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            bail!("registration filenames collide");
        }
        let mut authored = json!({"scene_id":scene.scene_id,"binding_key":key,"source_package":profile.package,"public_release":false}).as_object().unwrap().clone();
        if let Some(extra) = profile.primary_annotations.get(key) {
            if extra.keys().any(|k| authored.contains_key(k)) {
                bail!("annotation overrides native identity");
            }
            authored.extend(extra.clone());
        }
        let bytes = record(
            json!({"schema":"tig.instance.design-1","id":asset.logical_id,
            "definition":definition_pin(&primary_type.value().id, &primary_definition),"authored":authored,
            "selected":object,"streams":[],"status":{"lifecycle":"registered-native-input","selection_state":asset.selection_state},"metadata":metadata}),
            &primary_definition,
            &primary_schema,
        )?;
        total = total
            .checked_add(bytes.len() as u64)
            .context("registration byte overflow")?;
        if total > profile.max_output_bytes {
            bail!("registration output exceeds budget");
        }
        records.insert(format!("inputs/{name}"), bytes);
        references.insert(key.clone(), json!({"id":asset.logical_id}));
    }
    let variants = variants::prepare(
        profile,
        &package,
        &scene,
        &bindings,
        source,
        &mut remaining,
        &records,
    )?;
    for (path, bytes) in variants.records {
        total = total
            .checked_add(bytes.len() as u64)
            .context("registration byte overflow")?;
        if total > profile.max_output_bytes {
            bail!("registration output exceeds budget");
        }
        records.insert(path, bytes);
    }
    let mut parent_authored = json!({
        "scene_id":scene.scene_id,"episode_id":scene.episode_id,"source_package":profile.package,
        "binding_ids":references,"public_release":false
    });
    if !variants.overrides.is_empty() {
        parent_authored["language_binding_ids"] = json!(variants.overrides);
    }
    if !variants.pins.is_empty() {
        metadata["native_registration"]["language_bindings"] = json!(variants.pins);
    }
    if !variants.ancestors.is_empty() {
        metadata["native_registration"]["language_binding_ancestors"] = json!(variants.ancestors);
    }
    let parent = record(
        json!({"schema":"tig.instance.design-1","id":scene.scene_id,
        "definition":definition_pin(&scene_type.value().id,&scene_definition),"authored":parent_authored,
        "selected":scene_pin,"streams":profile.controls,
        "status":{"lifecycle":"registered-native-source","native_authoring_state":scene.authoring_state},"metadata":metadata}),
        &scene_definition,
        &scene_schema,
    )?;
    total = total
        .checked_add(parent.len() as u64)
        .context("registration byte overflow")?;
    if total > profile.max_output_bytes {
        bail!("registration output exceeds budget");
    }
    records.insert("scene.tig".into(), parent);
    Ok(Prepared {
        records,
        bytes: total,
    })
}
pub fn run(profile: &Path, output: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(profile)?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    let profile: Profile = serde_json::from_value(tig_core::identity::parse_json(&bytes)?)?;
    let mut source = ReadCache::new(profile.byte_source.clone())?;
    prepare(&profile, &mut source)?.write(output)
}

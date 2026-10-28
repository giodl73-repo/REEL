//! Reconcile explicit language binding sources without opening primary media.
use super::*;

pub(super) struct Variants {
    pub records: BTreeMap<String, Vec<u8>>,
    pub overrides: Map<String, Value>,
    pub pins: BTreeMap<String, Object>,
    pub ancestors: BTreeMap<String, Vec<Value>>,
}
pub(super) fn prepare(
    profile: &Profile,
    package: &Package,
    scene: &Scene,
    base: &Bindings,
    source: &mut impl ByteSource,
    remaining: &mut u64,
    existing: &BTreeMap<String, Vec<u8>>,
) -> Result<Variants> {
    let mut result = Variants {
        records: BTreeMap::new(),
        overrides: Map::new(),
        pins: BTreeMap::new(),
        ancestors: BTreeMap::new(),
    };
    if profile.language_bindings_assets.len() > 32
        || profile
            .language_filenames
            .keys()
            .any(|tag| !profile.language_bindings_assets.contains_key(tag))
        || profile
            .language_binding_ancestors
            .keys()
            .any(|tag| !profile.language_bindings_assets.contains_key(tag))
    {
        bail!("unsupported language binding sources");
    }
    if profile.language_bindings_assets.is_empty() {
        return Ok(result);
    }
    for names in profile.language_filenames.values() {
        for name in names.values() {
            filename(name)?;
        }
    }
    let definition = read_local(&profile.primary_definition.definition)?;
    let schema = read_local(&profile.primary_definition.authored_schema)?;
    let declared = Definition::parse(&definition)?;
    let mut identities = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    for (key, asset) in &base.assets {
        let asset = &identities::resolve(profile, base, key, asset, None)?;
        identities
            .entry(asset.logical_id.clone())
            .or_insert((key.clone(), serde_json::to_value(asset)?));
        sizes.insert(asset.sha256.clone(), asset.bytes);
    }
    let mut names: BTreeSet<_> = existing
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    let mut count = base.assets.len();
    let mut added_bytes = 0u64;
    for (tag, role) in &profile.language_bindings_assets {
        if tag.is_empty()
            || tag.len() > 32
            || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || !scene.languages.contains_key(tag)
        {
            bail!("binding source language is absent or invalid");
        }
        let pin = package
            .assets
            .get(role)
            .context("language bindings asset missing")?
            .object()?;
        let mut bindings: Bindings =
            serde_json::from_slice(&read_metadata(source, &pin, remaining)?)?;
        let inherited = scopes::merge(
            profile
                .language_binding_ancestors
                .get(tag)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            package,
            &mut bindings,
            source,
            remaining,
        )?;
        if !inherited.is_empty() {
            result.ancestors.insert(tag.clone(), inherited.clone());
        }
        count = count
            .checked_add(bindings.assets.len())
            .context("binding count overflow")?;
        if bindings.schema != scene_authoring::BINDINGS_SCHEMA
            || bindings.scope_id != scene.scene_id
            || bindings.assets.is_empty()
            || count > 4096
            || bindings
                .assets
                .keys()
                .any(|key| !base.assets.contains_key(key))
            || profile
                .language_filenames
                .get(tag)
                .is_some_and(|names| names.keys().any(|key| !bindings.assets.contains_key(key)))
            || profile
                .language_primary_ids
                .get(tag)
                .is_some_and(|ids| ids.keys().any(|key| !bindings.assets.contains_key(key)))
        {
            bail!("language binding scope, aliases or budget differ");
        }
        result.pins.insert(tag.clone(), pin.clone());
        let mut overrides = Map::new();
        for (key, asset) in &bindings.assets {
            let original = asset;
            let asset = &identities::resolve(profile, base, key, asset, Some(tag))?;
            let value = serde_json::to_value(asset)?;
            if asset.logical_id == scene.scene_id {
                bail!("primary identity collides with scene");
            }
            let object = Object {
                sha256: Digest::parse(&asset.sha256)?,
                bytes: asset.bytes,
            };
            if asset.cache_uri != format!("cache://sha256/{}", asset.sha256)
                || asset.selection_state.is_empty()
                || sizes
                    .insert(asset.sha256.clone(), asset.bytes)
                    .is_some_and(|n| n != asset.bytes)
            {
                bail!("language asset identity or state is inconsistent");
            }
            if let Some((previous_key, previous)) = identities.get(&asset.logical_id) {
                if previous != &value
                    || profile.primary_annotations.get(previous_key)
                        != profile.primary_annotations.get(key)
                {
                    bail!("shared identity has conflicting bindings");
                }
            } else {
                identities.insert(asset.logical_id.clone(), (key.clone(), value));
                let name = profile
                    .language_filenames
                    .get(tag)
                    .and_then(|names| names.get(key))
                    .cloned()
                    .unwrap_or_else(|| format!("{key}.{tag}.tig"));
                filename(&name)?;
                let path = format!("inputs/{name}");
                if !names.insert(path.to_ascii_lowercase()) {
                    bail!("variant filename collision");
                }
                let mut authored = json!({"scene_id":scene.scene_id,"binding_key":key,"source_package":profile.package,"public_release":false}).as_object().unwrap().clone();
                if let Some(extra) = profile.primary_annotations.get(key) {
                    if extra.keys().any(|key| authored.contains_key(key)) {
                        bail!("annotation overrides native identity");
                    }
                    authored.extend(extra.clone());
                }
                let mut metadata = profile.metadata.clone();
                metadata.insert("native_registration".into(), json!({"package":profile.package,
                    "artifact_id":package.artifact_id,"version_id":package.version_id,"bindings":pin,"language":tag}));
                if !inherited.is_empty() {
                    metadata["native_registration"]["binding_ancestors"] = json!(inherited);
                }
                identities::provenance(&mut metadata, original, asset);
                let bytes = record(
                    json!({"schema":"tig.instance.design-1","id":asset.logical_id,
                    "definition":{"id":declared.value().id,"sha256":Object::from_bytes(&definition).sha256,"bytes":definition.len()},
                    "authored":authored,"selected":object,"streams":[],"status":{"lifecycle":"registered-native-input","selection_state":asset.selection_state},"metadata":metadata}),
                    &definition,
                    &schema,
                )?;
                added_bytes = added_bytes
                    .checked_add(bytes.len() as u64)
                    .context("variant byte overflow")?;
                if added_bytes > profile.max_output_bytes {
                    bail!("variant outputs exceed budget");
                }
                result.records.insert(path, bytes);
            }
            if asset.logical_id
                != identities::resolve(profile, base, key, &base.assets[key], None)?.logical_id
            {
                overrides.insert(key.clone(), json!({"id":asset.logical_id}));
            }
        }
        if !overrides.is_empty() {
            result
                .overrides
                .insert(tag.clone(), Value::Object(overrides));
        }
    }
    Ok(result)
}

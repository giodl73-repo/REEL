//! Experimental design-1 envelope reader; raw byte identity, no normalization.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use tig_core::{
    Digest, Object,
    identity::{Error, Result, parse_json},
};

const MAX_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAX_RECORD_BYTES: usize = 65_536;

/// Wire pin. An ID is a logical name, never a physical provider path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub sha256: Digest,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

impl Pin {
    pub fn object(&self) -> Object {
        Object {
            sha256: self.sha256.clone(),
            bytes: self.bytes,
        }
    }

    /// Checks supplied exact bytes, not availability or retained custody elsewhere.
    pub fn verify(&self, bytes: &[u8]) -> Result<()> {
        self.validate()?;
        require(
            self.object() == Object::from_bytes(bytes),
            "byte pin mismatch",
        )
    }

    fn validate(&self) -> Result<()> {
        require(self.bytes <= MAX_INTEGER, "unsafe pin size")?;
        if let Some(id) = &self.id {
            token(id, 256)?;
        }
        if let Some(media) = &self.media_type {
            require(
                media.len() <= 128 && !media.chars().any(char::is_control),
                "invalid media type",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stream {
    pub slot: String,
    pub role: String,
    pub group: String,
    pub object: Pin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub schema: String,
    pub id: String,
    pub definition: Pin,
    pub authored: Map<String, Value>,
    pub streams: Vec<Stream>,
    pub selected: Option<Pin>,
    pub status: Map<String, Value>,
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authoring {
    pub properties: Vec<String>,
    pub required: Vec<String>,
    pub defaults: Map<String, Value>,
}

/// Definitions are separately pinned; rules remain data, never executable hooks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema: String,
    pub id: String,
    pub authored_schema: Pin,
    pub authoring: Authoring,
    pub stream_rules: Map<String, Value>,
    pub semantic_rules: Map<String, Value>,
}

/// Holds the identity of the exact received bytes, including whitespace.
/// Read-only access prevents mutating the parsed value behind that identity.
#[derive(Clone, Debug)]
pub struct Parsed<T> {
    value: T,
    object: Object,
}

impl<T> Parsed<T> {
    pub fn value(&self) -> &T {
        &self.value
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
}

impl Instance {
    pub fn parse(bytes: &[u8]) -> Result<Parsed<Self>> {
        let parsed = decode::<Self>(bytes)?;
        parsed.value.validate()?;
        Ok(parsed)
    }

    fn validate(&self) -> Result<()> {
        require(
            self.schema == "tig.instance.design-1",
            "unsupported instance schema",
        )?;
        token(&self.id, 256)?;
        self.definition.validate()?;
        require(self.definition.id.is_some(), "definition ID required")?;
        require(self.streams.len() <= 256, "stream count budget exceeded")?;
        let mut slots = BTreeSet::new();
        for stream in &self.streams {
            token(&stream.slot, 128)?;
            token(&stream.role, 128)?;
            token(&stream.group, 128)?;
            require(slots.insert(&stream.slot), "duplicate stream slot")?;
            stream.object.validate()?;
        }
        if let Some(selected) = &self.selected {
            selected.validate()?;
        }
        // A single digest cannot consistently describe different byte lengths.
        let mut sizes = std::collections::BTreeMap::new();
        for pin in std::iter::once(&self.definition)
            .chain(self.streams.iter().map(|s| &s.object))
            .chain(self.selected.iter())
        {
            if let Some(size) = sizes.insert(&pin.sha256, pin.bytes) {
                require(size == pin.bytes, "conflicting sizes for digest")?;
            }
        }
        Ok(())
    }
}

impl Definition {
    pub fn parse(bytes: &[u8]) -> Result<Parsed<Self>> {
        let parsed = decode::<Self>(bytes)?;
        let value = &parsed.value;
        require(
            value.schema == "tig.definition.design-1",
            "unsupported definition schema",
        )?;
        token(&value.id, 256)?;
        value.authored_schema.validate()?;
        require(
            value.authored_schema.id.is_none() && value.authored_schema.media_type.is_none(),
            "authored schema requires byte-only pin",
        )?;
        let properties = unique_fields(&value.authoring.properties)?;
        let required = unique_fields(&value.authoring.required)?;
        require(
            required.is_subset(&properties),
            "required property not declared",
        )?;
        require(
            value
                .authoring
                .defaults
                .keys()
                .all(|k| properties.contains(k)),
            "default property not declared",
        )?;
        Ok(parsed)
    }
}

impl Parsed<Instance> {
    /// Binds exact definition and authored-schema bytes. Checks only envelope and
    /// authoring field declarations; schema/rule meaning requires an admitted engine.
    /// Defaults are never silently applied to the authored instance.
    pub fn bind(&self, definition: &[u8], authored_schema: &[u8]) -> Result<BoundInstance> {
        self.value.definition.verify(definition)?;
        let definition = Definition::parse(definition)?;
        require(
            self.value.definition.id.as_deref() == Some(&definition.value.id),
            "definition ID mismatch",
        )?;
        definition.value.authored_schema.verify(authored_schema)?;
        for pin in self
            .value
            .streams
            .iter()
            .map(|s| &s.object)
            .chain(self.value.selected.iter())
        {
            require(
                pin.sha256 != definition.value.authored_schema.sha256
                    || pin.bytes == definition.value.authored_schema.bytes,
                "conflicting authored-schema digest size",
            )?;
        }
        let schema = parse_json(authored_schema)?;
        require(
            schema.is_object() || schema.is_boolean(),
            "invalid authored schema shape",
        )?;
        let fields = &definition.value.authoring;
        require(
            self.value
                .authored
                .keys()
                .all(|k| fields.properties.contains(k)),
            "undeclared authored property",
        )?;
        require(
            fields
                .required
                .iter()
                .all(|k| self.value.authored.contains_key(k)),
            "missing required authored property",
        )?;
        Ok(BoundInstance {
            instance: self.clone(),
            definition,
        })
    }
}

/// Exact binding with deliberately incomplete semantic/reference coverage.
/// It cannot be used as complete build, approval, retention or eviction evidence.
#[derive(Clone, Debug)]
pub struct BoundInstance {
    instance: Parsed<Instance>,
    definition: Parsed<Definition>,
}

impl BoundInstance {
    pub fn instance(&self) -> &Parsed<Instance> {
        &self.instance
    }
    pub fn definition(&self) -> &Parsed<Definition> {
        &self.definition
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<Parsed<T>> {
    // Shared strict reader rejects duplicate keys, floats, depth/node/byte overflow.
    let json = parse_json(bytes)?;
    // Optional wire fields can be absent, but explicit null is not their type.
    // serde's Option alone would erase this distinction from the JSON Schemas.
    for key in ["definition", "selected", "authored_schema"] {
        if let Some(pin) = json.get(key) {
            pin_optionals(pin)?;
        }
    }
    if let Some(streams) = json.get("streams").and_then(Value::as_array) {
        for stream in streams {
            if let Some(pin) = stream.get("object") {
                pin_optionals(pin)?;
            }
            require(
                stream.get("provenance").is_none_or(Value::is_object),
                "provenance must be an object when present",
            )?;
        }
    }
    // serde treats missing Option as None; the wire contract requires selected.
    if json.get("schema").and_then(Value::as_str) == Some("tig.instance.design-1") {
        require(
            json.get("selected").is_some(),
            "selected field required (null while planned)",
        )?;
    }
    let value =
        serde_json::from_value(json).map_err(|_| Error("invalid native envelope".into()))?;
    Ok(Parsed {
        value,
        object: Object::from_bytes(bytes),
    })
}

fn pin_optionals(pin: &Value) -> Result<()> {
    for field in ["id", "media_type"] {
        require(
            pin.get(field).is_none_or(Value::is_string),
            "optional pin field must be a string when present",
        )?;
    }
    Ok(())
}

fn unique_fields(fields: &[String]) -> Result<BTreeSet<String>> {
    require(fields.len() <= 256, "authoring field budget exceeded")?;
    let mut set = BTreeSet::new();
    for field in fields {
        token(field, 256)?;
        require(set.insert(field.clone()), "duplicate authoring field")?;
    }
    Ok(set)
}

fn token(value: &str, max: usize) -> Result<()> {
    require(
        !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control),
        "invalid logical token",
    )
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}

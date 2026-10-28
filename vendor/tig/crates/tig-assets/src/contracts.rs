//! Explicit local assertions only, never complete native or custody semantics.
use crate::{
    native::{BoundInstance, Definition, Parsed},
    schema::{self, Dialect, Schema},
};
use jsonschema::Validator;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tig_core::{Object, identity::canonical_json};

const WIRE: &str = "tig.local-json-schema-rules.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    schema: String,
    dialect: String,
    assertion: Value,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unspecified,
    Declaration,
    Dialect,
    Json,
    Schema(schema::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

fn rule(map: &Map<String, Value>, dialect: Dialect) -> Result<Validator, Error> {
    if map.is_empty() {
        return Err(Error::Unspecified);
    }
    let rule: Rule =
        serde_json::from_value(Value::Object(map.clone())).map_err(|_| Error::Declaration)?;
    if rule.schema != WIRE {
        return Err(Error::Declaration);
    }
    if rule
        .dialect
        .parse::<Dialect>()
        .map_err(|_| Error::Dialect)?
        != dialect
    {
        return Err(Error::Dialect);
    }
    schema::compile_value(&rule.assertion, dialect).map_err(Error::Schema)
}

pub struct Contract {
    authored: Schema,
    streams: Validator,
    record: Validator,
    policy: Object,
    policy_bytes: Vec<u8>,
}
impl Contract {
    pub fn compile(
        definition: &Parsed<Definition>,
        authored_schema: &[u8],
        dialect: Dialect,
    ) -> Result<Self, Error> {
        let authored =
            Schema::compile(definition, authored_schema, dialect).map_err(Error::Schema)?;
        let streams = rule(&definition.value().stream_rules, dialect)?;
        let record = rule(&definition.value().semantic_rules, dialect)?;
        let policy_bytes =
            canonical_json(&json!({"schema":"tig.local-contract-policy.experimental-1",
            "authored_policy":authored.policy(),"rules_wire":WIRE,
            "streams_projection":"parsed-ordered-streams","record_projection":"parsed-instance"}))
            .map_err(|_| Error::Json)?;
        Ok(Self {
            authored,
            streams,
            record,
            policy: Object::from_bytes(&policy_bytes),
            policy_bytes,
        })
    }
    pub fn policy(&self) -> &Object {
        &self.policy
    }
    pub fn policy_bytes(&self) -> &[u8] {
        &self.policy_bytes
    }
    pub fn check(&self, bound: &BoundInstance) -> Result<Checked, Error> {
        let authored = self.authored.check(bound).map_err(Error::Schema)?;
        let streams =
            serde_json::to_value(&bound.instance().value().streams).map_err(|_| Error::Json)?;
        let record = serde_json::to_value(bound.instance().value()).map_err(|_| Error::Json)?;
        schema::check_value(&self.streams, &streams).map_err(Error::Schema)?;
        schema::check_value(&self.record, &record).map_err(Error::Schema)?;
        Ok(Checked {
            authored,
            policy: self.policy.clone(),
        })
    }
}

#[derive(Debug)]
pub struct Checked {
    authored: schema::Checked,
    policy: Object,
}
impl Checked {
    pub fn instance(&self) -> &Object {
        self.authored.instance()
    }
    pub fn definition(&self) -> &Object {
        self.authored.definition()
    }
    pub fn authored_schema(&self) -> &Object {
        self.authored.authored_schema()
    }
    pub fn authored_policy(&self) -> &Object {
        self.authored.policy()
    }
    pub fn policy(&self) -> &Object {
        &self.policy
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
}

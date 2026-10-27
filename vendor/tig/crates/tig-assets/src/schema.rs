//! Explicit authored JSON Schema checks, never complete asset semantics/authority.
use crate::native::{BoundInstance, Definition, MAX_RECORD_BYTES, Parsed};
use jsonschema::{Draft, PatternOptions, Validator, error::ValidationErrorKind};
use serde_json::{Value, json};
use tig_core::{
    Object,
    identity::{canonical_json, parse_json},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Draft4,
    Draft6,
    Draft7,
    Draft201909,
    Draft202012,
}
impl Dialect {
    pub fn uri(self) -> &'static str {
        match self {
            Self::Draft4 => "http://json-schema.org/draft-04/schema",
            Self::Draft6 => "http://json-schema.org/draft-06/schema",
            Self::Draft7 => "http://json-schema.org/draft-07/schema",
            Self::Draft201909 => "https://json-schema.org/draft/2019-09/schema",
            Self::Draft202012 => "https://json-schema.org/draft/2020-12/schema",
        }
    }
    fn engine(self) -> Draft {
        match self {
            Self::Draft4 => Draft::Draft4,
            Self::Draft6 => Draft::Draft6,
            Self::Draft7 => Draft::Draft7,
            Self::Draft201909 => Draft::Draft201909,
            Self::Draft202012 => Draft::Draft202012,
        }
    }
}
impl std::str::FromStr for Dialect {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        match value {
            "4" => Ok(Self::Draft4),
            "6" => Ok(Self::Draft6),
            "7" => Ok(Self::Draft7),
            "2019-09" => Ok(Self::Draft201909),
            "2020-12" => Ok(Self::Draft202012),
            _ => Err(Error::Dialect),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Pin,
    Json,
    Dialect,
    Compile,
    InvalidInstance,
    Evaluation,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

/// Reusable exact-definition compiler. No external document retrieval.
pub struct Schema {
    validator: Validator,
    definition: Object,
    schema: Object,
    policy: Object,
    policy_bytes: Vec<u8>,
}
impl Schema {
    pub fn compile(
        definition: &Parsed<Definition>,
        raw: &[u8],
        dialect: Dialect,
    ) -> Result<Self, Error> {
        if raw.len() > MAX_RECORD_BYTES {
            return Err(Error::Json);
        }
        definition
            .value()
            .authored_schema
            .verify(raw)
            .map_err(|_| Error::Pin)?;
        let schema = parse_json(raw).map_err(|_| Error::Json)?;
        let validator = compile_value(&schema, dialect)?;
        let policy_bytes = canonical_json(
            &json!({"schema":"tig.authored-schema-policy.experimental-1",
            "engine":"jsonschema-0.60.2","dialect":dialect.uri(),"external_retrieval":false,
            "assert_formats":true,"ignore_unknown_formats":false,"regex_backtracks":100000,
            "regex_bytes":1048576,"regex_dfa_bytes":1048576}),
        )
        .map_err(|_| Error::Json)?;
        Ok(Self {
            validator,
            definition: definition.object().clone(),
            schema: Object::from_bytes(raw),
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
        if bound.definition().object() != &self.definition {
            return Err(Error::Pin);
        }
        let authored = Value::Object(bound.instance().value().authored.clone());
        check_value(&self.validator, &authored)?;
        Ok(Checked {
            instance: bound.instance().object().clone(),
            definition: self.definition.clone(),
            schema: self.schema.clone(),
            policy: self.policy.clone(),
        })
    }
}

/// Internal callers supply values from the strict bounded native reader.
pub(crate) fn compile_value(value: &Value, dialect: Dialect) -> Result<Validator, Error> {
    if let Some(declared) = value.get("$schema")
        && declared
            .as_str()
            .is_none_or(|s| s.strip_suffix('#').unwrap_or(s) != dialect.uri())
    {
        return Err(Error::Dialect);
    }
    jsonschema::options()
        .with_draft(dialect.engine())
        .offline()
        .should_validate_formats(true)
        .should_ignore_unknown_formats(false)
        .with_pattern_options(
            PatternOptions::fancy_regex()
                .backtrack_limit(100_000)
                .size_limit(1_048_576)
                .dfa_size_limit(1_048_576),
        )
        .build(value)
        .map_err(|_| Error::Compile)
}

pub(crate) fn check_value(validator: &Validator, value: &Value) -> Result<(), Error> {
    validator.validate(value).map_err(|error| {
        if matches!(
            error.kind(),
            ValidationErrorKind::BacktrackLimitExceeded { .. }
                | ValidationErrorKind::RegexEngineFailure { .. }
                | ValidationErrorKind::Referencing(_)
        ) {
            Error::Evaluation
        } else {
            Error::InvalidInstance
        }
    })
}

/// Exact authored-schema check only; no producer, approval or custody capability.
#[derive(Debug)]
pub struct Checked {
    instance: Object,
    definition: Object,
    schema: Object,
    policy: Object,
}
impl Checked {
    pub fn instance(&self) -> &Object {
        &self.instance
    }
    pub fn definition(&self) -> &Object {
        &self.definition
    }
    pub fn authored_schema(&self) -> &Object {
        &self.schema
    }
    pub fn policy(&self) -> &Object {
        &self.policy
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
}

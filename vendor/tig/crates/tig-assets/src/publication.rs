//! Pure retained-receipt binding, never provider identity/currentness or custody.
use crate::{native::BoundInstance, schema::Checked};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tig_core::{Digest, LogicalPath, Object, identity};

mod evidence_json;
mod pointer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Policy,
    Schema,
    Subject,
    Target,
    Selector,
    Pin,
    Evidence,
    Budget,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: String,
    target_pointer: String,
    subject_slot: String,
    subject_role: String,
    #[serde(default)]
    subject_selection: SubjectSelection,
    receipt_role: String,
    limits: Limits,
    rules: Vec<Rule>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SubjectSelection {
    #[default]
    Selected,
    DeclaredStream,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    bytes: usize,
    nodes: usize,
    depth: usize,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Rule {
    SingleRow { root: String, fields: Fields },
    ArrayRows { root: String, fields: Fields },
    ObjectRows { root: String, fields: Fields },
    KeyedHashes { root: String },
}
impl Rule {
    fn root(&self) -> &str {
        match self {
            Self::SingleRow { root, .. }
            | Self::ArrayRows { root, .. }
            | Self::ObjectRows { root, .. }
            | Self::KeyedHashes { root } => root,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    sha256: String,
    bytes: Option<String>,
    target: TargetField,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum TargetField {
    Pointer(String),
    MemberKey(MemberKey),
    OpaqueLocator(OpaqueLocator),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberKey {
    kind: MemberKeyKind,
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MemberKeyKind {
    MemberKey,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpaqueLocator {
    kind: OpaqueLocatorKind,
    pointer: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OpaqueLocatorKind {
    OpaqueLocator,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    provider: String,
    scope: String,
    receipt_slot: String,
    receipt_selector: String,
}

/// An exact schema-checked instance and admitted mapping, ready for one receipt.
/// Debug is intentionally absent: the target can contain private selector keys.
pub struct Checker {
    instance: Object,
    definition: Object,
    authored_schema: Object,
    schema_policy: Object,
    subject_is_selected: Option<bool>,
    inner: ReceiptChecker,
}

/// Private immutable parsed evidence, reusable across anchors without reparsing.
/// Each checker still verifies its exact pin and its own byte/node/depth limits.
pub struct ParsedReceipt {
    object: Object,
    value: Value,
    nodes: usize,
    depth: usize,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub instance: Object,
    pub definition: Object,
    pub authored_schema: Object,
    pub schema_policy: Object,
    pub evidence_policy: Object,
    pub subject: Object,
    /// Present only for the explicit retained-stream policy. Neither field
    /// establishes producer currency, remote state or retention authority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_selection: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_is_selected: Option<bool>,
    pub receipt: Object,
    pub declared_target_identity: Digest,
    pub retained_receipt_binding_validated: bool,
    pub receipt_bytes_verified: bool,
    pub receipt_length_asserted: bool,
    pub payload_verified: bool,
    pub provider_identity_verified: bool,
    pub current_remote_content_verified: bool,
    pub independent_custody_verified: bool,
    pub authorizes_eviction: bool,
    pub complete_semantic_coverage: bool,
}

fn token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

impl Checker {
    pub fn new(bound: &BoundInstance, checked: &Checked, policy: &[u8]) -> Result<Self> {
        if checked.instance() != bound.instance().object()
            || checked.definition() != bound.definition().object()
        {
            return Err(Error::Schema);
        }
        let parsed = identity::parse_json(policy).map_err(|_| Error::Policy)?;
        let policy_value: Policy = serde_json::from_value(parsed).map_err(|_| Error::Policy)?;
        if policy_value.schema != "tig.publication-evidence-policy.experimental-1"
            || !token(&policy_value.subject_slot)
            || !token(&policy_value.subject_role)
            || !token(&policy_value.receipt_role)
            || policy_value.rules.is_empty()
            || policy_value.rules.len() > 16
            || !(1..=16 * 1024 * 1024).contains(&policy_value.limits.bytes)
            || !(1..=262_144).contains(&policy_value.limits.nodes)
            || !(1..=32).contains(&policy_value.limits.depth)
        {
            return Err(Error::Policy);
        }
        validate_mapping(&policy_value.limits, &policy_value.rules)?;
        let authored = Value::Object(bound.instance().value().authored.clone());
        let target: Target = serde_json::from_value(
            pointer::resolve(&authored, &pointer::parse(&policy_value.target_pointer)?)?.clone(),
        )
        .map_err(|_| Error::Target)?;
        if !token(&target.provider)
            || !token(&target.scope)
            || !token(&target.receipt_slot)
            || target.receipt_slot == policy_value.subject_slot
        {
            return Err(Error::Target);
        }
        pointer::parse(&target.receipt_selector)?;
        let value = bound.instance().value();
        let (subject, subject_is_selected) = match policy_value.subject_selection {
            SubjectSelection::Selected => {
                let subject = value.selected.as_ref().ok_or(Error::Subject)?.object();
                if value.streams.iter().all(|s| {
                    s.slot != policy_value.subject_slot
                        || s.role != policy_value.subject_role
                        || s.object.object() != subject
                }) {
                    return Err(Error::Subject);
                }
                (subject, None)
            }
            SubjectSelection::DeclaredStream => {
                let mut streams = value.streams.iter().filter(|s| {
                    s.slot == policy_value.subject_slot && s.role == policy_value.subject_role
                });
                let subject = streams.next().ok_or(Error::Subject)?.object.object();
                if streams.next().is_some() {
                    return Err(Error::Subject);
                }
                let selected = value.selected.as_ref().map(|pin| pin.object());
                let same = selected.as_ref() == Some(&subject);
                (subject, Some(same))
            }
        };
        let receipt = value
            .streams
            .iter()
            .find(|s| s.slot == target.receipt_slot && s.role == policy_value.receipt_role)
            .ok_or(Error::Target)?
            .object
            .object();
        if receipt.bytes > policy_value.limits.bytes as u64 {
            return Err(Error::Budget);
        }
        Ok(Self {
            instance: bound.instance().object().clone(),
            definition: checked.definition().clone(),
            authored_schema: checked.authored_schema().clone(),
            schema_policy: checked.policy().clone(),
            subject_is_selected,
            inner: ReceiptChecker {
                binding: None,
                policy_object: Object::from_bytes(policy),
                policy: Mapping {
                    schema: "tig.receipt-mapping.experimental-1".into(),
                    limits: policy_value.limits,
                    rules: policy_value.rules,
                },
                target: ReceiptTarget {
                    provider: target.provider,
                    scope: target.scope,
                    receipt_selector: target.receipt_selector,
                },
                subject,
                receipt,
            },
        })
    }

    pub fn receipt(&self) -> &Object {
        self.inner.receipt()
    }
    pub fn parse_receipt(&self, raw: &[u8]) -> Result<ParsedReceipt> {
        self.inner.parse_receipt(raw)
    }
    pub fn check(&self, raw: &[u8]) -> Result<Report> {
        self.check_parsed(&self.parse_receipt(raw)?)
    }
    pub fn check_parsed(&self, receipt: &ParsedReceipt) -> Result<Report> {
        let result = self.inner.check_parsed(receipt)?;
        Ok(Report {
            schema: "tig.publication-evidence-check.experimental-1",
            instance: self.instance.clone(),
            definition: self.definition.clone(),
            authored_schema: self.authored_schema.clone(),
            schema_policy: self.schema_policy.clone(),
            evidence_policy: result.evidence_policy,
            subject: result.subject,
            subject_selection: self.subject_is_selected.map(|_| "declared-stream"),
            subject_is_selected: self.subject_is_selected,
            receipt: result.receipt,
            declared_target_identity: result.declared_target_identity,
            retained_receipt_binding_validated: true,
            receipt_bytes_verified: true,
            receipt_length_asserted: result.receipt_length_asserted,
            payload_verified: false,
            provider_identity_verified: false,
            current_remote_content_verified: false,
            independent_custody_verified: false,
            authorizes_eviction: false,
            complete_semantic_coverage: false,
        })
    }
}

/// Explicit evidence binding independent of an asset's authored target fields.
pub struct ReceiptChecker {
    binding: Option<Object>,
    policy_object: Object,
    policy: Mapping,
    target: ReceiptTarget,
    subject: Object,
    receipt: Object,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    schema: String,
    limits: Limits,
    rules: Vec<Rule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptBinding {
    schema: String,
    subject: Object,
    receipt: Object,
    provider: String,
    scope: String,
    selector: String,
    mapping: Object,
}
/// Exact immutable binding metadata; no target strings exposed by diagnostics.
pub struct ParsedBinding {
    object: Object,
    value: ReceiptBinding,
}
impl ParsedBinding {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let value: ReceiptBinding =
            serde_json::from_value(identity::parse_json(raw).map_err(|_| Error::Policy)?)
                .map_err(|_| Error::Policy)?;
        if value.schema != "tig.receipt-binding.experimental-1"
            || !token(&value.provider)
            || !token(&value.scope)
        {
            return Err(Error::Policy);
        }
        pointer::parse(&value.selector)?;
        let object = Object::from_bytes(raw);
        let mut sizes = std::collections::BTreeMap::new();
        for pin in [&value.subject, &value.receipt, &value.mapping, &object] {
            if sizes
                .insert(&pin.sha256, pin.bytes)
                .is_some_and(|old| old != pin.bytes)
            {
                return Err(Error::Pin);
            }
        }
        if value.mapping.bytes > 65_536 || value.receipt.bytes > 16 * 1024 * 1024 {
            return Err(Error::Budget);
        }
        Ok(Self { object, value })
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn mapping(&self) -> &Object {
        &self.value.mapping
    }
    pub fn receipt(&self) -> &Object {
        &self.value.receipt
    }
    pub fn subject(&self) -> &Object {
        &self.value.subject
    }
}
struct ReceiptTarget {
    provider: String,
    scope: String,
    receipt_selector: String,
}
#[derive(Debug, Serialize)]
pub struct ReceiptReport {
    pub schema: &'static str,
    pub binding: Option<Object>,
    pub evidence_policy: Object,
    pub subject: Object,
    pub receipt: Object,
    pub declared_target_identity: Digest,
    pub retained_receipt_binding_validated: bool,
    pub receipt_bytes_verified: bool,
    pub receipt_length_asserted: bool,
    pub payload_verified: bool,
    pub provider_identity_verified: bool,
    pub current_remote_content_verified: bool,
    pub independent_custody_verified: bool,
    pub authorizes_eviction: bool,
    pub complete_semantic_coverage: bool,
}
fn validate_mapping(limits: &Limits, rules: &[Rule]) -> Result<()> {
    if rules.is_empty()
        || rules.len() > 16
        || !(1..=16 * 1024 * 1024).contains(&limits.bytes)
        || !(1..=262_144).contains(&limits.nodes)
        || !(1..=32).contains(&limits.depth)
    {
        return Err(Error::Policy);
    }
    let mut roots: Vec<Vec<String>> = Vec::new();
    for rule in rules {
        let root = pointer::parse(rule.root())?;
        if roots
            .iter()
            .any(|r| root.starts_with(r) || r.starts_with(&root))
        {
            return Err(Error::Policy);
        }
        if let Rule::SingleRow { fields, .. }
        | Rule::ArrayRows { fields, .. }
        | Rule::ObjectRows { fields, .. } = rule
        {
            pointer::parse(&fields.sha256)?;
            match &fields.target {
                TargetField::Pointer(path) => {
                    pointer::parse(path)?;
                }
                TargetField::MemberKey(MemberKey {
                    kind: MemberKeyKind::MemberKey,
                }) => {
                    if !matches!(rule, Rule::ObjectRows { .. }) {
                        return Err(Error::Policy);
                    }
                }
                TargetField::OpaqueLocator(OpaqueLocator {
                    kind: OpaqueLocatorKind::OpaqueLocator,
                    pointer,
                }) => {
                    pointer::parse(pointer)?;
                }
            }
            if let Some(bytes) = &fields.bytes {
                pointer::parse(bytes)?;
            }
        }
        roots.push(root);
    }
    Ok(())
}
impl ReceiptChecker {
    pub fn new(binding: &[u8], mapping: &[u8]) -> Result<Self> {
        Self::from_binding(&ParsedBinding::parse(binding)?, mapping)
    }
    pub fn from_binding(binding: &ParsedBinding, mapping: &[u8]) -> Result<Self> {
        let value = &binding.value;
        let policy: Mapping =
            serde_json::from_value(identity::parse_json(mapping).map_err(|_| Error::Policy)?)
                .map_err(|_| Error::Policy)?;
        if policy.schema != "tig.receipt-mapping.experimental-1" {
            return Err(Error::Policy);
        }
        if value.mapping != Object::from_bytes(mapping) {
            return Err(Error::Pin);
        }
        validate_mapping(&policy.limits, &policy.rules)?;
        if !token(&value.provider) || !token(&value.scope) {
            return Err(Error::Target);
        }
        pointer::parse(&value.selector)?;
        let mut sizes = std::collections::BTreeMap::new();
        for pin in [&value.subject, &value.receipt, &value.mapping] {
            if sizes
                .insert(&pin.sha256, pin.bytes)
                .is_some_and(|old| old != pin.bytes)
            {
                return Err(Error::Pin);
            }
        }
        if value.receipt.bytes > policy.limits.bytes as u64 {
            return Err(Error::Budget);
        }
        Ok(Self {
            binding: Some(binding.object.clone()),
            policy_object: value.mapping.clone(),
            policy,
            target: ReceiptTarget {
                provider: value.provider.clone(),
                scope: value.scope.clone(),
                receipt_selector: value.selector.clone(),
            },
            subject: value.subject.clone(),
            receipt: value.receipt.clone(),
        })
    }

    pub fn receipt_limit(&self) -> usize {
        self.policy.limits.bytes
    }
    pub fn receipt(&self) -> &Object {
        &self.receipt
    }

    pub fn parse_receipt(&self, raw: &[u8]) -> Result<ParsedReceipt> {
        if raw.len() > self.policy.limits.bytes {
            return Err(Error::Budget);
        }
        if Object::from_bytes(raw) != self.receipt {
            return Err(Error::Pin);
        }
        let (value, nodes, depth) = evidence_json::parse(raw, &self.policy.limits)?;
        Ok(ParsedReceipt {
            object: self.receipt.clone(),
            value,
            nodes,
            depth,
        })
    }

    pub fn check(&self, raw: &[u8]) -> Result<ReceiptReport> {
        self.check_parsed(&self.parse_receipt(raw)?)
    }

    pub fn check_parsed(&self, receipt: &ParsedReceipt) -> Result<ReceiptReport> {
        if receipt.object != self.receipt {
            return Err(Error::Pin);
        }
        if receipt.object.bytes > self.policy.limits.bytes as u64
            || receipt.nodes > self.policy.limits.nodes
            || receipt.depth > self.policy.limits.depth
        {
            return Err(Error::Budget);
        }
        let evidence = &receipt.value;
        let selector = pointer::parse(&self.target.receipt_selector)?;
        let mut selected = None;
        for rule in &self.policy.rules {
            let root = pointer::parse(rule.root())?;
            let matches = match rule {
                Rule::SingleRow { .. } => selector == root,
                _ => selector.len() == root.len() + 1 && selector.starts_with(&root),
            };
            if matches {
                if selected.is_some() {
                    return Err(Error::Selector);
                }
                selected = Some((rule, root));
            }
        }
        let (rule, root) = selected.ok_or(Error::Selector)?;
        let parent = pointer::resolve(evidence, &root)?;
        let member = selector.last();
        let (target, length_asserted, opaque) = match rule {
            Rule::KeyedHashes { .. } => {
                let member = member.ok_or(Error::Selector)?;
                let hash = parent
                    .as_object()
                    .and_then(|m| m.get(member))
                    .and_then(Value::as_str)
                    .ok_or(Error::Evidence)?;
                if hash != self.subject.sha256.as_str() {
                    return Err(Error::Subject);
                }
                (member.as_str(), false, false)
            }
            Rule::SingleRow { fields, .. }
            | Rule::ArrayRows { fields, .. }
            | Rule::ObjectRows { fields, .. } => {
                let row = match rule {
                    Rule::SingleRow { .. } => parent.as_object().map(|_| parent),
                    Rule::ArrayRows { .. } => parent.as_array().and_then(|items| {
                        member
                            .and_then(|member| pointer::index(member).ok())
                            .and_then(|n| items.get(n))
                    }),
                    _ => parent
                        .as_object()
                        .and_then(|m| member.and_then(|member| m.get(member))),
                }
                .ok_or(Error::Selector)?;
                let hash = pointer::resolve(row, &pointer::parse(&fields.sha256)?)?
                    .as_str()
                    .ok_or(Error::Evidence)?;
                if hash != self.subject.sha256.as_str() {
                    return Err(Error::Subject);
                }
                if let Some(bytes) = &fields.bytes
                    && pointer::resolve(row, &pointer::parse(bytes)?)?.as_u64()
                        != Some(self.subject.bytes)
                {
                    return Err(Error::Subject);
                }
                let (target, opaque) = match &fields.target {
                    TargetField::Pointer(path) => (
                        pointer::resolve(row, &pointer::parse(path)?)?
                            .as_str()
                            .ok_or(Error::Target)?,
                        false,
                    ),
                    TargetField::MemberKey(MemberKey {
                        kind: MemberKeyKind::MemberKey,
                    }) => (member.ok_or(Error::Selector)?.as_str(), false),
                    TargetField::OpaqueLocator(OpaqueLocator {
                        kind: OpaqueLocatorKind::OpaqueLocator,
                        pointer,
                    }) => (
                        pointer::resolve(row, &pointer::parse(pointer)?)?
                            .as_str()
                            .ok_or(Error::Target)?,
                        true,
                    ),
                };
                (target, fields.bytes.is_some(), opaque)
            }
        };
        let identity = if opaque {
            if target.trim().is_empty()
                || target.len() > 4096
                || target.chars().any(char::is_control)
            {
                return Err(Error::Target);
            }
            json!({"schema":"tig.declared-publication-locator.experimental-1",
                "provider":self.target.provider,"scope":self.target.scope,"locator":target})
        } else {
            LogicalPath::parse(target).map_err(|_| Error::Target)?;
            json!({
                "schema":"tig.declared-publication-target.experimental-1",
                "provider":self.target.provider,"scope":self.target.scope,"path":target
            })
        };
        let declared_target_identity =
            Object::from_bytes(&identity::canonical_json(&identity).map_err(|_| Error::Target)?)
                .sha256;
        Ok(ReceiptReport {
            schema: "tig.receipt-binding-check.experimental-1",
            binding: self.binding.clone(),
            evidence_policy: self.policy_object.clone(),
            subject: self.subject.clone(),
            receipt: self.receipt.clone(),
            declared_target_identity,
            retained_receipt_binding_validated: true,
            receipt_bytes_verified: true,
            receipt_length_asserted: length_asserted,
            payload_verified: false,
            provider_identity_verified: false,
            current_remote_content_verified: false,
            independent_custody_verified: false,
            authorizes_eviction: false,
            complete_semantic_coverage: false,
        })
    }
}

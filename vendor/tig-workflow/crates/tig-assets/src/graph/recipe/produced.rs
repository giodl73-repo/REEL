//! Unsigned produced-output claims and declared-recipe correspondence only.
//! Never producer execution, approval, byte availability, custody or eviction.
use super::*;
use serde::{Deserialize, Serialize};
use tig_core::identity::parse_json;

const SCHEMA: &str = "tig.produced-claim.experimental-1";
const MAX_OUTPUTS: usize = 256;
const MAX_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Selector {
    Selected {},
    Stream { slot: String },
}
impl Selector {
    pub(super) fn validate(&self) -> Result<()> {
        if let Self::Stream { slot } = self {
            token(slot)?;
            require(slot.len() <= 128, "output slot budget")?;
        }
        Ok(())
    }
}

/// Stable subject, without a self-referential hash of the .tig claiming it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    asset_id: String,
    definition: Reference,
    selector: Selector,
    object: Object,
}
impl Subject {
    pub fn asset_id(&self) -> &str {
        &self.asset_id
    }
    pub fn definition(&self) -> &Reference {
        &self.definition
    }
    pub fn selector(&self) -> &Selector {
        &self.selector
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
    fn validate(&self) -> Result<()> {
        token(&self.asset_id)?;
        self.definition.validate()?;
        self.selector.validate()?;
        require(self.object.bytes <= MAX_INTEGER, "unsafe output size")
    }
}

/// Current selection bound under caller admission, not a deserialized claim subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundSubject {
    subject: Subject,
}
impl BoundSubject {
    /// Missing selected/named bytes are planned, never a fallback to another slot.
    /// Definition admission is external; this grants no producer/rights authority.
    pub fn bind(
        bound: &BoundInstance,
        admission: &Reference,
        selector: Selector,
    ) -> Result<Option<Self>> {
        admission.validate()?;
        selector.validate()?;
        require(
            bound.definition().object() == &admission.object
                && bound.definition().value().id == admission.id,
            "output definition admission differs",
        )?;
        let record = bound.instance().value();
        let object = match &selector {
            Selector::Selected {} => record.selected.as_ref(),
            Selector::Stream { slot } => record
                .streams
                .iter()
                .find(|s| &s.slot == slot)
                .map(|s| &s.object),
        };
        Ok(object.map(|pin| Self {
            subject: Subject {
                asset_id: record.id.clone(),
                definition: admission.clone(),
                selector,
                object: pin.object(),
            },
        }))
    }
    /// Bind from the qualified immutable resolver port without another record read.
    pub fn resolve(
        resolved: &crate::resolve::Resolved,
        admission: &Reference,
        selector: Selector,
    ) -> Result<Option<Self>> {
        use crate::resolve::Error as ResolveError;
        admission.validate()?;
        selector.validate()?;
        require(
            resolved.definition() == admission,
            "output definition admission differs",
        )?;
        let object = match &selector {
            Selector::Selected {} => match resolved.selected() {
                Ok(object) => Some(object.clone()),
                Err(ResolveError::Planned) => None,
                Err(_) => return Err(Error("output resolution unavailable".into())),
            },
            Selector::Stream { slot } => match resolved.stream(slot) {
                Ok(object) => Some(object.clone()),
                Err(ResolveError::MissingStream) => None,
                Err(_) => return Err(Error("output resolution unavailable".into())),
            },
        };
        object
            .map(|object| {
                let subject = Subject {
                    asset_id: resolved.reference().id.clone(),
                    definition: admission.clone(),
                    selector,
                    object,
                };
                subject.validate()?;
                Ok(Self { subject })
            })
            .transpose()
    }
    pub fn subject(&self) -> &Subject {
        &self.subject
    }
}

/// Claimed immutable native context pin; parsing/plan checking does not read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPin {
    pub root: Object,
    pub identity: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    checkpoint: Digest,
    binding: Digest,
    admission: Digest,
    consumption: Digest,
}
impl Origin {
    fn plan(plan: &Plan) -> Self {
        Self {
            checkpoint: plan.checkpoint.clone(),
            binding: plan.binding.clone(),
            admission: plan.admission.clone(),
            consumption: plan.consumption.clone(),
        }
    }
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn binding(&self) -> &Digest {
        &self.binding
    }
    pub fn admission(&self) -> &Digest {
        &self.admission
    }
    pub fn consumption(&self) -> &Digest {
        &self.consumption
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: String,
    operation: Operation,
    recipe: Digest,
    origin: Origin,
    context: ContextPin,
    outputs: Vec<Subject>,
}
/// Exact private claim bytes. Immutable and deliberately not directly deserializable.
pub struct Claim {
    wire: Wire,
    object: Object,
    bytes: Vec<u8>,
}
impl std::fmt::Debug for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProducedClaim")
            .field("object", &self.object)
            .field("outputs", &self.wire.outputs.len())
            .finish_non_exhaustive()
    }
}
fn validate(wire: &Wire, own: Option<&Object>) -> Result<()> {
    require(wire.schema == SCHEMA, "unsupported produced claim schema")?;
    token(&wire.operation.name)?;
    require(
        (1..=MAX_OUTPUTS).contains(&wire.outputs.len()),
        "produced output count budget",
    )?;
    let mut sizes = BTreeMap::new();
    let mut size = |object: &Object| -> Result<()> {
        require(
            object.bytes <= MAX_INTEGER,
            "unsafe produced claim pin size",
        )?;
        require(
            sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_none_or(|n| n == object.bytes),
            "produced claim pin sizes conflict",
        )
    };
    if let Some(own) = own {
        size(own)?;
    }
    for object in [
        &wire.operation.producer,
        &wire.operation.parameters,
        &wire.context.root,
    ] {
        size(object)?;
    }
    let mut keys = BTreeSet::new();
    for subject in &wire.outputs {
        subject.validate()?;
        require(
            keys.insert((&subject.asset_id, &subject.selector)),
            "duplicate output subject",
        )?;
        size(&subject.definition.object)?;
        size(&subject.object)?;
    }
    Ok(())
}
impl Claim {
    /// Records a candidate producer claim. Context/output bytes are NOT verified.
    pub fn create(plan: &Plan, context: ContextPin, outputs: Vec<Subject>) -> Result<Self> {
        let mut outputs = outputs;
        outputs.sort();
        let wire = Wire {
            schema: SCHEMA.into(),
            operation: plan.operation.clone(),
            recipe: plan.fingerprint.clone(),
            origin: Origin::plan(plan),
            context,
            outputs,
        };
        validate(&wire, None)?;
        let bytes = canonical_json(
            &serde_json::to_value(&wire).map_err(|_| Error("invalid produced claim".into()))?,
        )?;
        let object = Object::from_bytes(&bytes);
        validate(&wire, Some(&object))?;
        Ok(Self {
            wire,
            object,
            bytes,
        })
    }
    /// Exact externally pinned bytes only; no provider lookup or authority inferred.
    pub fn parse(pin: &Object, bytes: &[u8]) -> Result<Self> {
        require(
            pin.bytes <= MAX_RECORD_BYTES as u64 && bytes.len() <= MAX_RECORD_BYTES,
            "produced claim byte budget",
        )?;
        require(
            pin == &Object::from_bytes(bytes),
            "produced claim byte pin differs",
        )?;
        let wire: Wire = serde_json::from_value(parse_json(bytes)?)
            .map_err(|_| Error("invalid produced claim fields".into()))?;
        validate(&wire, Some(pin))?;
        Ok(Self {
            wire,
            object: pin.clone(),
            bytes: bytes.to_vec(),
        })
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn operation(&self) -> &Operation {
        &self.wire.operation
    }
    pub fn recipe(&self) -> &Digest {
        &self.wire.recipe
    }
    pub fn origin(&self) -> &Origin {
        &self.wire.origin
    }
    pub fn context(&self) -> &ContextPin {
        &self.wire.context
    }
    pub fn outputs(&self) -> &[Subject] {
        &self.wire.outputs
    }
    /// Checks only exact original declared Plan correspondence; context remains unread.
    pub fn check_plan<'a>(&'a self, plan: &Plan) -> Result<PlanCheckedClaim<'a>> {
        require(
            self.wire.operation == plan.operation
                && self.wire.recipe == plan.fingerprint
                && self.wire.origin == Origin::plan(plan),
            "produced claim differs from original plan",
        )?;
        Ok(PlanCheckedClaim { claim: self })
    }
}
#[derive(Debug)]
pub struct PlanCheckedClaim<'a> {
    claim: &'a Claim,
}
impl PlanCheckedClaim<'_> {
    pub fn claim(&self) -> &Claim {
        self.claim
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Correspondence {
    Unbuilt,
    Unproven,
    ClaimedRecipeMatch,
    StaleRecipe,
}

/// Compare exact current subject with a claim checked against its original Plan.
/// A matching unsigned claim still grants no execution/approval/custody authority.
pub fn compare(
    desired: &Plan,
    current: Option<&BoundSubject>,
    claim: Option<&PlanCheckedClaim<'_>>,
) -> Result<Correspondence> {
    match (current, claim) {
        (None, None) => Ok(Correspondence::Unbuilt),
        (Some(_), None) => Ok(Correspondence::Unproven),
        (None, Some(_)) => Err(Error("produced claim has no current output subject".into())),
        (Some(subject), Some(checked)) => {
            require(
                checked.claim.wire.outputs.contains(subject.subject()),
                "claim does not name exact current output subject",
            )?;
            Ok(
                if checked.claim.wire.recipe == desired.fingerprint
                    && checked.claim.wire.operation == desired.operation
                {
                    Correspondence::ClaimedRecipeMatch
                } else {
                    Correspondence::StaleRecipe
                },
            )
        }
    }
}

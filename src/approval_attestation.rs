//! Owner-issued signed approval attestations.
//!
//! REEL never creates reviewer identities, decides who has authority, or grants
//! rights, publication, or release. It provides a strict, owner-neutral envelope
//! that cryptographically binds an owner-controlled Ed25519 key to an exact
//! target, policy, scope, and decision, and verifies that binding against an
//! independently trusted authority registry.
//!
//! # Canonical signing bytes
//!
//! Attestations never sign ordinary JSON. Signatures cover a documented,
//! deterministic, domain-separated, length-delimited UTF-8 encoding of every
//! security-relevant field. The encoding is:
//!
//! ```text
//! bytes := chunk(DOMAIN) || field*
//! field := chunk(label) || presence || (present ? chunk(value) : "")
//! chunk(x) := big_endian_u64(len(x)) || x
//! presence := 0x01 for a present value, 0x00 for an absent optional value
//! ```
//!
//! Fields are emitted in this fixed order, each value encoded as strict UTF-8:
//! schema, owner id, registry id, registry SHA-256, authority-context SHA-256,
//! target kind, target SHA-256, policy SHA-256, scope, decision, role, approver
//! id, public key (hex), key id (hex), issued-at UTC, expires-at UTC, sequence
//! (decimal), and the optional prior-attestation SHA-256. The signature itself is
//! never part of the signed bytes; the exact public key is, so the verifier binds
//! the signature to that key and derives the key id independently.

use std::{
    collections::{BTreeSet, HashSet},
    fmt,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{
    Deserialize, Serialize,
    de::{self, DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor},
};
use sha2::{Digest, Sha256};
use tempfile::Builder;
use zeroize::Zeroize;

pub const REGISTRY_SCHEMA: &str = "reel.approval-authority-registry.v0.1";
pub const AUTHORIZATION_INPUT_SCHEMA: &str = "reel.approval-authorization-input.v0.1";
pub const ATTESTATION_SCHEMA: &str = "reel.signed-approval-attestation.v0.1";
pub const VERIFICATION_INPUT_SCHEMA: &str = "reel.approval-verification-input.v0.1";
pub const VERIFICATION_REPORT_SCHEMA: &str = "reel.approval-verification-report.v0.1";

/// Domain-separation prefix for the canonical signing bytes.
const CANONICAL_DOMAIN: &[u8] = b"REEL-signed-approval-attestation-canonical-v0.2.38";
const MAX_CONTRACT_BYTES: u64 = 4 * 1024 * 1024;
const SIGNING_KEY_LEN: usize = 32;
const MAX_TOKEN_LEN: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    Approved,
    Rejected,
    Revoked,
}

impl Decision {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "revoked" => Ok(Self::Revoked),
            _ => bail!("decision must be approved, rejected, or revoked"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Revoked => "revoked",
        }
    }
}

/// Owner-defined legal decision transitions. Revocation always requires an exact
/// validated prior attestation, so it can never originate a chain.
fn legal_transition(prior: Option<Decision>, next: Decision) -> bool {
    use Decision::{Approved, Rejected, Revoked};
    match (prior, next) {
        (None, Approved) | (None, Rejected) => true,
        (None, Revoked) => false,
        (Some(Approved), Revoked) | (Some(Approved), Rejected) => true,
        (Some(Rejected), Approved) => true,
        (Some(Revoked), Approved) => true,
        _ => false,
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalFileHash {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalAuthorityRegistry {
    schema: String,
    owner_id: String,
    registry_id: String,
    entries: Vec<AuthorityEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityEntry {
    approver_id: String,
    role: String,
    public_key: String,
    key_id: String,
    scopes: Vec<String>,
    decisions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalAuthorizationInput {
    schema: String,
    owner_id: String,
    registry: LocalFileHash,
    signing_key_path: PathBuf,
    authority_context_sha256: String,
    approver_id: String,
    role: String,
    scope: String,
    decision: String,
    target_kind: String,
    target: LocalFileHash,
    policy_sha256: String,
    sequence: u64,
    issued_at_utc: String,
    expires_at_utc: String,
    #[serde(default)]
    prior_attestation: Option<LocalFileHash>,
}

/// Immutable signed attestation. Reordering or renaming a field changes the
/// canonical bytes and therefore the signature.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedApprovalAttestation {
    pub schema: String,
    pub owner_id: String,
    pub registry_id: String,
    pub registry_sha256: String,
    pub authority_context_sha256: String,
    pub approver_id: String,
    pub role: String,
    pub public_key: String,
    pub key_id: String,
    pub target_kind: String,
    pub target_sha256: String,
    pub policy_sha256: String,
    pub scope: String,
    pub decision: String,
    pub sequence: u64,
    pub issued_at_utc: String,
    pub expires_at_utc: String,
    pub prior_attestation_sha256: Option<String>,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalVerificationInput {
    schema: String,
    expected_registry_sha256: String,
    registry: LocalFileHash,
    target: LocalFileHash,
    verification_time_utc: String,
    chain: Vec<LocalFileHash>,
}

/// Path-free portable verification report. Every dimension is stated
/// independently; a valid signature never infers rights, publication, or release.
#[derive(Clone, Debug, Serialize)]
pub struct ApprovalVerificationReport {
    pub schema: String,
    pub owner_id: String,
    pub registry_id: String,
    pub registry_sha256: String,
    pub authority_context_sha256: String,
    pub target_kind: String,
    pub target_sha256: String,
    pub policy_sha256: String,
    pub scope: String,
    pub cryptographic_signature_valid: bool,
    pub registry_authorized: bool,
    pub target_integrity_verified: bool,
    pub time_valid_at_verification: bool,
    pub signed_human_decision: String,
    pub historical_decisions: Vec<String>,
    pub current_decision: String,
    pub current_status_basis: String,
    pub authenticated_from_origin: bool,
    pub chain_length: usize,
    pub chain_attestation_sha256: Vec<String>,
    pub head_approver_id: String,
    pub head_role: String,
    pub head_key_id: String,
    pub head_sequence: u64,
    pub head_issued_at_utc: String,
    pub head_expires_at_utc: String,
    pub verification_time_utc: String,
    pub decision_scope_only: bool,
    pub implies_other_scopes: bool,
    pub implies_rights: bool,
    pub implies_publication: bool,
    pub implies_release: bool,
    pub creative_quality_evaluated: bool,
    pub human_authority_required: bool,
}

/// Sign one owner-issued approval attestation from a strict local authorization
/// contract and publish it as a no-clobber, path-free attestation file.
pub fn sign_attestation(
    input_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<SignedApprovalAttestation> {
    let input_path = input_path.as_ref();
    let input_bytes = read_contract_bytes(input_path, "approval authorization input")?;
    let input: ApprovalAuthorizationInput =
        parse_json_strict(&input_bytes, "approval authorization input")?;
    require_schema(&input.schema, AUTHORIZATION_INPUT_SCHEMA)?;
    require_token("owner id", &input.owner_id)?;
    require_token("approver id", &input.approver_id)?;
    require_token("role", &input.role)?;
    require_token("scope", &input.scope)?;
    require_token("target kind", &input.target_kind)?;
    let decision = Decision::parse(&input.decision)?;
    require_hash(&input.authority_context_sha256)?;
    require_hash(&input.policy_sha256)?;
    require_utc(&input.issued_at_utc)?;
    require_utc(&input.expires_at_utc)?;
    if input.issued_at_utc >= input.expires_at_utc {
        bail!("issued_at_utc must strictly precede expires_at_utc");
    }
    if input.sequence == 0 {
        bail!("sequence must be a positive integer");
    }

    let (registry, registry_sha256) = load_registry(input_path, &input.registry)?;
    if registry.owner_id != input.owner_id {
        bail!("authority registry owner does not match the authorization owner");
    }

    let signing_key = read_signing_key(&resolve(input_path, &input.signing_key_path))?;
    let public_key_bytes = signing_key.verifying_key().to_bytes();
    let public_key = to_hex(&public_key_bytes);
    let key_id = hash_bytes(&public_key_bytes);

    authorized_entry(
        &registry,
        &public_key,
        &key_id,
        &input.approver_id,
        &input.role,
        &input.scope,
        decision.as_str(),
    )?;

    require_hash(&input.target.sha256)?;
    let target_path = resolve(input_path, &input.target.path);
    let target_sha256 = hash_file_streaming(&target_path)?;
    if target_sha256 != input.target.sha256 {
        bail!("approval target hash does not match the pinned digest");
    }

    let prior_attestation_sha256 = match (decision, input.prior_attestation.as_ref()) {
        (Decision::Revoked, None) => {
            bail!("revocation requires an exact validated prior attestation")
        }
        (_, None) => {
            if input.sequence != 1 {
                bail!("an origin attestation must use sequence 1");
            }
            if !legal_transition(None, decision) {
                bail!("{} requires a prior attestation", decision.as_str());
            }
            None
        }
        (_, Some(binding)) => {
            require_hash(&binding.sha256)?;
            let prior_bytes = read_contract_bytes(
                &resolve(input_path, &binding.path),
                "prior approval attestation",
            )?;
            let prior_hash = hash_bytes(&prior_bytes);
            if prior_hash != binding.sha256 {
                bail!("prior attestation hash does not match the pinned digest");
            }
            let prior: SignedApprovalAttestation =
                parse_json_strict(&prior_bytes, "prior approval attestation")?;
            let prior_decision = validate_attestation_structure(&prior)?;
            verify_signature(&prior)?;
            if prior.registry_sha256 != registry_sha256 {
                bail!("prior attestation is bound to a different authority registry");
            }
            authorized_entry(
                &registry,
                &prior.public_key,
                &prior.key_id,
                &prior.approver_id,
                &prior.role,
                &prior.scope,
                &prior.decision,
            )?;
            ensure_same_authority_context(
                &prior,
                &input.owner_id,
                &registry.registry_id,
                &registry_sha256,
                &input.authority_context_sha256,
                &input.target_kind,
                &target_sha256,
                &input.policy_sha256,
                &input.scope,
            )?;
            let expected_sequence = prior
                .sequence
                .checked_add(1)
                .ok_or_else(|| anyhow!("prior attestation sequence overflow"))?;
            if input.sequence != expected_sequence {
                bail!("child attestation sequence must equal prior sequence plus one");
            }
            if input.issued_at_utc <= prior.issued_at_utc {
                bail!("issued_at_utc must increase beyond the prior attestation");
            }
            if !legal_transition(Some(prior_decision), decision) {
                bail!(
                    "illegal decision transition from {} to {}",
                    prior_decision.as_str(),
                    decision.as_str()
                );
            }
            Some(prior_hash)
        }
    };

    let mut attestation = SignedApprovalAttestation {
        schema: ATTESTATION_SCHEMA.to_string(),
        owner_id: input.owner_id,
        registry_id: registry.registry_id,
        registry_sha256,
        authority_context_sha256: input.authority_context_sha256,
        approver_id: input.approver_id,
        role: input.role,
        public_key,
        key_id,
        target_kind: input.target_kind,
        target_sha256,
        policy_sha256: input.policy_sha256,
        scope: input.scope,
        decision: decision.as_str().to_string(),
        sequence: input.sequence,
        issued_at_utc: input.issued_at_utc,
        expires_at_utc: input.expires_at_utc,
        prior_attestation_sha256,
        signature: String::new(),
    };

    let message = canonical_bytes(&attestation);
    let signature = signing_key.sign(&message);
    attestation.signature = to_hex(&signature.to_bytes());
    // Self-verify before publishing so a corrupt key or encoding never persists.
    verify_signature(&attestation)?;
    write_json_new(&attestation, output_path.as_ref())?;
    Ok(attestation)
}

/// Independently verify a hash-pinned attestation chain against an independently
/// trusted registry digest and the current target bytes.
pub fn verify_attestation(
    input_path: impl AsRef<Path>,
    output_path: Option<&Path>,
) -> Result<ApprovalVerificationReport> {
    let input_path = input_path.as_ref();
    let input_bytes = read_contract_bytes(input_path, "approval verification input")?;
    let input: ApprovalVerificationInput =
        parse_json_strict(&input_bytes, "approval verification input")?;
    require_schema(&input.schema, VERIFICATION_INPUT_SCHEMA)?;
    require_hash(&input.expected_registry_sha256)?;
    require_hash(&input.registry.sha256)?;
    require_hash(&input.target.sha256)?;
    require_utc(&input.verification_time_utc)?;
    if input.chain.is_empty() {
        bail!("verification requires at least one hash-pinned attestation");
    }

    let (registry, registry_sha256) = load_registry(input_path, &input.registry)?;
    if registry_sha256 != input.expected_registry_sha256 {
        bail!("authority registry does not match the independently trusted digest");
    }

    let target_path = resolve(input_path, &input.target.path);
    let target_sha256 = hash_file_streaming(&target_path)?;
    if target_sha256 != input.target.sha256 {
        bail!("approval target hash does not match the pinned digest");
    }

    let mut chain: Vec<(String, SignedApprovalAttestation)> = Vec::new();
    let mut seen_hashes = HashSet::new();
    for binding in &input.chain {
        require_hash(&binding.sha256)?;
        let bytes =
            read_contract_bytes(&resolve(input_path, &binding.path), "approval attestation")?;
        let hash = hash_bytes(&bytes);
        if hash != binding.sha256 {
            bail!("attestation hash does not match the pinned digest");
        }
        if !seen_hashes.insert(hash.clone()) {
            bail!("verification chain repeats an attestation, which forms a cycle");
        }
        let attestation: SignedApprovalAttestation =
            parse_json_strict(&bytes, "approval attestation")?;
        validate_attestation_structure(&attestation)?;
        verify_signature(&attestation)?;
        chain.push((hash, attestation));
    }
    chain.sort_by_key(|entry| entry.1.sequence);

    if chain[0].1.sequence != 1 {
        bail!("verification chain must begin with a sequence 1 origin attestation");
    }
    if chain[0].1.prior_attestation_sha256.is_some() {
        bail!("verification chain origin attestation must not reference a parent");
    }

    let head_context = &chain[0].1;
    for (_, attestation) in &chain {
        if attestation.owner_id != registry.owner_id
            || attestation.registry_id != registry.registry_id
        {
            bail!("attestation identity does not match the trusted authority registry");
        }
        if attestation.registry_sha256 != registry_sha256 {
            bail!("attestation is bound to a different authority registry");
        }
        if attestation.registry_sha256 != input.expected_registry_sha256 {
            bail!("attestation registry digest is not the independently trusted digest");
        }
        if attestation.target_sha256 != target_sha256 {
            bail!("attestation does not bind the current target bytes");
        }
        if attestation.owner_id != head_context.owner_id
            || attestation.registry_id != head_context.registry_id
            || attestation.authority_context_sha256 != head_context.authority_context_sha256
            || attestation.target_kind != head_context.target_kind
            || attestation.policy_sha256 != head_context.policy_sha256
            || attestation.scope != head_context.scope
        {
            bail!("attestation chain mixes authority contexts");
        }
        authorized_entry(
            &registry,
            &attestation.public_key,
            &attestation.key_id,
            &attestation.approver_id,
            &attestation.role,
            &attestation.scope,
            &attestation.decision,
        )?;
    }

    for index in 1..chain.len() {
        if Some(chain[index].1.sequence) != chain[index - 1].1.sequence.checked_add(1) {
            bail!("attestation sequence must increment exactly by one along the full chain");
        }
        if chain[index].1.issued_at_utc <= chain[index - 1].1.issued_at_utc {
            bail!("attestation issued_at_utc must strictly increase along the chain");
        }
        let expected_parent = &chain[index - 1].0;
        match chain[index].1.prior_attestation_sha256.as_deref() {
            Some(parent) if parent == expected_parent => {}
            _ => bail!("attestation parent hash does not match the preceding chain attestation"),
        }
    }

    for index in 0..chain.len() {
        let decision = Decision::parse(&chain[index].1.decision)?;
        let prior_decision = if index > 0 {
            Some(Decision::parse(&chain[index - 1].1.decision)?)
        } else {
            None
        };
        if !legal_transition(prior_decision, decision) {
            bail!(
                "illegal decision transition to {} at sequence {}",
                decision.as_str(),
                chain[index].1.sequence
            );
        }
    }

    let head = &chain.last().expect("chain checked as non-empty").1;
    if input.verification_time_utc < head.issued_at_utc {
        bail!("attestation is not yet valid at the verification time");
    }
    if input.verification_time_utc > head.expires_at_utc {
        bail!("attestation is expired at the verification time");
    }

    let report = ApprovalVerificationReport {
        schema: VERIFICATION_REPORT_SCHEMA.to_string(),
        owner_id: head.owner_id.clone(),
        registry_id: head.registry_id.clone(),
        registry_sha256,
        authority_context_sha256: head.authority_context_sha256.clone(),
        target_kind: head.target_kind.clone(),
        target_sha256,
        policy_sha256: head.policy_sha256.clone(),
        scope: head.scope.clone(),
        cryptographic_signature_valid: true,
        registry_authorized: true,
        target_integrity_verified: true,
        time_valid_at_verification: true,
        signed_human_decision: head.decision.clone(),
        historical_decisions: chain
            .iter()
            .map(|(_, attestation)| attestation.decision.clone())
            .collect(),
        current_decision: head.decision.clone(),
        current_status_basis: "full-hash-pinned-origin-chain-head".to_string(),
        authenticated_from_origin: true,
        chain_length: chain.len(),
        chain_attestation_sha256: chain.iter().map(|(hash, _)| hash.clone()).collect(),
        head_approver_id: head.approver_id.clone(),
        head_role: head.role.clone(),
        head_key_id: head.key_id.clone(),
        head_sequence: head.sequence,
        head_issued_at_utc: head.issued_at_utc.clone(),
        head_expires_at_utc: head.expires_at_utc.clone(),
        verification_time_utc: input.verification_time_utc,
        decision_scope_only: true,
        implies_other_scopes: false,
        implies_rights: false,
        implies_publication: false,
        implies_release: false,
        creative_quality_evaluated: false,
        human_authority_required: true,
    };
    if let Some(output_path) = output_path {
        write_json_new(&report, output_path)?;
    }
    Ok(report)
}

fn load_registry(
    contract_path: &Path,
    binding: &LocalFileHash,
) -> Result<(ApprovalAuthorityRegistry, String)> {
    require_hash(&binding.sha256)?;
    let bytes = read_contract_bytes(&resolve(contract_path, &binding.path), "authority registry")?;
    let registry_sha256 = hash_bytes(&bytes);
    if registry_sha256 != binding.sha256 {
        bail!("authority registry hash does not match the pinned digest");
    }
    let registry: ApprovalAuthorityRegistry = parse_json_strict(&bytes, "authority registry")?;
    validate_registry(&registry)?;
    Ok((registry, registry_sha256))
}

fn validate_registry(registry: &ApprovalAuthorityRegistry) -> Result<()> {
    require_schema(&registry.schema, REGISTRY_SCHEMA)?;
    require_token("registry owner id", &registry.owner_id)?;
    require_token("registry id", &registry.registry_id)?;
    if registry.entries.is_empty() {
        bail!("authority registry must contain at least one entry");
    }
    let mut identity_tuples = HashSet::new();
    let mut key_to_approver: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for entry in &registry.entries {
        require_token("registry approver id", &entry.approver_id)?;
        require_token("registry role", &entry.role)?;
        let public_key_bytes = decode_hex::<32>(&entry.public_key, "registry public key")?;
        VerifyingKey::from_bytes(&public_key_bytes)
            .map_err(|_| anyhow!("registry public key is not a valid Ed25519 point"))?;
        require_hash(&entry.key_id)?;
        if entry.key_id != hash_bytes(&public_key_bytes) {
            bail!("registry key id is not the SHA-256 of the public key");
        }
        if entry.scopes.is_empty() {
            bail!("registry entry must authorize at least one scope");
        }
        let mut scope_set = BTreeSet::new();
        for scope in &entry.scopes {
            require_token("registry scope", scope)?;
            if !scope_set.insert(scope.clone()) {
                bail!("registry entry lists a duplicate scope");
            }
        }
        if entry.decisions.is_empty() {
            bail!("registry entry must authorize at least one decision");
        }
        let mut decision_set = BTreeSet::new();
        for decision in &entry.decisions {
            Decision::parse(decision)?;
            if !decision_set.insert(decision.clone()) {
                bail!("registry entry lists a duplicate decision");
            }
        }
        if !identity_tuples.insert((
            entry.public_key.clone(),
            entry.approver_id.clone(),
            entry.role.clone(),
        )) {
            bail!("authority registry has a duplicate key, approver, and role entry");
        }
        match key_to_approver.get(&entry.public_key) {
            Some(existing) if existing != &entry.approver_id => {
                bail!("a public key must map to a single approver identity");
            }
            _ => {
                key_to_approver.insert(entry.public_key.clone(), entry.approver_id.clone());
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ensure_same_authority_context(
    prior: &SignedApprovalAttestation,
    owner_id: &str,
    registry_id: &str,
    registry_sha256: &str,
    authority_context_sha256: &str,
    target_kind: &str,
    target_sha256: &str,
    policy_sha256: &str,
    scope: &str,
) -> Result<()> {
    if prior.owner_id != owner_id
        || prior.registry_id != registry_id
        || prior.registry_sha256 != registry_sha256
        || prior.authority_context_sha256 != authority_context_sha256
        || prior.target_kind != target_kind
        || prior.target_sha256 != target_sha256
        || prior.policy_sha256 != policy_sha256
        || prior.scope != scope
    {
        bail!("prior attestation does not share the exact authority context");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn authorized_entry<'a>(
    registry: &'a ApprovalAuthorityRegistry,
    public_key: &str,
    key_id: &str,
    approver_id: &str,
    role: &str,
    scope: &str,
    decision: &str,
) -> Result<&'a AuthorityEntry> {
    for entry in &registry.entries {
        if entry.public_key == public_key
            && entry.key_id == key_id
            && entry.approver_id == approver_id
            && entry.role == role
            && entry.scopes.iter().any(|value| value == scope)
            && entry.decisions.iter().any(|value| value == decision)
        {
            return Ok(entry);
        }
    }
    bail!("no registry entry authorizes this key, approver, role, scope, and decision");
}

fn validate_attestation_structure(attestation: &SignedApprovalAttestation) -> Result<Decision> {
    require_schema(&attestation.schema, ATTESTATION_SCHEMA)?;
    require_token("owner id", &attestation.owner_id)?;
    require_token("registry id", &attestation.registry_id)?;
    require_hash(&attestation.registry_sha256)?;
    require_hash(&attestation.authority_context_sha256)?;
    require_token("approver id", &attestation.approver_id)?;
    require_token("role", &attestation.role)?;
    let public_key_bytes = decode_hex::<32>(&attestation.public_key, "public key")?;
    VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|_| anyhow!("attestation public key is not a valid Ed25519 point"))?;
    require_hash(&attestation.key_id)?;
    if attestation.key_id != hash_bytes(&public_key_bytes) {
        bail!("attestation key id is not the SHA-256 of the public key");
    }
    require_token("target kind", &attestation.target_kind)?;
    require_hash(&attestation.target_sha256)?;
    require_hash(&attestation.policy_sha256)?;
    require_token("scope", &attestation.scope)?;
    let decision = Decision::parse(&attestation.decision)?;
    if attestation.sequence == 0 {
        bail!("attestation sequence must be a positive integer");
    }
    require_utc(&attestation.issued_at_utc)?;
    require_utc(&attestation.expires_at_utc)?;
    if attestation.issued_at_utc >= attestation.expires_at_utc {
        bail!("attestation issued_at_utc must strictly precede expires_at_utc");
    }
    if let Some(prior) = attestation.prior_attestation_sha256.as_deref() {
        require_hash(prior)?;
    } else if decision == Decision::Revoked {
        bail!("a revoked attestation must reference its prior attestation");
    }
    let _ = decode_hex::<64>(&attestation.signature, "signature")?;
    Ok(decision)
}

fn verify_signature(attestation: &SignedApprovalAttestation) -> Result<()> {
    let public_key_bytes = decode_hex::<32>(&attestation.public_key, "public key")?;
    let verifying_key = VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|_| anyhow!("attestation public key is not a valid Ed25519 point"))?;
    let signature_bytes = decode_hex::<64>(&attestation.signature, "signature")?;
    let signature = Signature::from_bytes(&signature_bytes);
    let message = canonical_bytes(attestation);
    verifying_key
        .verify_strict(&message, &signature)
        .map_err(|_| anyhow!("attestation signature is not cryptographically valid"))?;
    Ok(())
}

fn canonical_bytes(attestation: &SignedApprovalAttestation) -> Vec<u8> {
    let sequence = attestation.sequence.to_string();
    let fields: [(&str, Option<&str>); 18] = [
        ("schema", Some(attestation.schema.as_str())),
        ("owner_id", Some(attestation.owner_id.as_str())),
        ("registry_id", Some(attestation.registry_id.as_str())),
        (
            "registry_sha256",
            Some(attestation.registry_sha256.as_str()),
        ),
        (
            "authority_context_sha256",
            Some(attestation.authority_context_sha256.as_str()),
        ),
        ("target_kind", Some(attestation.target_kind.as_str())),
        ("target_sha256", Some(attestation.target_sha256.as_str())),
        ("policy_sha256", Some(attestation.policy_sha256.as_str())),
        ("scope", Some(attestation.scope.as_str())),
        ("decision", Some(attestation.decision.as_str())),
        ("role", Some(attestation.role.as_str())),
        ("approver_id", Some(attestation.approver_id.as_str())),
        ("public_key", Some(attestation.public_key.as_str())),
        ("key_id", Some(attestation.key_id.as_str())),
        ("issued_at_utc", Some(attestation.issued_at_utc.as_str())),
        ("expires_at_utc", Some(attestation.expires_at_utc.as_str())),
        ("sequence", Some(sequence.as_str())),
        (
            "prior_attestation_sha256",
            attestation.prior_attestation_sha256.as_deref(),
        ),
    ];
    let mut out = Vec::new();
    write_chunk(&mut out, CANONICAL_DOMAIN);
    for (label, value) in fields {
        write_chunk(&mut out, label.as_bytes());
        match value {
            Some(value) => {
                out.push(0x01);
                write_chunk(&mut out, value.as_bytes());
            }
            None => out.push(0x00),
        }
    }
    out
}

fn write_chunk(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn read_signing_key(path: &Path) -> Result<SigningKey> {
    let mut seed = [0u8; SIGNING_KEY_LEN];
    let mut extra = [0u8; 1];
    let result = (|| {
        let mut file = File::open(path)
            .with_context(|| format!("failed to open signing key {}", path.display()))?;
        file.read_exact(&mut seed)
            .with_context(|| format!("failed to read signing key {}", path.display()))?;
        if file
            .read(&mut extra)
            .with_context(|| format!("failed to read signing key {}", path.display()))?
            != 0
        {
            bail!("signing key must be exactly 32 raw Ed25519 bytes");
        }
        Ok(SigningKey::from_bytes(&seed))
    })();
    seed.zeroize();
    extra.zeroize();
    result
}

fn write_json_new<T: Serialize>(value: &T, output: &Path) -> Result<()> {
    if output.exists() {
        bail!("refusing to overwrite {}", output.display());
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = Builder::new()
        .prefix(".reel-approval-")
        .tempfile_in(parent)?;
    temporary.write_all(format!("{}\n", serde_json::to_string_pretty(value)?).as_bytes())?;
    temporary.flush()?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output.display()))?;
    Ok(())
}

fn read_contract_bytes(path: &Path, label: &str) -> Result<Vec<u8>> {
    let file =
        File::open(path).with_context(|| format!("failed to open {label} {}", path.display()))?;
    let mut reader = file.take(MAX_CONTRACT_BYTES + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {label} {}", path.display()))?;
    if bytes.len() as u64 > MAX_CONTRACT_BYTES {
        bail!("{label} exceeds the {MAX_CONTRACT_BYTES} byte contract bound");
    }
    Ok(bytes)
}

fn parse_json_strict<T: DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T> {
    ensure_no_duplicate_keys(bytes)
        .with_context(|| format!("{label} contains duplicate JSON fields"))?;
    serde_json::from_slice(bytes).with_context(|| format!("{label} is not valid strict JSON"))
}

fn resolve(contract_path: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        contract_path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

fn require_schema(actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("unsupported schema {actual}; expected {expected}");
    }
    Ok(())
}

fn require_token(field: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_TOKEN_LEN {
        bail!("{field} must be a nonempty token of at most {MAX_TOKEN_LEN} characters");
    }
    if value == "." || value == ".." {
        bail!("{field} must be a portable identifier");
    }
    for character in value.chars() {
        if !character.is_ascii_graphic() || matches!(character, '/' | '\\' | '*' | '?') {
            bail!(
                "{field} must be a portable token without whitespace, wildcards, or path separators"
            );
        }
    }
    Ok(())
}

fn require_hash(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        bail!("expected a lowercase SHA-256 digest");
    }
    Ok(())
}

fn require_utc(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        bail!("timestamp must be canonical UTC YYYY-MM-DDTHH:MM:SSZ");
    }
    for &index in &[0usize, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !bytes[index].is_ascii_digit() {
            bail!("timestamp must be canonical UTC YYYY-MM-DDTHH:MM:SSZ");
        }
    }
    let field = |start: usize, end: usize| value[start..end].parse::<u32>().unwrap_or(u32::MAX);
    let year = field(0, 4);
    let month = field(5, 7);
    let day = field(8, 10);
    let hour = field(11, 13);
    let minute = field(14, 16);
    let second = field(17, 19);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > max_day
        || hour > 23
        || minute > 59
        || second > 59
    {
        bail!("timestamp has an out-of-range field");
    }
    Ok(())
}

fn decode_hex<const N: usize>(value: &str, label: &str) -> Result<[u8; N]> {
    if value.len() != N * 2 {
        bail!("{label} must be {} lowercase hex characters", N * 2);
    }
    let bytes = value.as_bytes();
    let mut out = [0u8; N];
    for index in 0..N {
        let high = hex_value(bytes[index * 2])
            .ok_or_else(|| anyhow!("{label} must be lowercase hexadecimal"))?;
        let low = hex_value(bytes[index * 2 + 1])
            .ok_or_else(|| anyhow!("{label} must be lowercase hexadecimal"))?;
        out[index] = (high << 4) | low;
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    finalize_hex(hasher)
}

fn hash_file_streaming(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(finalize_hex(hasher))
}

fn finalize_hex(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn ensure_no_duplicate_keys(bytes: &[u8]) -> Result<()> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    StrictShape::deserialize(&mut deserializer).map_err(|error| anyhow!("{error}"))?;
    deserializer.end().map_err(|error| anyhow!("{error}"))?;
    Ok(())
}

struct StrictShape;

impl<'de> Deserialize<'de> for StrictShape {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictShapeVisitor)
    }
}

struct StrictShapeVisitor;

impl<'de> Visitor<'de> for StrictShapeVisitor {
    type Value = StrictShape;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("strict JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_i128<E>(self, _value: i128) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_u128<E>(self, _value: u128) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(StrictShapeVisitor)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<StrictShape>()?.is_some() {}
        Ok(StrictShape)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate JSON field: {key}")));
            }
            map.next_value::<StrictShape>()?;
        }
        Ok(StrictShape)
    }
}

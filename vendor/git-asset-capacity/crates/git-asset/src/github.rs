//! Authenticated request provenance, independent of storage authority/current head.
//! No implicit network, provider access or durable operation journal.
#[cfg(test)]
#[path = "../tests/common/github_fixture.rs"]
pub(crate) mod fixture;
pub mod http;
pub mod intake;
pub mod placement;
pub mod source;
use crate::{lifecycle::Enrollment, source::GitId};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use git_asset_core::{Digest, identity};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::BTreeSet,
    time::{Duration, Instant},
};
pub const ISSUER: &str = "https://token.actions.githubusercontent.com";
pub const JWKS: &str = "https://token.actions.githubusercontent.com/.well-known/jwks";
pub const MAX_TOKEN: usize = 16384;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    Invalid,
    Budget,
    Trust,
    Key,
    Signature,
    Claims,
    Clock,
    Expired,
    Stale,
    Unavailable,
    Disabled,
    Denied,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn json(bytes: &[u8]) -> Result<serde_json::Value> {
    identity::parse_json(bytes).map_err(|_| Error::Invalid)
}
fn hash(v: &impl Serialize) -> Result<Digest> {
    let bytes = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if bytes.len() > 65536 {
        return Err(Error::Budget);
    }
    Ok(Digest::blob(&bytes))
}
fn decode(s: &str, max: usize) -> Result<Vec<u8>> {
    if s.is_empty() || s.len() > max * 2 {
        return Err(Error::Budget);
    }
    let b = URL_SAFE_NO_PAD.decode(s).map_err(|_| Error::Invalid)?;
    if b.len() > max {
        return Err(Error::Budget);
    }
    if URL_SAFE_NO_PAD.encode(&b) != s {
        return Err(Error::Invalid);
    }
    Ok(b)
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 20
        && !s.starts_with('0')
        && s.bytes().all(|c| c.is_ascii_digit())
        && s.parse::<u64>().is_ok()
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
fn reference(s: &str) -> bool {
    s.starts_with("refs/heads/")
        && text(s, 512)
        && !s.ends_with(['.', '/'])
        && !s.contains([' ', '\\', '~', '^', ':', '?', '*', '['])
        && !s.contains("..")
        && !s.contains("@{")
        && s.split('/')
            .all(|v| !v.is_empty() && !v.starts_with('.') && !v.ends_with(".lock"))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicKey {
    pub kid: String,
    pub n: String,
    pub e: String,
}
impl PublicKey {
    fn components(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        if !text(&self.kid, 128) {
            return Err(Error::Key);
        }
        let n = decode(&self.n, 1024)?;
        let e = decode(&self.e, 8)?;
        if !(256..=1024).contains(&n.len()) || n[0] == 0 || e != [1, 0, 1] {
            return Err(Error::Key);
        }
        Ok((n, e))
    }
}
/// Trusted operator/TLS-origin snapshot, never a key set supplied by a request.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySnapshot {
    pub schema: String,
    pub issuer: String,
    pub valid_from: u64,
    pub valid_until: u64,
    pub keys: Vec<PublicKey>,
}
impl KeySnapshot {
    pub fn validate(&self, now: u64) -> Result<Digest> {
        if self.schema != "tig.github-keys.design-1"
            || self.issuer != ISSUER
            || self.keys.is_empty()
            || self.keys.len() > 8
        {
            return Err(Error::Key);
        }
        if self.valid_until <= self.valid_from
            || self.valid_until - self.valid_from > 86400
            || now < self.valid_from
            || now >= self.valid_until
        {
            return Err(Error::Expired);
        }
        let mut ids = BTreeSet::new();
        for key in &self.keys {
            key.components()?;
            if !ids.insert(&key.kid) {
                return Err(Error::Key);
            }
        }
        hash(self)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reusable {
    pub workflow_ref: String,
    pub workflow_sha: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trust {
    pub schema: String,
    pub audience: String,
    pub subject: String,
    pub repository: String,
    pub repository_id: String,
    pub owner_id: String,
    pub ref_name: String,
    pub workflow_ref: String,
    pub reusable: Option<Reusable>,
    pub events: Vec<String>,
    pub max_token_seconds: u64,
    pub clock_skew_seconds: u64,
    pub selection_enrollment: Digest,
    pub placement_enrollment: Digest,
}
impl Trust {
    pub fn id(&self) -> Result<Digest> {
        let repo = self.repository.split('/').collect::<Vec<_>>();
        if self.schema != "tig.github-trust.design-1"
            || !text(&self.audience, 512)
            || !text(&self.subject, 1024)
            || repo.len() != 2
            || repo.iter().any(|v| {
                v.is_empty()
                    || v.len() > 100
                    || v.starts_with('.')
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
            || !identifier(&self.repository_id)
            || !identifier(&self.owner_id)
            || !reference(&self.ref_name)
            || !self
                .workflow_ref
                .starts_with(&format!("{}/.github/workflows/", self.repository))
            || !self.workflow_ref.ends_with(&format!("@{}", self.ref_name))
            || !text(&self.workflow_ref, 1024)
            || self.events.is_empty()
            || self.events.len() > 3
            || self.max_token_seconds == 0
            || self.max_token_seconds > 600
            || self.clock_skew_seconds > 30
        {
            return Err(Error::Trust);
        }
        let mut events = BTreeSet::new();
        for event in &self.events {
            if !matches!(event.as_str(), "push" | "workflow_dispatch" | "schedule")
                || !events.insert(event)
            {
                return Err(Error::Trust);
            }
        }
        if let Some(r) = &self.reusable
            && (!text(&r.workflow_ref, 1024)
                || !r.workflow_ref.ends_with(&format!("@{}", r.workflow_sha))
                || GitId::parse(&r.workflow_sha).is_err())
        {
            return Err(Error::Trust);
        }
        hash(self)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    kid: String,
    typ: String,
    #[serde(default)]
    x5t: Option<String>,
    #[serde(default, rename = "x5t#S256")]
    x5t_s256: Option<String>,
}
#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    sub: String,
    repository: String,
    repository_id: String,
    repository_owner_id: String,
    #[serde(rename = "ref")]
    ref_name: String,
    ref_type: String,
    sha: String,
    event_name: String,
    runner_environment: String,
    workflow_ref: String,
    workflow_sha: String,
    job_workflow_ref: Option<String>,
    job_workflow_sha: Option<String>,
    run_id: String,
    run_attempt: String,
    jti: String,
    exp: u64,
    iat: u64,
    nbf: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Provenance {
    pub trust: Digest,
    pub key_snapshot: Digest,
    pub run_id: String,
    pub run_attempt: String,
    pub token_id: Digest,
    pub commit: String,
    pub expires: u64,
}
/// Cannot be deserialized or constructed from claimed event JSON. This only
/// admits an exact request to the independently trusted executor/source adapter.
pub struct Verified {
    commit: GitId,
    selection: Digest,
    placement: Digest,
    provenance: Provenance,
    accepted: u64,
    last_clock: Cell<u64>,
    start: Instant,
    lifetime: Duration,
}
impl std::fmt::Debug for Verified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedGitHubRequest([redacted])")
    }
}
impl Verified {
    pub fn commit(&self) -> &GitId {
        &self.commit
    }
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    pub fn admit(&self, now: u64, selection: &Digest, placement: &Digest) -> Result<()> {
        if now < self.accepted || now < self.last_clock.get() {
            return Err(Error::Clock);
        }
        self.last_clock.set(now);
        if now >= self.provenance.expires || self.start.elapsed() >= self.lifetime {
            return Err(Error::Expired);
        }
        if selection != &self.selection || placement != &self.placement {
            return Err(Error::Trust);
        }
        Ok(())
    }
    pub fn selection_request(
        &self,
        enrollment: &Enrollment,
        placement: &Digest,
        now: u64,
    ) -> Result<serde_json::Value> {
        self.admit(now, &enrollment.id().map_err(|_| Error::Trust)?, placement)?;
        Ok(
            serde_json::json!({"schema":"tig.git-lifecycle-request.design-1","repository":enrollment.repository,"ref_name":enrollment.ref_name,"commit":self.commit.as_str()}),
        )
    }
}
pub fn verify(
    trust: &Trust,
    keys: &KeySnapshot,
    token: &str,
    requested_commit: &GitId,
    now: u64,
) -> Result<Verified> {
    let trust_id = trust.id()?;
    let snapshot = keys.validate(now)?;
    if token.len() > MAX_TOKEN {
        return Err(Error::Budget);
    }
    let pieces = token.split('.').collect::<Vec<_>>();
    if pieces.len() != 3 {
        return Err(Error::Invalid);
    }
    let header: Header =
        serde_json::from_value(json(&decode(pieces[0], 1024)?)?).map_err(|_| Error::Invalid)?;
    if header.alg != "RS256"
        || header.typ != "JWT"
        || !text(&header.kid, 128)
        || header
            .x5t
            .iter()
            .chain(&header.x5t_s256)
            .any(|v| !text(v, 128))
    {
        return Err(Error::Invalid);
    }
    let key = keys
        .keys
        .iter()
        .find(|k| k.kid == header.kid)
        .ok_or(Error::Key)?;
    let (n, e) = key.components()?;
    let signature = decode(pieces[2], 1024)?;
    let message_len = pieces[0].len() + 1 + pieces[1].len();
    RsaPublicKeyComponents { n, e }
        .verify(
            &RSA_PKCS1_2048_8192_SHA256,
            &token.as_bytes()[..message_len],
            &signature,
        )
        .map_err(|_| Error::Signature)?;
    let claims: Claims =
        serde_json::from_value(json(&decode(pieces[1], 12288)?)?).map_err(|_| Error::Claims)?;
    if claims.iss != ISSUER
        || claims.aud != trust.audience
        || claims.sub != trust.subject
        || claims.repository != trust.repository
        || claims.repository_id != trust.repository_id
        || claims.repository_owner_id != trust.owner_id
        || claims.ref_name != trust.ref_name
        || claims.ref_type != "branch"
        || claims.workflow_ref != trust.workflow_ref
        || GitId::parse(&claims.workflow_sha).is_err()
        || claims.runner_environment != "github-hosted"
        || !trust.events.contains(&claims.event_name)
        || !identifier(&claims.run_id)
        || !identifier(&claims.run_attempt)
        || !text(&claims.jti, 256)
    {
        return Err(Error::Claims);
    }
    if let Some(expected) = &trust.reusable
        && (claims.job_workflow_ref.as_ref() != Some(&expected.workflow_ref)
            || claims.job_workflow_sha.as_ref() != Some(&expected.workflow_sha))
    {
        return Err(Error::Claims);
    }
    let commit = GitId::parse(&claims.sha).map_err(|_| Error::Claims)?;
    if &commit != requested_commit {
        return Err(Error::Stale);
    }
    if claims.exp <= claims.iat
        || claims.exp - claims.iat > trust.max_token_seconds
        || claims.nbf > claims.exp
    {
        return Err(Error::Clock);
    }
    if now >= claims.exp {
        return Err(Error::Expired);
    }
    if claims.iat > now.saturating_add(trust.clock_skew_seconds)
        || claims.nbf > now.saturating_add(trust.clock_skew_seconds)
    {
        return Err(Error::Clock);
    }
    let expires = claims.exp.min(keys.valid_until);
    Ok(Verified {
        commit: commit.clone(),
        selection: trust.selection_enrollment.clone(),
        placement: trust.placement_enrollment.clone(),
        provenance: Provenance {
            trust: trust_id,
            key_snapshot: snapshot,
            run_id: claims.run_id,
            run_attempt: claims.run_attempt,
            token_id: Digest::blob(claims.jti.as_bytes()),
            commit: commit.as_str().into(),
            expires,
        },
        accepted: now,
        last_clock: Cell::new(now),
        start: Instant::now(),
        lifetime: Duration::from_secs(expires - now),
    })
}

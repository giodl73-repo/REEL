//! One create effect per durable operation, with explicit lookup reconciliation.
//! The configured proxy must enforce operation-id/payload uniqueness remotely.
#[path = "tracker_effect.rs"]
mod effect;
use super::{Config as HttpConfig, ResponseHead, error};
use effect::Effect;
pub use effect::{CloseConfig, CloseReceipt, Closed};
use reel_workflow::{
    ErrorKind, MAX_CONTROL_BYTES, Result, canonical_digest, decode, digest, tracker::Draft,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub registry: PathBuf,
    pub create: HttpConfig,
    pub lookup: HttpConfig,
    /// Explicit canonical issue URL prefix for each allowed owner/repository.
    pub repositories: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub operation_id: String,
    pub repository: String,
    pub request_sha256: String,
    pub issue_id: String,
    pub issue_url: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Delivered<R = Receipt> {
    pub schema: String,
    pub operation_id: String,
    pub draft_sha256: String,
    pub evidence_key: String,
    pub receipt: R,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Call {
    schema: String,
    operation_id: String,
    create_operation_id: String,
    config_sha256: String,
    draft_sha256: String,
    request_sha256: String,
    request_bytes: u64,
    query_id: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    schema: String,
    status: u16,
    body_sha256: String,
    body_bytes: u64,
}
struct Registry {
    root: PathBuf,
    _directories: Vec<File>,
    _lock: File,
}
fn ordinary(meta: &fs::Metadata, directory: bool) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    #[cfg(not(windows))]
    if meta.file_type().is_symlink() {
        return false;
    }
    if directory {
        meta.is_dir()
    } else {
        meta.is_file()
    }
}
fn options(write: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    if write {
        options.write(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3).custom_flags(0x0020_0000);
    }
    options
}
impl Registry {
    fn open(root: &Path, op: &str) -> Result<Self> {
        if !root.is_absolute()
            || root.as_os_str().len() > 4096
            || root.components().count() > 128
            || root
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(error(ErrorKind::Invalid, "tracker-registry-path"));
        }
        let mut directories = Vec::new();
        let mut ancestors: Vec<_> = root.ancestors().collect();
        ancestors.reverse();
        for path in ancestors {
            if !ordinary(
                &fs::symlink_metadata(path)
                    .map_err(|_| error(ErrorKind::Denied, "tracker-registry-directory"))?,
                true,
            ) {
                return Err(error(ErrorKind::Denied, "tracker-registry-directory"));
            }
            let mut options = options(false);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.custom_flags(0x0220_0000);
            }
            let held = options
                .open(path)
                .map_err(|_| error(ErrorKind::Denied, "tracker-registry-directory"))?;
            if !ordinary(
                &held
                    .metadata()
                    .map_err(|_| error(ErrorKind::Denied, "tracker-registry-directory"))?,
                true,
            ) {
                return Err(error(ErrorKind::Denied, "tracker-registry-directory"));
            }
            directories.push(held);
        }
        let lock = options(true)
            .create(true)
            .truncate(false)
            .open(root.join(format!("{op}.lock")))
            .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-lock-open"))?;
        if !ordinary(
            &lock
                .metadata()
                .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-lock-file"))?,
            false,
        ) {
            return Err(error(ErrorKind::Denied, "tracker-lock-file"));
        }
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|e| {
            error(
                if e.kind() == std::io::ErrorKind::WouldBlock || e.raw_os_error() == Some(33) {
                    ErrorKind::Busy
                } else {
                    ErrorKind::NeedsRecovery
                },
                "tracker-operation-lock",
            )
        })?;
        Ok(Self {
            root: root.to_path_buf(),
            _directories: directories,
            _lock: lock,
        })
    }
    fn path(&self, key: &str, suffix: &str) -> PathBuf {
        self.root.join(format!("{key}.{suffix}.json"))
    }
    fn exists(&self, key: &str, suffix: &str) -> bool {
        fs::symlink_metadata(self.path(key, suffix)).is_ok()
    }
    fn read(&self, key: &str, suffix: &str) -> Result<Vec<u8>> {
        let mut file = options(false)
            .open(self.path(key, suffix))
            .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-control-open"))?;
        let meta = file
            .metadata()
            .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-control-metadata"))?;
        if !ordinary(&meta, false) || meta.len() > MAX_CONTROL_BYTES as u64 {
            return Err(error(ErrorKind::NeedsRecovery, "tracker-control-file"));
        }
        let mut raw = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_CONTROL_BYTES as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-control-read"))?;
        if raw.len() > MAX_CONTROL_BYTES {
            return Err(error(ErrorKind::Budget, "tracker-control-bytes"));
        }
        Ok(raw)
    }
    fn create(&self, key: &str, suffix: &str) -> Result<File> {
        options(true)
            .create_new(true)
            .open(self.path(key, suffix))
            .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-control-create-preserve"))
    }
    fn store(&self, key: &str, suffix: &str, value: &impl Serialize) -> Result<()> {
        let raw = serde_json::to_vec(value)
            .map_err(|_| error(ErrorKind::Invalid, "tracker-control-json"))?;
        if raw.len() > MAX_CONTROL_BYTES {
            return Err(error(ErrorKind::Budget, "tracker-control-bytes"));
        }
        flush(&mut self.create(key, suffix)?, &raw)
    }
}
fn flush(file: &mut File, bytes: &[u8]) -> Result<()> {
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-control-write-preserve"))
}
impl Config {
    pub fn submit(
        &self,
        draft: &Draft,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<Delivered> {
        self.submit_effect(draft, resolve, true)
    }
    pub fn reconcile(
        &self,
        draft: &Draft,
        query: &str,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<Delivered> {
        self.reconcile_effect(draft, query, resolve)
    }
    /// Read-only, full original request/response/config validation. A serialized
    /// receipt alone is insufficient. Lost/incomplete delivery must reconcile.
    pub fn verified_delivery(&self, draft: &Draft) -> Result<Delivered> {
        self.check(draft)?;
        let registry = Registry::open(&self.registry, &draft.operation_id)?;
        self.delivered(&registry, draft)?.ok_or_else(|| {
            error(
                ErrorKind::NeedsRecovery,
                "tracker-verified-delivery-required",
            )
        })
    }
    fn check<E: Effect>(&self, draft: &E) -> Result<()> {
        draft.check()?;
        if self.schema != "reel.workflow-tracker.v1"
            || self.repositories.is_empty()
            || self.repositories.len() > 128
        {
            return Err(error(ErrorKind::Invalid, "tracker-config"));
        }
        for (repo, prefix) in &self.repositories {
            let pieces: Vec<_> = repo.split('/').collect();
            if prefix.len() > 8192 {
                return Err(error(ErrorKind::Budget, "tracker-issue-url-prefix"));
            }
            let url = reqwest::Url::parse(prefix)
                .map_err(|_| error(ErrorKind::Invalid, "tracker-issue-url-prefix"))?;
            if pieces.len() != 2
                || pieces.iter().any(|s| {
                    s.is_empty()
                        || s.len() > 128
                        || !s
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                        || *s == "."
                        || *s == ".."
                })
                || repo != &repo.to_ascii_lowercase()
                || url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || !prefix.ends_with('/')
                || url.as_str() != prefix
            {
                return Err(error(ErrorKind::Invalid, "tracker-issue-url-prefix"));
            }
        }
        if !self.repositories.contains_key(draft.repository()) {
            return Err(error(ErrorKind::Denied, "tracker-repository-not-allowed"));
        }
        draft.check_target(&self.repositories[draft.repository()])?;
        for http in [&self.create, &self.lookup] {
            if http.request_byte_limit > MAX_CONTROL_BYTES as u64
                || http.response_byte_limit > MAX_CONTROL_BYTES as u64
                || http.timeout_seconds > 300
            {
                return Err(error(ErrorKind::Budget, "tracker-http-bounds"));
            }
        }
        Ok(())
    }
    fn call<E: Effect>(&self, draft: &E, query: Option<&str>) -> Result<(Call, Vec<u8>)> {
        let payload = draft.payload()?;
        let (op, body) = if let Some(query) = query {
            if !super::token(query) {
                return Err(error(ErrorKind::Invalid, "tracker-query-id"));
            }
            (canonical_digest(&(E::LOOKUP_OPERATION,draft.operation_id(),query))?,serde_json::to_vec(&serde_json::json!({"schema":E::LOOKUP,"operation_id":draft.operation_id(),"repository":draft.repository(),"request_sha256":digest(&payload)})).map_err(|_|error(ErrorKind::Invalid,"tracker-query-json"))?)
        } else {
            (draft.operation_id().to_owned(), payload)
        };
        Ok((
            Call {
                schema: E::CALL.into(),
                operation_id: op,
                create_operation_id: draft.operation_id().to_owned(),
                config_sha256: canonical_digest(self)?,
                draft_sha256: canonical_digest(draft)?,
                request_sha256: digest(&body),
                request_bytes: body.len() as u64,
                query_id: query.map(str::to_owned),
            },
            body,
        ))
    }
    fn original<E: Effect>(&self, registry: &Registry, draft: &E, key: &str) -> Result<Call> {
        let raw = registry.read(key, "intent")?;
        let recorded: Call = decode(&raw).map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "tracker-original-intent-unreadable",
            )
        })?;
        let (expected, body) = self.call(draft, recorded.query_id.as_deref())?;
        if expected.operation_id != key
            || canonical_digest(&expected)? != canonical_digest(&recorded)?
        {
            return Err(error(
                ErrorKind::Conflict,
                "tracker-original-intent-binding",
            ));
        }
        if registry.read(key, "request")? != body {
            return Err(error(
                ErrorKind::Integrity,
                "tracker-original-request-content",
            ));
        }
        Ok(recorded)
    }
    fn finish<E: Effect>(&self, registry: &Registry, draft: &E, key: &str) -> Result<E::Receipt> {
        self.original(registry, draft, key)?;
        let complete: Complete = decode(&registry.read(key, "complete")?)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-original-completion"))?;
        let raw = registry.read(key, "response")?;
        if complete.schema != E::RESPONSE
            || complete.body_bytes != raw.len() as u64
            || complete.body_sha256 != digest(&raw)
        {
            return Err(error(ErrorKind::Integrity, "tracker-response-content"));
        }
        if !(200..300).contains(&complete.status) {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "tracker-remote-outcome-uncertain",
            ));
        }
        let receipt: E::Receipt = decode(&raw).map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "tracker-remote-receipt-unreadable",
            )
        })?;
        draft.receipt(&receipt, &self.repositories[draft.repository()])?;
        Ok(receipt)
    }
    fn delivered<E: Effect>(
        &self,
        registry: &Registry,
        draft: &E,
    ) -> Result<Option<Delivered<E::Receipt>>> {
        if !registry.exists(draft.operation_id(), "receipt") {
            return Ok(None);
        }
        let record: Delivered<E::Receipt> =
            decode(&registry.read(draft.operation_id(), "receipt")?)
                .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-local-receipt-unreadable"))?;
        if record.schema != E::DELIVERY
            || record.operation_id != draft.operation_id()
            || record.draft_sha256 != canonical_digest(draft)?
            || record.evidence_key.len() != 64
            || !record
                .evidence_key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || record.receipt != self.finish(registry, draft, &record.evidence_key)?
        {
            return Err(error(ErrorKind::Integrity, "tracker-local-receipt-binding"));
        }
        Ok(Some(record))
    }
    fn persist_receipt<E: Effect>(
        &self,
        registry: &Registry,
        draft: &E,
        key: &str,
    ) -> Result<Delivered<E::Receipt>> {
        let record = Delivered {
            schema: E::DELIVERY.into(),
            operation_id: draft.operation_id().to_owned(),
            draft_sha256: canonical_digest(draft)?,
            evidence_key: key.into(),
            receipt: self.finish(registry, draft, key)?,
        };
        registry.store(draft.operation_id(), "receipt", &record)?;
        Ok(record)
    }
    fn send<E: Effect>(
        &self,
        registry: &Registry,
        draft: &E,
        query: Option<&str>,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<String> {
        let (record, body) = self.call(draft, query)?;
        let key = &record.operation_id;
        if ["request", "intent", "head", "response", "complete"]
            .iter()
            .any(|suffix| registry.exists(key, suffix))
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "tracker-original-call-exists",
            ));
        }
        if fs::read_dir(&registry.root)
            .map_err(|_| error(ErrorKind::Denied, "tracker-registry-list"))?
            .take(4097)
            .count()
            + 6
            + E::extra_files()
            > 4096
        {
            return Err(error(ErrorKind::Budget, "tracker-registry-entry-limit"));
        }
        let http = if query.is_some() {
            &self.lookup
        } else {
            &self.create
        };
        let prepared = http.prepare(key, &body, resolve)?;
        flush(&mut registry.create(key, "request")?, &body)?;
        let mut output = registry.create(key, "response")?;
        let response = prepared.send_observed(
            |intent| {
                if intent.operation_id != *key
                    || intent.request_sha256 != record.request_sha256
                    || intent.request_bytes != record.request_bytes
                    || intent.config_sha256 != canonical_digest(http)?
                {
                    return Err(error(ErrorKind::Integrity, "tracker-transport-intent"));
                }
                registry.store(key, "intent", &record)?;
                if query.is_none() {
                    draft.consume(registry)?;
                }
                Ok(())
            },
            |head: &ResponseHead| registry.store(key, "head", head),
            |chunk| flush(&mut output, chunk),
        )?;
        registry.store(
            key,
            "complete",
            &Complete {
                schema: E::RESPONSE.into(),
                status: response.status,
                body_sha256: response.body_sha256,
                body_bytes: response.body.len() as u64,
            },
        )?;
        Ok(key.clone())
    }
    /// A consumed create intent is never sent again, even on HTTP failure.
    fn submit_effect<E: Effect>(
        &self,
        draft: &E,
        resolve: impl Fn(&str) -> Option<String>,
        fresh: bool,
    ) -> Result<Delivered<E::Receipt>> {
        self.check(draft)?;
        let registry = Registry::open(&self.registry, draft.operation_id())?;
        if registry.exists(draft.operation_id(), "intent") {
            self.original(&registry, draft, draft.operation_id())?;
            if let Some(receipt) = self.delivered(&registry, draft)? {
                return Ok(receipt);
            }
            return self.persist_receipt(&registry, draft, draft.operation_id());
        }
        if !fresh {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "tracker-original-effect-uncertain",
            ));
        }
        let key = self.send(&registry, draft, None, resolve)?;
        self.persist_receipt(&registry, draft, &key)
    }
    /// Proxy lookup MUST be read-only. Not-found/uncertain lookup never permits
    /// another create. A fresh query id permits another explicit lookup only.
    fn reconcile_effect<E: Effect>(
        &self,
        draft: &E,
        query: &str,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<Delivered<E::Receipt>> {
        self.check(draft)?;
        let registry = Registry::open(&self.registry, draft.operation_id())?;
        self.original(&registry, draft, draft.operation_id())?;
        if let Some(receipt) = self.delivered(&registry, draft)? {
            return Ok(receipt);
        }
        let (record, _) = self.call(draft, Some(query))?;
        let key = if registry.exists(&record.operation_id, "intent") {
            self.original(&registry, draft, &record.operation_id)?;
            record.operation_id
        } else {
            self.send(&registry, draft, Some(query), resolve)?
        };
        self.persist_receipt(&registry, draft, &key)
    }
}

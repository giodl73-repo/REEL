//! Adopt exact legacy bytes into one shared native annex pool. Original cache
//! names survive. This grants neither legacy removal nor retained custody.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tig_store::existing::{ByteSource, Profile as SourceProfile, ReadCache, RequiredProtection};

static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub pool: crate::annex::process::Enrollment,
    pub store: Digest,
    pub source: SourceProfile,
    pub object: Object,
    /// Owner admits immutable legacy bytes even after the verification guard
    /// ends. Hard links share later in-place mutations and file attributes.
    pub legacy_immutable: bool,
    pub maximum_bytes: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Adopted,
    AlreadyAdopted,
}
#[derive(Serialize)]
pub struct Report {
    pub status: Status,
    pub store: Digest,
    pub object: Object,
    pub shared_file_identity: bool,
    pub additional_retained_payload_bytes: u64,
    pub legacy_alias_retained: bool,
    pub custody: bool,
    pub removal_authorized: bool,
}
fn report(p: &Profile, status: Status) -> Report {
    Report {
        status,
        store: p.store.clone(),
        object: p.object.clone(),
        shared_file_identity: true,
        additional_retained_payload_bytes: 0,
        legacy_alias_retained: true,
        custody: false,
        removal_authorized: false,
    }
}
pub(super) fn stage(root: &Path) -> io::Result<PathBuf> {
    for _ in 0..16 {
        let path = root.join(format!(
            "tig-adopt-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e),
        }
    }
    Err(invalid())
}
fn validate(p: &Profile) -> io::Result<()> {
    if !cfg!(windows)
        || p.schema != "tig.annex-cache-adoption.experimental-1"
        || !p.legacy_immutable
        || p.source.places.len() != 1
        || p.source.required != RequiredProtection::PreventMutation
        || p.maximum_bytes == 0
        || p.maximum_bytes > 64 * 1024 * 1024 * 1024
        || p.object.bytes > p.maximum_bytes
        || p.source.limits.requests < 2
        || p.object
            .bytes
            .checked_mul(2)
            .is_none_or(|n| n > p.source.limits.total_bytes)
    {
        return Err(invalid());
    }
    // Owned native stores need their own pending/read gates, not reinterpretation
    // through an existing-layout reader.
    if p.source.places[0].root.join("store.json").try_exists()? {
        return Err(invalid());
    }
    Ok(())
}
/// No cache initialization or inventory. One exact existing object only, with
/// hard-link-only staging. Cross-volume/unsupported links fail without a copy.
pub fn adopt(p: &Profile) -> io::Result<Report> {
    validate(p)?;
    let pool = Runner::new(p.pool.clone())?;
    if uuid(&info(&pool, None)?)?.store() != p.store {
        return Err(invalid());
    }
    let key = Key::from_object(&p.object);
    let _admission = managed_guards(&[&pool], &key)?;
    let mut cache = ReadCache::new(p.source.clone()).map_err(|_| invalid())?;
    let source = cache.open_verified(&p.object).map_err(|_| invalid())?;
    let (identity, source_path) = source
        .with_path(|path| {
            Ok::<_, io::Error>((same_file::Handle::from_path(path)?, path.to_owned()))
        })
        .map_err(|_| invalid())??;
    if let Some(existing) = hold_copy(&pool, &key, p.maximum_bytes)? {
        if !existing.same_file(&identity)? {
            return Err(invalid()); // Never overwrite an existing separate copy.
        }
        return Ok(report(p, Status::AlreadyAdopted));
    }
    // This is annex staging, not a recreated payload layout. Annex alone chooses
    // and publishes the final object path.
    let git = if pool.repository().join(".git").is_dir() {
        pool.repository().join(".git")
    } else {
        pool.repository().to_owned()
    };
    let annex = git.join("annex");
    let _annex = crate::annex::legacy::hold_link_directories(&annex)?;
    let temporary = annex.join("tmp");
    match fs::create_dir(&temporary) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let _temporary = crate::annex::legacy::hold_link_directories(&temporary)?;
    let attempt = stage(&temporary)?;
    let _attempt = crate::annex::legacy::hold_link_directories(&attempt)?;
    let alias = attempt.join(key.as_str());
    let _source_parents =
        crate::annex::legacy::hold_link_directories(source_path.parent().ok_or_else(invalid)?)?;
    // Windows hard-link creation and rename need the strict read guard released.
    // Keep identity and namespace guards; immutable legacy writers are admitted.
    drop(source);
    fs::hard_link(&source_path, &alias)
        .map_err(|e| io::Error::new(e.kind(), format!("adoption link refused: {e}")))?;
    if same_file::Handle::from_path(&alias)? != identity {
        return Err(invalid());
    }
    let output = pool.reinject_alias(&alias, &key)?;
    if !successful_key(&output, &key, "reinject") {
        return Err(invalid());
    }
    let retained = cache.open_verified(&p.object).map_err(|_| invalid())?;
    let retained_identity = retained
        .with_path(|path| same_file::Handle::from_path(path))
        .map_err(|_| invalid())??;
    let published = hold_copy(&pool, &key, p.maximum_bytes)?.ok_or_else(invalid)?;
    if retained_identity != identity || !published.same_file(&identity)? || alias.try_exists()? {
        return Err(invalid());
    }
    // Empty per-attempt metadata is retained for diagnostics. No legacy path,
    // payload object, or abandoned staging entry is deleted here.
    Ok(report(p, Status::Adopted))
}

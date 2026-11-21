//! Caller-owned file ingress through native annex staging/publication only.
use super::{
    VerifiedCopy, adoption, hold_copy_with_guards, info, invalid, managed_guards, successful_key,
    uuid,
};
use crate::annex::{
    identity::Key,
    legacy::hold_link_directories,
    process::{Enrollment, Runner},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};
use tig_core::{Digest, LogicalPath, Object};
use tig_store::source_claims::protection::effects::HeldObject;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub pool: Enrollment,
    pub store: Digest,
    pub source_root: PathBuf,
    pub source: LogicalPath,
    pub source_owned: bool,
    #[serde(default)]
    pub expected: Option<Object>,
    /// Private transient provider locator for an explicitly admitted guarded reader.
    #[serde(default)]
    pub include_private_location: bool,
    pub maximum_bytes: u64,
    pub minimum_free_bytes: u64,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub status: &'static str,
    pub store: Digest,
    pub object: Object,
    pub source_opened: bool,
    pub staged_bytes: u64,
    pub reopened_verified: bool,
    pub source_consumed: bool,
    pub source_may_be_retired: bool,
    pub custody: bool,
    pub removal_authorized: bool,
}
pub struct Published {
    report: Report,
    _copy: VerifiedCopy,
    _admission: Vec<HeldObject>,
    _source: Option<Source>,
}
impl Published {
    pub fn report(&self) -> &Report {
        &self.report
    }
}
struct Source {
    file: File,
    bytes: u64,
    _parents: Vec<File>,
}
fn validate(p: &Profile) -> io::Result<()> {
    if !cfg!(windows)
        || p.schema != "tig.annex-file-publication.experimental-1"
        || !p.source_owned
        || !p.source_root.is_absolute()
        || p.source_root.to_str().is_none_or(|s| s.len() > 4096)
        || p.source_root.components().count() > 128
        || p.source_root
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        || p.maximum_bytes == 0
        || p.maximum_bytes > 64 * 1024 * 1024 * 1024
        || p.minimum_free_bytes < 100 * 1024 * 1024
        || p.expected
            .as_ref()
            .is_some_and(|o| o.bytes > p.maximum_bytes)
    {
        return Err(invalid());
    }
    Ok(())
}
fn source(p: &Profile, pool: &Runner) -> io::Result<Source> {
    let mut parents = hold_link_directories(&p.source_root)?;
    let root = fs::canonicalize(&p.source_root)?;
    if root.starts_with(fs::canonicalize(pool.repository())?) {
        return Err(invalid());
    }
    let path = root.join(p.source.as_str());
    for parent in path.parent().ok_or_else(invalid)?.ancestors() {
        if parent.join("store.json").try_exists()? {
            return Err(invalid());
        }
        if parent == root {
            break;
        }
    }
    parents.extend(hold_link_directories(path.parent().ok_or_else(invalid)?)?);
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > p.maximum_bytes
    {
        return Err(invalid());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        options.share_mode(1).custom_flags(0x00200000);
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
    }
    let file = options.open(&path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() != metadata.len() {
        return Err(invalid());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if opened.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
    }
    Ok(Source {
        file,
        bytes: metadata.len(),
        _parents: parents,
    })
}
fn transfer(
    source: &mut File,
    bytes: u64,
    mut destination: Option<&mut File>,
) -> io::Result<Object> {
    source.rewind()?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(30) {
            return Err(invalid());
        }
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total = total.checked_add(n as u64).ok_or_else(invalid)?;
        if total > bytes {
            return Err(invalid());
        }
        hash.update(&buffer[..n]);
        if let Some(out) = destination.as_mut() {
            out.write_all(&buffer[..n])?;
        }
    }
    if total != bytes {
        return Err(invalid());
    }
    Ok(Object {
        sha256: Digest::parse(
            &hash
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        )
        .map_err(|_| invalid())?,
        bytes,
    })
}
fn existing(pool: &Runner, key: &Key, maximum: u64) -> io::Result<Option<VerifiedCopy>> {
    hold_copy_with_guards(pool, key, maximum, hold_link_directories)
}
fn result(
    p: &Profile,
    object: Object,
    copy: VerifiedCopy,
    admission: Vec<HeldObject>,
    source: Option<Source>,
    staged: Option<u64>,
) -> Published {
    Published {
        report: Report {
            schema: "tig.annex-file-publication-report.experimental-1",
            status: if staged.is_some() {
                "published"
            } else {
                "already-present"
            },
            store: p.store.clone(),
            object,
            source_opened: source.is_some(),
            staged_bytes: staged.unwrap_or(0),
            reopened_verified: true,
            source_consumed: false,
            source_may_be_retired: false,
            custody: false,
            removal_authorized: false,
        },
        _copy: copy,
        _admission: admission,
        _source: source,
    }
}
fn publish_new(
    p: &Profile,
    pool: &Runner,
    source: &mut Source,
    object: &Object,
    key: &Key,
) -> io::Result<()> {
    if fs2::available_space(pool.repository())?
        .checked_sub(object.bytes)
        .is_none_or(|n| n < p.minimum_free_bytes)
    {
        return Err(invalid());
    }
    let git = if pool.repository().join(".git").is_dir() {
        pool.repository().join(".git")
    } else {
        pool.repository().to_owned()
    };
    let annex = git.join("annex");
    let _annex = hold_link_directories(&annex)?;
    let tmp = annex.join("tmp");
    match fs::create_dir(&tmp) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let _tmp = hold_link_directories(&tmp)?;
    let attempt = adoption::stage(&tmp)?;
    let _attempt = hold_link_directories(&attempt)?;
    let alias = attempt.join(key.as_str());
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&alias)?;
    if transfer(&mut source.file, source.bytes, Some(&mut output))? != *object {
        return Err(invalid());
    }
    output.sync_all()?;
    drop(output);
    let output = pool.reinject_alias(&alias, key)?;
    if !successful_key(&output, key, "reinject") || alias.try_exists()? {
        return Err(invalid());
    }
    Ok(())
}
/// Positive verified ingress only. The caller's source remains its responsibility.
pub fn publish(p: &Profile) -> io::Result<Published> {
    validate(p)?;
    let pool = Runner::new(p.pool.clone())?;
    if uuid(&info(&pool, None)?)?.store() != p.store {
        return Err(invalid());
    }
    if let Some(object) = &p.expected {
        let key = Key::from_object(object);
        let admission = managed_guards(&[&pool], &key)?;
        if let Some(copy) = existing(&pool, &key, p.maximum_bytes)? {
            return Ok(result(p, object.clone(), copy, admission, None, None));
        }
    }
    let mut source = source(p, &pool)?;
    let object = transfer(&mut source.file, source.bytes, None)?;
    if p.expected
        .as_ref()
        .is_some_and(|expected| expected != &object)
    {
        return Err(invalid());
    }
    let key = Key::from_object(&object);
    let admission = managed_guards(&[&pool], &key)?;
    if let Some(copy) = existing(&pool, &key, p.maximum_bytes)? {
        return Ok(result(p, object, copy, admission, Some(source), None));
    }
    publish_new(p, &pool, &mut source, &object, &key)?;
    let reopened = Runner::new(p.pool.clone())?;
    if uuid(&info(&reopened, None)?)?.store() != p.store {
        return Err(invalid());
    }
    let copy = existing(&reopened, &key, p.maximum_bytes)?.ok_or_else(invalid)?;
    let bytes = object.bytes;
    Ok(result(
        p,
        object,
        copy,
        admission,
        Some(source),
        Some(bytes),
    ))
}
/// Persist one bounded, no-clobber observation while its source/object guards live.
pub fn publish_file(p: &Profile, profile: Object, output: &Path) -> io::Result<serde_json::Value> {
    validate(p)?;
    if !output.is_absolute()
        || output
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(invalid());
    }
    match fs::symlink_metadata(output) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        _ => return Err(invalid()),
    }
    let parent = fs::canonicalize(output.parent().ok_or_else(invalid)?)?;
    let _parents = hold_link_directories(&parent)?;
    let output = parent.join(output.file_name().ok_or_else(invalid)?);
    for root in [
        Some(&p.pool.repository),
        Some(&p.source_root),
        Some(&p.pool.empty_hooks),
        p.pool.control.as_ref().map(|c| &c.root),
    ]
    .into_iter()
    .flatten()
    {
        if parent.starts_with(root)
            || fs::canonicalize(root).is_ok_and(|known| parent.starts_with(known))
        {
            return Err(invalid());
        }
    }
    let published = publish(p)?;
    let mut value = serde_json::to_value(published.report()).map_err(|_| invalid())?;
    value["profile"] = serde_json::to_value(profile).map_err(|_| invalid())?;
    if p.include_private_location {
        value["private_location"] =
            serde_json::to_value(super::reader::provider_location(&published._copy)?)
                .map_err(|_| invalid())?;
    }
    let bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
    if bytes.len() > 65_536 {
        return Err(invalid());
    }
    let attempt = adoption::stage(&parent)?;
    let _attempt = hold_link_directories(&attempt)?;
    let temporary = attempt.join("receipt.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::hard_link(&temporary, &output)?;
    Ok(value)
}

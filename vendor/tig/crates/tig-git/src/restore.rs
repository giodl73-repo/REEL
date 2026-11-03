//! Shared explicit verified copy; no cache initialization or retention authority.
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_core::Object;
use tig_store::existing::{ByteSource, Protection, ReadCache};

pub type Result<T> = std::result::Result<T, String>;

pub fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path).map_err(|_| "destination metadata unavailable")?;
    if m.file_type().is_symlink() {
        return Err("destination link refused".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err("destination reparse point refused".into());
        }
    }
    Ok(m)
}

fn verify(mut input: impl Read, object: &Object, mut out: Option<&mut File>) -> Result<()> {
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 65536];
    loop {
        let n = input.read(&mut buffer).map_err(|_| "copy read failed")?;
        if n == 0 {
            break;
        }
        size = size.checked_add(n as u64).ok_or("copy size overflow")?;
        if size > object.bytes {
            return Err("copy exceeds pinned size".into());
        }
        hash.update(&buffer[..n]);
        if let Some(file) = out.as_mut() {
            file.write_all(&buffer[..n])
                .map_err(|_| "copy write failed")?;
        }
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if size != object.bytes || digest != object.sha256.as_str() {
        return Err("copy integrity mismatch".into());
    }
    Ok(())
}

/// Missing parents are allowed only for an explicit batch preflight.
pub fn check_destination(dest: &Path, object: &Object, missing_parents: bool) -> Result<bool> {
    if !dest.is_absolute()
        || dest
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("destination must be absolute without parent traversal".into());
    }
    for parent in dest
        .parent()
        .ok_or("destination parent required")?
        .ancestors()
    {
        match fs::symlink_metadata(parent) {
            Err(e) if missing_parents && e.kind() == std::io::ErrorKind::NotFound => {}
            _ => {
                if !ordinary(parent)?.is_dir() {
                    return Err("destination parent is not a directory".into());
                }
            }
        }
    }
    match fs::symlink_metadata(dest) {
        Ok(_) => {
            let m = ordinary(dest)?;
            if !m.is_file() || m.len() != object.bytes {
                return Err("existing destination differs".into());
            }
            verify(
                File::open(dest).map_err(|_| "cannot read destination")?,
                object,
                None,
            )?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("cannot inspect destination".into()),
    }
}

/// Trusted caller-owned namespace, not a sandbox against hostile directory swaps.
pub fn create_parents(dest: &Path) -> Result<()> {
    if !dest.is_absolute()
        || dest
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("destination must be absolute without parent traversal".into());
    }
    let mut parents: Vec<_> = dest
        .parent()
        .ok_or("destination parent required")?
        .ancestors()
        .collect();
    parents.reverse();
    for parent in parents {
        match fs::symlink_metadata(parent) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(parent) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err("cannot create owned destination parent".into()),
            },
            Err(_) => return Err("cannot inspect owned destination parent".into()),
        }
        if !ordinary(parent)?.is_dir() {
            return Err("destination parent is not a directory".into());
        }
    }
    Ok(())
}

struct Partial(PathBuf);
impl Partial {
    fn create(path: PathBuf) -> Result<(Self, File)> {
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "cannot create private copy")?;
        // A proposed filename grants no ownership. Arm cleanup only after creation.
        Ok((Self(path), output))
    }
}
impl Drop for Partial {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Outcome {
    pub action: &'static str,
    pub source_protection_at_copy: Protection,
    pub destination_verified: bool,
    pub copied_bytes: u64,
}

/// Reopens and verifies a source guard for this operation, keeping it through copy.
pub fn restore(
    cache: &mut ReadCache,
    object: &Object,
    dest: &Path,
    apply: bool,
) -> Result<Outcome> {
    let mut source = cache.open_verified(object).map_err(|e| e.to_string())?;
    let protection = source.protection();
    let exists = check_destination(dest, object, false)?;
    if apply && !exists {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| "cannot allocate private copy name")?;
        let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let (partial, mut output) = Partial::create(
            dest.parent()
                .ok_or("destination parent required")?
                .join(format!(".tig-restore-{suffix}.partial")),
        )?;
        verify(&mut source, object, Some(&mut output))?;
        output.sync_all().map_err(|_| "cannot sync private copy")?;
        drop(output);
        verify(
            File::open(&partial.0).map_err(|_| "cannot read private copy")?,
            object,
            None,
        )?;
        fs::hard_link(&partial.0, dest).map_err(|_| "cannot publish without overwrite")?;
    }
    Ok(Outcome {
        action: if exists {
            "already-present"
        } else if apply {
            "restored"
        } else {
            "would-restore"
        },
        source_protection_at_copy: protection,
        destination_verified: exists || apply,
        copied_bytes: if apply && !exists { object.bytes } else { 0 },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_creation_refusal_preserves_foreign_private_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root
            .path()
            .join(".tig-restore-00000000000000000000000000000000.partial");
        fs::write(&path, b"foreign acknowledged bytes").unwrap();
        assert!(Partial::create(path.clone()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"foreign acknowledged bytes");
    }

    #[test]
    fn failed_copy_removes_only_its_newly_created_private_file() {
        fn copy(path: PathBuf) -> Result<()> {
            let (_partial, mut output) = Partial::create(path)?;
            verify(
                &b"incomplete"[..],
                &Object::from_bytes(b"complete source"),
                Some(&mut output),
            )
        }
        let root = tempfile::tempdir().unwrap();
        let path = root
            .path()
            .join(".tig-restore-11111111111111111111111111111111.partial");
        let other = root.path().join("other-file");
        fs::write(&other, b"keep me").unwrap();
        assert!(copy(path.clone()).is_err());
        assert!(!path.exists());
        assert_eq!(fs::read(other).unwrap(), b"keep me");
    }
}

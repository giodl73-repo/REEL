//! Explicit native-control routing for the cel editor. No discovery or media I/O.
use crate::{Result, tiering};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

pub const FILES: &[&str] = &[
    "scene.json",
    "scene-bindings.json",
    "alignment.json",
    "selected-graph.json",
    "selected-pointer.json",
    "profile.json",
    "catalog.json",
    "episode.json",
    "policy.json",
    "season-bindings.json",
    "episode-bindings.json",
    "alignment-paths.json",
    "compile.json",
];
const SCHEMA: &str = "reel.cel-sorter-project.v1";
const PROFILE: &str = "cel-sorter-project.json";
const REQUEST: &str = "edit-request.json";
const CONTROL_BYTES: u64 = 2 * 1024 * 1024;
fn control_budget(role: &str) -> u64 {
    if matches!(role, "scene.json" | "scene-bindings.json") {
        super::CONTROL_BYTES as u64
    } else {
        CONTROL_BYTES
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    schema: String,
    root: PathBuf,
    #[serde(deserialize_with = "unique_files")]
    files: BTreeMap<String, String>,
}
fn unique_files<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct Files;
    impl<'de> serde::de::Visitor<'de> for Files {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("unique native control roles")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut input: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut files = BTreeMap::new();
            while let Some((role, path)) = input.next_entry::<String, String>()? {
                if files.insert(role, path).is_some() {
                    return Err(serde::de::Error::custom("duplicate native control role"));
                }
            }
            Ok(files)
        }
    }
    decoder.deserialize_map(Files)
}

/// Private local configuration. A content lock is not custody or publication authority.
pub struct Project {
    root: PathBuf,
    files: BTreeMap<String, String>,
    descriptor_sha256: Option<String>,
}
pub struct Snapshot {
    pub controls: BTreeMap<String, Value>,
    pub hashes: Value,
    pub bytes: BTreeMap<String, u64>,
}

fn plain(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if reparse || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err("Native project controls require regular non-reparse paths".into());
    }
    Ok(())
}
fn root(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    #[cfg(windows)]
    for component in path.components() {
        if let Component::Prefix(prefix) = component
            && !matches!(
                prefix.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            )
        {
            return Err("Native project root must be an explicit local directory".into());
        }
    }
    for ancestor in path.ancestors() {
        plain(ancestor, true)?;
    }
    Ok(path.canonicalize()?)
}
fn relative(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 512
        || path.contains(['\\', ':'])
        || path
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '<' | '>' | '"' | '|' | '?' | '*'))
        || path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' '])
        })
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Native control path must be a portable project-relative path".into());
    }
    for part in path.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err("Native control path uses a reserved device name".into());
        }
    }
    let first = path.split('/').next().unwrap_or("");
    if first.eq_ignore_ascii_case(PROFILE) || first.eq_ignore_ascii_case(REQUEST) {
        return Err("Native control path overlaps draft metadata".into());
    }
    Ok(())
}
impl Project {
    /// A directory selects the legacy thirteen-file layout. A regular JSON file
    /// explicitly selects a versioned map; its relative root is manifest-relative.
    pub fn open(input: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(input)?;
        if metadata.is_dir() {
            return Ok(Self {
                root: root(input)?,
                files: FILES
                    .iter()
                    .map(|name| ((*name).into(), (*name).into()))
                    .collect(),
                descriptor_sha256: None,
            });
        }
        plain(input, false)?;
        let parent = root(input.parent().unwrap_or(Path::new(".")))?;
        let bytes = tiering::read_bounded(
            &parent.join(input.file_name().ok_or("Missing project descriptor name")?),
            64 * 1024,
        )?;
        let descriptor: Descriptor = serde_json::from_slice(&bytes)?;
        if descriptor.schema != SCHEMA
            || descriptor.root.as_os_str().is_empty()
            || descriptor.files.len() > FILES.len()
            || FILES
                .iter()
                .take(6)
                .any(|role| !descriptor.files.contains_key(*role))
            || descriptor
                .files
                .keys()
                .any(|role| !FILES.contains(&role.as_str()))
        {
            return Err("Unsupported or incomplete native project descriptor".into());
        }
        let mut paths = BTreeSet::new();
        for path in descriptor.files.values() {
            relative(path)?;
            if !paths.insert(path.to_lowercase()) {
                return Err("Aliased native control paths".into());
            }
        }
        let declared_root = if descriptor.root.is_absolute() {
            descriptor.root
        } else {
            parent.join(descriptor.root)
        };
        Ok(Self {
            root: root(&declared_root)?,
            files: descriptor.files,
            descriptor_sha256: Some(tiering::digest(&bytes)),
        })
    }
    pub fn descriptor_sha256(&self) -> Option<&str> {
        self.descriptor_sha256.as_deref()
    }
    pub fn accepts_edit(&self, request: &Value) -> bool {
        match &self.descriptor_sha256 {
            Some(hash) => {
                request["schema"] == "reel.cel-sorter-edit.v2"
                    && request["expected_project"] == *hash
            }
            None => {
                request["schema"] == "reel.cel-sorter-edit.v1"
                    && request.get("expected_project").is_none()
            }
        }
    }
    /// Read only declared controls, once each, with exact byte locks. Native
    /// semantic validation remains with the caller; this method resolves no media.
    pub fn load(&self) -> Result<Snapshot> {
        let mut controls = BTreeMap::new();
        let mut hashes = serde_json::Map::new();
        let mut sizes = BTreeMap::new();
        for (role, relative_path) in &self.files {
            let mut path = self.root.clone();
            let parts: Vec<_> = Path::new(relative_path).components().collect();
            for (i, part) in parts.iter().enumerate() {
                path.push(part.as_os_str());
                plain(&path, i + 1 < parts.len())?;
            }
            let bytes = tiering::read_bounded(&path, control_budget(role))?;
            hashes.insert(role.clone(), Value::String(tiering::digest(&bytes)));
            sizes.insert(role.clone(), bytes.len() as u64);
            controls.insert(role.clone(), serde_json::from_slice(&bytes)?);
        }
        Ok(Snapshot {
            controls,
            hashes: Value::Object(hashes),
            bytes: sizes,
        })
    }
    /// Create an entirely new candidate directory. Mapping and relative layout
    /// survive; embedded native references are never rewritten or discovered.
    /// Failed I/O may leave an incomplete candidate for explicit reconciliation.
    pub fn write_draft(
        &self,
        out: &Path,
        controls: &BTreeMap<String, Value>,
        request: &Value,
    ) -> Result<()> {
        if controls.len() != self.files.len()
            || controls.keys().any(|role| !self.files.contains_key(role))
        {
            return Err("Draft controls differ from explicit project scope".into());
        }
        let mut encoded = Vec::new();
        for (role, control) in controls {
            let bytes = serde_json::to_vec(control)?;
            let max = control_budget(role);
            if bytes.len() as u64 > max {
                return Err("Draft control exceeds intake byte budget".into());
            }
            encoded.push((self.files[role].clone(), bytes));
        }
        let request = serde_json::to_vec(request)?;
        if request.len() as u64 > CONTROL_BYTES {
            return Err("Draft request exceeds byte budget".into());
        }
        encoded.push((REQUEST.into(), request));
        if self.descriptor_sha256.is_some() {
            encoded.push((
                PROFILE.into(),
                serde_json::to_vec(&Descriptor {
                    schema: SCHEMA.into(),
                    root: ".".into(),
                    files: self.files.clone(),
                })?,
            ));
        }
        let parent = out.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let out = root(parent)?.join(out.file_name().ok_or("Invalid candidate directory name")?);
        fs::create_dir(&out)?;
        for (name, bytes) in encoded {
            let path = out.join(&name);
            let mut directory = out.clone();
            for part in Path::new(&name)
                .parent()
                .unwrap_or(Path::new(""))
                .components()
            {
                directory.push(part.as_os_str());
                match fs::create_dir(&directory) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                    Err(e) => return Err(e.into()),
                }
                plain(&directory, true)?;
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        Ok(())
    }
}

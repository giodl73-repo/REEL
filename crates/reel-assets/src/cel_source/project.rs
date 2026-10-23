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
const SCOPED_SCHEMA: &str = "reel.cel-sorter-project.v3";
const MULTI_SCHEMA: &str = "reel.cel-sorter-project.v2";
pub const MAX_CUES: usize = 128;
const TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const PROFILE: &str = "cel-sorter-project.json";
const REQUEST: &str = "edit-request.json";
const CONTROL_BYTES: u64 = 2 * 1024 * 1024;
fn control_budget(role: &str) -> u64 {
    if role.starts_with("alignment-paths:") {
        128 * 1024
    } else if matches!(role, "scene.json" | "scene-bindings.json") {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_files",
        skip_serializing_if = "Option::is_none"
    )]
    alignment_paths: Option<BTreeMap<String, String>>,
    #[serde(deserialize_with = "unique_files")]
    files: BTreeMap<String, String>,
}
fn optional_files<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<BTreeMap<String, String>>, D::Error> {
    Ok(Option::<NativePaths>::deserialize(decoder)?.map(|NativePaths(paths)| paths))
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
    declared_files: BTreeMap<String, String>,
    captured: BTreeMap<String, Vec<u8>>,
    multi_cue: bool,
    language: Option<String>,
    declared_alignment_paths: Option<BTreeMap<String, String>>,
}
pub fn alignment_paths_role(language: &str) -> String {
    format!("alignment-paths:{language}")
}
pub fn scoped_alignment_role(language: &str, cue_id: &str) -> String {
    format!("alignment:{}", serde_json::json!([language, cue_id]))
}
pub fn alignment_role(cue_id: &str) -> String {
    format!("alignment:{cue_id}")
}
#[derive(Deserialize)]
#[serde(transparent)]
struct NativePaths(#[serde(deserialize_with = "unique_files")] BTreeMap<String, String>);
pub struct Snapshot {
    pub controls: BTreeMap<String, Value>,
    pub hashes: Value,
    pub bytes: BTreeMap<String, u64>,
    pub raw: BTreeMap<String, Vec<u8>>,
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
                declared_files: FILES
                    .iter()
                    .map(|name| ((*name).into(), (*name).into()))
                    .collect(),
                captured: BTreeMap::new(),
                multi_cue: false,
                language: None,
                declared_alignment_paths: None,
            });
        }
        plain(input, false)?;
        let parent = root(input.parent().unwrap_or(Path::new(".")))?;
        let bytes = tiering::read_bounded(
            &parent.join(input.file_name().ok_or("Missing project descriptor name")?),
            64 * 1024,
        )?;
        let descriptor: Descriptor = serde_json::from_slice(&bytes)?;
        let scoped = descriptor.schema == SCOPED_SCHEMA;
        if scoped != descriptor.alignment_paths.is_some()
            || scoped != descriptor.language.is_some()
            || descriptor
                .language
                .as_ref()
                .is_some_and(|language| language.trim().is_empty() || language.len() > 64)
        {
            return Err("Explicit language is required only for scoped project v3".into());
        }
        let multi_cue = matches!(descriptor.schema.as_str(), MULTI_SCHEMA | SCOPED_SCHEMA);
        let required = if scoped {
            vec![
                "scene.json",
                "scene-bindings.json",
                "selected-graph.json",
                "selected-pointer.json",
                "profile.json",
            ]
        } else if multi_cue {
            vec![
                "scene.json",
                "scene-bindings.json",
                "selected-graph.json",
                "selected-pointer.json",
                "profile.json",
                "alignment-paths.json",
            ]
        } else {
            FILES.iter().take(6).copied().collect()
        };
        if !matches!(
            descriptor.schema.as_str(),
            SCHEMA | MULTI_SCHEMA | SCOPED_SCHEMA
        ) || descriptor.root.as_os_str().is_empty()
            || descriptor.files.len() > FILES.len()
            || required
                .iter()
                .any(|role| !descriptor.files.contains_key(*role))
            || (multi_cue && descriptor.files.contains_key("alignment.json"))
            || (scoped && descriptor.files.contains_key("alignment-paths.json"))
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
        let mut project = Self {
            root: root(&declared_root)?,
            declared_files: descriptor.files.clone(),
            files: descriptor.files,
            descriptor_sha256: Some(tiering::digest(&bytes)),
            captured: BTreeMap::new(),
            multi_cue,
            language: descriptor.language,
            declared_alignment_paths: descriptor.alignment_paths,
        };
        if multi_cue {
            let indexes: Vec<(Option<String>, String, String)> =
                if let Some(indexes) = &project.declared_alignment_paths {
                    if indexes.is_empty()
                        || indexes.len() > 16
                        || !indexes.contains_key(project.language().ok_or("language")?)
                    {
                        return Err("Scoped project needs bounded explicit language indexes".into());
                    }
                    let mut entries = Vec::new();
                    for (language, path) in indexes {
                        if language.trim().is_empty() || language.len() > 64 {
                            return Err("Invalid bounded language index identity".into());
                        }
                        relative(path)?;
                        if !paths.insert(path.to_lowercase()) {
                            return Err("Aliased language index path".into());
                        }
                        let role = alignment_paths_role(language);
                        project.files.insert(role.clone(), path.clone());
                        entries.push((Some(language.clone()), role, path.clone()));
                    }
                    entries
                } else {
                    vec![(
                        None,
                        "alignment-paths.json".into(),
                        project.files["alignment-paths.json"].clone(),
                    )]
                };
            let mut total_cues = 0;
            for (language, index_role, manifest) in indexes {
                let bytes = project.read_path(&manifest, 128 * 1024)?;
                let NativePaths(cues) = serde_json::from_slice(&bytes)?;
                total_cues += cues.len();
                if cues.is_empty() || total_cues > MAX_CUES {
                    return Err("Native cue path count exceeds budget".into());
                }
                let base = manifest
                    .rsplit_once('/')
                    .map(|(base, _)| base)
                    .unwrap_or("");
                for (cue, path) in cues {
                    if cue.trim().is_empty() || cue.len() > 256 {
                        return Err("Invalid bounded native cue identity".into());
                    }
                    let path = if base.is_empty() {
                        path
                    } else {
                        format!("{base}/{path}")
                    };
                    relative(&path)?;
                    if !paths.insert(path.to_lowercase()) {
                        return Err("Aliased native cue control path".into());
                    }
                    let role = language
                        .as_ref()
                        .map(|language| scoped_alignment_role(language, &cue))
                        .unwrap_or_else(|| alignment_role(&cue));
                    project.files.insert(role, path);
                }
                project.captured.insert(index_role, bytes);
            }
        }
        Ok(project)
    }
    pub fn descriptor_sha256(&self) -> Option<&str> {
        self.descriptor_sha256.as_deref()
    }
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
    pub fn is_multi_cue(&self) -> bool {
        self.multi_cue
    }
    pub fn session_schema(&self) -> &'static str {
        if self.language.is_some() {
            "reel.cel-sorter-session.v4"
        } else if self.multi_cue {
            "reel.cel-sorter-session.v3"
        } else if self.descriptor_sha256.is_some() {
            "reel.cel-sorter-session.v2"
        } else {
            "reel.cel-sorter-session.v1"
        }
    }
    pub fn accepts_edit(&self, request: &Value) -> bool {
        match &self.descriptor_sha256 {
            Some(hash) => {
                request["schema"]
                    == if self.language.is_some() {
                        "reel.cel-sorter-edit.v4"
                    } else if self.multi_cue {
                        "reel.cel-sorter-edit.v3"
                    } else {
                        "reel.cel-sorter-edit.v2"
                    }
                    && request["expected_project"] == *hash
                    && match &self.language {
                        Some(language) => request["language"] == *language,
                        None => request.get("language").is_none(),
                    }
            }
            None => {
                request["schema"] == "reel.cel-sorter-edit.v1"
                    && request.get("expected_project").is_none()
            }
        }
    }
    fn read_path(&self, relative_path: &str, max: u64) -> Result<Vec<u8>> {
        let mut path = self.root.clone();
        let parts: Vec<_> = Path::new(relative_path).components().collect();
        for (index, part) in parts.iter().enumerate() {
            path.push(part.as_os_str());
            plain(&path, index + 1 < parts.len())?;
        }
        tiering::read_bounded(&path, max)
    }
    /// Read only declared controls, once each, with exact byte locks. Native
    /// semantic validation remains with the caller; this method resolves no media.
    pub fn load(&self) -> Result<Snapshot> {
        let mut controls = BTreeMap::new();
        let mut hashes = serde_json::Map::new();
        let mut sizes = BTreeMap::new();
        let mut raw = BTreeMap::new();
        let mut total = 0;
        for (role, relative_path) in &self.files {
            let max = control_budget(role).min(TOTAL_BYTES - total);
            let bytes = if let Some(bytes) = self.captured.get(role) {
                if bytes.len() as u64 > max {
                    return Err("Native project total byte budget exceeded".into());
                }
                bytes.clone()
            } else {
                self.read_path(relative_path, max)?
            };
            total += bytes.len() as u64;
            hashes.insert(role.clone(), Value::String(tiering::digest(&bytes)));
            sizes.insert(role.clone(), bytes.len() as u64);
            controls.insert(role.clone(), serde_json::from_slice(&bytes)?);
            raw.insert(role.clone(), bytes);
        }
        Ok(Snapshot {
            controls,
            hashes: Value::Object(hashes),
            bytes: sizes,
            raw,
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
        self.write_candidate(out, controls, request, None)
    }
    /// Preserve exact captured bytes for unchanged controls, including protected
    /// alignments whose selected hashes bind formatting. No source reopening.
    pub fn write_draft_from_snapshot(
        &self,
        out: &Path,
        controls: &BTreeMap<String, Value>,
        request: &Value,
        snapshot: &Snapshot,
    ) -> Result<()> {
        self.write_candidate(out, controls, request, Some(snapshot))
    }
    fn write_candidate(
        &self,
        out: &Path,
        controls: &BTreeMap<String, Value>,
        request: &Value,
        snapshot: Option<&Snapshot>,
    ) -> Result<()> {
        if controls.len() != self.files.len()
            || controls.keys().any(|role| !self.files.contains_key(role))
        {
            return Err("Draft controls differ from explicit project scope".into());
        }
        let mut encoded = Vec::new();
        let mut total = 0u64;
        for (role, control) in controls {
            let bytes = if let Some(snapshot) = snapshot {
                let raw = snapshot
                    .raw
                    .get(role)
                    .ok_or("Missing captured native control")?;
                if snapshot.hashes[role] != tiering::digest(raw)
                    || snapshot.bytes.get(role) != Some(&(raw.len() as u64))
                {
                    return Err("Captured native bytes differ from source locks".into());
                }
                let original: Value = serde_json::from_slice(raw)?;
                if original == *control {
                    raw.clone()
                } else {
                    serde_json::to_vec(control)?
                }
            } else {
                serde_json::to_vec(control)?
            };
            let max = if self.multi_cue && role == "alignment-paths.json" {
                128 * 1024
            } else {
                control_budget(role)
            };
            if bytes.len() as u64 > max {
                return Err("Draft control exceeds intake byte budget".into());
            }
            total = total
                .checked_add(bytes.len() as u64)
                .ok_or("Native draft size overflow")?;
            if total > TOTAL_BYTES {
                return Err("Native draft total byte budget exceeded".into());
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
                    schema: if self.language.is_some() {
                        SCOPED_SCHEMA
                    } else if self.multi_cue {
                        MULTI_SCHEMA
                    } else {
                        SCHEMA
                    }
                    .into(),
                    language: self.language.clone(),
                    alignment_paths: self.declared_alignment_paths.clone(),
                    root: ".".into(),
                    files: self.declared_files.clone(),
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

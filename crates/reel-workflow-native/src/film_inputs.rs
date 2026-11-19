//! Borrowed exact native files for REEL's existing renderer. No producer dispatch,
//! output custody, workspace/capacity admission or task completion is granted.
use crate::{NativeInputs, Tool, error, hold_tool, io_error};
use reel::scene_delivery::{FileRef, InputFiles, MediaProgram};
use reel_workflow::{ErrorKind, Packet, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::{Path, PathBuf},
};
use tig_git::annex::{
    identity::Key,
    process::{Command, Runner},
};
use tig_store::existing::{Limits, ProviderPath, ReadCache, RequiredProtection, VerifiedRead};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Programs {
    pub ffmpeg: Tool,
    pub ffprobe: Tool,
}
/// An operation's caller must freeze this program declaration with its request
/// or invocation. Holding executable bytes here does not authenticate execution
/// or supply retained ownership of the installed runtime.
impl Programs {
    pub fn fingerprint(&self) -> Result<String> {
        reel_workflow::canonical_digest(self)
    }
}
pub struct Inputs {
    _native: NativeInputs,
    _cache: ReadCache,
    files: BTreeMap<String, (tig_core::Object, usize)>,
    held: Vec<VerifiedRead>,
    programs: Programs,
    _tools: Vec<File>,
    _tool_directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
}
fn namespace(name: &str) -> bool {
    !name.is_empty() && name.len() <= 512 && !name.chars().any(char::is_control)
}
fn path_error(e: tig_store::existing::Error) -> reel_workflow::Error {
    use tig_store::existing::Error as E;
    error(
        match e {
            E::Invalid => ErrorKind::Invalid,
            E::Budget => ErrorKind::Budget,
            E::Missing => ErrorKind::Missing,
            E::Integrity => ErrorKind::Integrity,
            E::Denied => ErrorKind::Denied,
            E::Busy => ErrorKind::Busy,
            E::Io => ErrorKind::NeedsRecovery,
            E::Unsupported => ErrorKind::Unsupported,
        },
        "film-native-path-provider",
    )
}
impl Inputs {
    /// Exact aliases -> declared logical asset IDs. All declared task inputs
    /// must be covered; no current/latest, filesystem or staging-byte fallback.
    /// Metadata parsing and renderer-specific reference closure remain with the
    /// existing renderer. Only a live admitted or retained input authority may
    /// construct this bridge.
    pub fn prepare(
        packet: &Packet,
        mut native: NativeInputs,
        aliases: &BTreeMap<String, String>,
        programs: Programs,
    ) -> Result<Self> {
        reel_workflow::check_packet(packet)?;
        if native.snapshot.packet != *packet
            || (native._claims.is_none() && native._permit.is_none())
        {
            return Err(error(
                ErrorKind::Denied,
                "film-input-live-authority-required",
            ));
        }
        let declared: BTreeSet<_> = packet
            .task
            .inputs
            .iter()
            .map(|a| a.logical_id.as_str())
            .collect();
        if aliases.is_empty()
            || aliases.len() > 256
            || aliases.keys().any(|n| !namespace(n))
            || aliases
                .values()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                != declared
        {
            return Err(error(ErrorKind::Conflict, "film-input-exact-alias-closure"));
        }
        let total = packet
            .task
            .inputs
            .iter()
            .try_fold(0u64, |n, a| n.checked_add(a.bytes))
            .ok_or_else(|| error(ErrorKind::Budget, "film-input-byte-overflow"))?;
        if total > 512 * 1024 * 1024 || packet.task.inputs.len() > 128 {
            return Err(error(ErrorKind::Budget, "film-input-byte-bound"));
        }
        let mut directories = Vec::new();
        let mut tools = Vec::new();
        for tool in [&programs.ffmpeg, &programs.ffprobe] {
            directories.push(
                reel_assets::jobs::open_workspace_directory(
                    tool.path
                        .parent()
                        .ok_or_else(|| error(ErrorKind::Invalid, "film-program-parent"))?,
                )
                .map_err(|_| error(ErrorKind::Denied, "film-program-namespace"))?,
            );
            tools.push(hold_tool(tool)?);
        }
        let runner = Runner::new(native.source.pool.clone()).map_err(io_error)?;
        let mut cache = ReadCache::provider(
            RequiredProtection::PreventMutation,
            Limits {
                object_bytes: 512 * 1024 * 1024,
                total_bytes: 512 * 1024 * 1024,
                requests: 128,
                live_readers: 128,
                verify_millis: 30000,
            },
        )
        .map_err(path_error)?;
        let mut held = Vec::new();
        let mut by_id = BTreeMap::new();
        let mut by_object = BTreeMap::new();
        for asset in &packet.task.inputs {
            let object = native
                .snapshot
                .objects
                .get(&asset.logical_id)
                .ok_or_else(|| error(ErrorKind::Missing, "film-native-object"))?;
            if object.bytes != asset.bytes || object.sha256.as_str() != asset.sha256 {
                return Err(error(ErrorKind::Integrity, "film-native-input-pin"));
            }
            let index = if let Some(index) = by_object.get(object) {
                *index
            } else {
                // This verifies the enrolled store/key and keeps its object and
                // native namespace gates alive before resolving an executable path.
                // Metadata binding may already own this exact operation's gate.
                // captured() is populated only after a successful guarded read;
                // reopening that non-reentrant gate would conflict with itself.
                if native.captured(object).is_none() {
                    native.held.push(
                        tig_git::annex::workflow::reader::open(&native.source, object)
                            .map_err(io_error)?,
                    );
                }
                let key = Key::from_object(object);
                let location = runner
                    .run(Command::ContentLocation(&key))
                    .map_err(io_error)?;
                if !location.success {
                    return Err(error(
                        ErrorKind::NeedsRecovery,
                        "film-native-location-observation",
                    ));
                }
                if location.stdout.len() > 4096 {
                    return Err(error(ErrorKind::Budget, "film-native-location-bytes"));
                }
                let suffix = std::str::from_utf8(&location.stdout)
                    .map_err(|_| error(ErrorKind::Integrity, "film-native-location-encoding"))?
                    .trim_end_matches(['\r', '\n'])
                    .replace('\\', "/");
                let suffix = suffix.strip_prefix("./").unwrap_or(&suffix);
                let suffix = ProviderPath::parse(suffix)
                    .map_err(|_| error(ErrorKind::Integrity, "film-native-location-scope"))?;
                let file = cache
                    .open_provider_file(&native.source.pool.repository, &suffix, object)
                    .map_err(path_error)?;
                let index = held.len();
                held.push(file);
                by_object.insert(object.clone(), index);
                index
            };
            by_id.insert(asset.logical_id.as_str(), (object.clone(), index));
        }
        let files = aliases
            .iter()
            .map(|(name, id)| (name.clone(), by_id[id.as_str()].clone()))
            .collect();
        Ok(Self {
            _native: native,
            _cache: cache,
            files,
            held,
            programs,
            _tools: tools,
            _tool_directories: directories,
        })
    }
    pub fn program_fingerprint(&self) -> Result<String> {
        self.programs.fingerprint()
    }
    pub fn objects(&self) -> usize {
        self.held.len()
    }
    /// Private child-process locators. The caller must keep this provider alive
    /// until every child exits; these paths do not carry independent authority.
    #[cfg(windows)]
    pub(crate) fn process_files(&self) -> Result<BTreeMap<String, FileRef>> {
        self.files
            .iter()
            .map(|(name, (object, index))| {
                let path = self.held[*index]
                    .with_path(Path::to_path_buf)
                    .map_err(path_error)?;
                Ok((
                    name.clone(),
                    FileRef {
                        path,
                        sha256: object.sha256.as_str().into(),
                        bytes: object.bytes,
                    },
                ))
            })
            .collect()
    }
    #[cfg(windows)]
    pub(crate) fn programs(&self) -> Programs {
        self.programs.clone()
    }
    fn binding(&self, item: &FileRef) -> anyhow::Result<&VerifiedRead> {
        let name = item
            .path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("film reference encoding"))?;
        let (object, index) = self
            .files
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("film reference undeclared"))?;
        anyhow::ensure!(
            object.bytes == item.bytes && object.sha256.as_str() == item.sha256,
            "film reference pin differs"
        );
        Ok(&self.held[*index])
    }
}
impl InputFiles for Inputs {
    fn verified_path(&self, item: &FileRef) -> anyhow::Result<PathBuf> {
        Ok(self.binding(item)?.with_path(Path::to_path_buf)?)
    }
    fn validate_reference_name(&self, item: &FileRef) -> anyhow::Result<()> {
        self.binding(item)?;
        Ok(())
    }
    fn media_program(&self, program: MediaProgram) -> anyhow::Result<PathBuf> {
        Ok(match program {
            MediaProgram::Ffmpeg => &self.programs.ffmpeg.path,
            MediaProgram::Ffprobe => &self.programs.ffprobe.path,
        }
        .clone())
    }
}

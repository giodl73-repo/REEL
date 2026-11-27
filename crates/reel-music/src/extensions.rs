use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{hash::canonical_sha256, validate_sha256};

pub const SCHEMA: &str = "reel.music-extensions.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Extension {
    #[serde(rename = "muscriptor")]
    MuScriptor,
    Demucs,
    #[serde(rename = "pop2piano")]
    Pop2Piano,
    Ffmpeg,
    #[serde(rename = "musescore")]
    MuseScore,
}

impl Extension {
    pub const ALL: [Self; 5] = [
        Self::MuScriptor,
        Self::Demucs,
        Self::Pop2Piano,
        Self::Ffmpeg,
        Self::MuseScore,
    ];

    pub fn purpose(self) -> &'static str {
        match self {
            Self::MuScriptor => "timestamped parts and candidate instrument labels",
            Self::Demucs => "estimated audio stems",
            Self::Pop2Piano => "piano reductions and arrangement studies",
            Self::Ffmpeg => "decoding and exact-clock audio preparation",
            Self::MuseScore => "notation export and instrument-control rendering",
        }
    }

    fn required(self) -> &'static [&'static str] {
        match self {
            Self::MuScriptor => &["executable", "weights", "config"],
            Self::Demucs | Self::Pop2Piano => &["executable", "adapter", "weights", "config"],
            Self::Ffmpeg | Self::MuseScore => &["executable"],
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FilePin {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub version: String,
    pub files: BTreeMap<String, FilePin>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema: String,
    pub installations: BTreeMap<Extension, Installation>,
}

#[derive(Debug, Serialize)]
pub struct ExtensionStatus {
    pub extension: Extension,
    pub purpose: &'static str,
    pub required_files: &'static [&'static str],
    pub status: &'static str,
    pub detail: Option<String>,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        let registry: Self = serde_yaml::from_slice(&fs::read(path)?)?;
        if registry.schema != SCHEMA {
            bail!("unsupported music extension registry schema");
        }
        Ok(registry)
    }

    /// Return only installations for which every declared local pin verifies.
    pub fn resolve(&self, extension: Extension) -> Result<Option<&Installation>> {
        let Some(installation) = self.installations.get(&extension) else {
            return Ok(None);
        };
        crate::nonempty("extension version", &installation.version)?;
        for role in extension.required() {
            if !installation.files.contains_key(*role) {
                bail!("missing required extension file: {role}");
            }
        }
        for (role, file) in &installation.files {
            verify(file).with_context(|| format!("extension file {role} failed verification"))?;
        }
        Ok(Some(installation))
    }

    /// Resolve a registered executable for use by an offline evidence producer.
    /// The caller supplies its job arguments and verifies its output contract.
    pub fn command(&self, extension: Extension) -> Result<Command> {
        let installation = self
            .resolve(extension)?
            .context("music extension is unconfigured")?;
        Ok(offline_command(installation))
    }
    pub fn inspect(&self) -> Vec<ExtensionStatus> {
        Extension::ALL
            .into_iter()
            .map(|extension| {
                let (status, detail) = match self.resolve(extension) {
                    Ok(Some(_)) => ("verified", None),
                    Ok(None) => ("unconfigured", None),
                    Err(error) => ("invalid", Some(format!("{error:#}"))),
                };
                ExtensionStatus {
                    extension,
                    purpose: extension.purpose(),
                    required_files: extension.required(),
                    status,
                    detail,
                }
            })
            .collect()
    }
}

fn verify(pin: &FilePin) -> Result<()> {
    validate_sha256("extension file SHA-256", &pin.sha256)?;
    if !pin.path.is_absolute() {
        bail!("extension file path must be absolute");
    }
    let mut reader = BufReader::new(File::open(&pin.path)?);
    if !reader.get_ref().metadata()?.is_file() {
        bail!("extension pin must resolve to a regular file");
    }
    let mut hasher = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        bytes += count as u64;
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if bytes != pin.bytes || !digest.eq_ignore_ascii_case(&pin.sha256) {
        bail!("extension file size or hash mismatch");
    }
    Ok(())
}

fn offline_command(installation: &Installation) -> Command {
    let mut command = Command::new(&installation.files["executable"].path);
    if let Some(adapter) = installation.files.get("adapter") {
        command.arg(&adapter.path);
    }
    command
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null());
    command
}

#[derive(Debug, Serialize)]
struct MuScriptorCapture<'a> {
    schema: &'static str,
    extension_installation_sha256: String,
    extension_version: &'a str,
    input_sha256: &'a str,
    device: &'static str,
    instrument_prompt: Option<()>,
    events: Vec<serde_json::Value>,
}

/// Capture unprompted, raw MuScriptor evidence using a verified local model.
/// This is an observation input and does not qualify an instrument detector.
pub fn capture_muscriptor(registry: &Registry, source: FilePin, output: &Path) -> Result<()> {
    let installation = registry
        .resolve(Extension::MuScriptor)?
        .context("MuScriptor extension is unconfigured")?;
    verify(&source).context("input recording failed verification")?;
    let weights = &installation.files["weights"].path;
    let config = &installation.files["config"].path;
    if fs::canonicalize(weights.with_file_name("config.json"))? != fs::canonicalize(config)? {
        bail!("MuScriptor config must be config.json beside the weights");
    }
    let mut published = tempfile::NamedTempFile::new_in(
        output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    if output.exists() {
        bail!("output already exists");
    }
    let work = tempfile::tempdir()?;
    let staged = work.path().join("source.wav");
    fs::copy(&source.path, &staged)?;
    verify(&FilePin {
        path: staged.clone(),
        sha256: source.sha256.clone(),
        bytes: source.bytes,
    })?;
    let raw = work.path().join("events.json");
    let status = offline_command(installation)
        .current_dir(work.path())
        .arg("transcribe")
        .arg(&staged)
        .arg("--model")
        .arg(weights)
        .args([
            "--device",
            "cpu",
            "--batch-size",
            "1",
            "--format",
            "json",
            "-o",
        ])
        .arg(&raw)
        .env("CUDA_VISIBLE_DEVICES", "-1")
        .env("OMP_NUM_THREADS", "2")
        .env("MKL_NUM_THREADS", "2")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .context("launching verified MuScriptor executable")?;
    if !status.success() {
        bail!("MuScriptor exited with {status}");
    }
    let events = serde_json::from_reader(BufReader::new(File::open(&raw)?))?;
    let record = MuScriptorCapture {
        schema: "reel.muscriptor-capture.v1",
        extension_installation_sha256: canonical_sha256(installation)?,
        extension_version: &installation.version,
        input_sha256: &source.sha256,
        device: "cpu",
        instrument_prompt: None,
        events,
    };
    serde_json::to_writer_pretty(&mut published, &record)?;
    writeln!(published)?;
    published.persist_noclobber(output)?;
    Ok(())
}

/// Run the registered CAIMITOS-compatible Demucs adapter with the pinned
/// htdemucs checkpoint in a private local repository.
pub fn run_demucs(registry: &Registry, source: &Path, output: &Path) -> Result<()> {
    let installation = registry
        .resolve(Extension::Demucs)?
        .context("Demucs extension is unconfigured")?;
    let weights = &installation.files["weights"];
    let config = &installation.files["config"];
    let source = source.canonicalize()?;
    if !source.is_file() || output.exists() {
        bail!("Demucs requires a source file and a fresh output directory");
    }
    let repo = tempfile::tempdir()?;
    let checkpoint = weights
        .path
        .file_name()
        .context("missing checkpoint filename")?;
    for (pin, destination) in [
        (weights, repo.path().join(checkpoint)),
        (config, repo.path().join("htdemucs.yaml")),
    ] {
        fs::copy(&pin.path, &destination)?;
        verify(&FilePin {
            path: destination,
            sha256: pin.sha256.clone(),
            bytes: pin.bytes,
        })?;
    }
    let mut command = offline_command(installation);
    // The retained htdemucs checkpoint predates PyTorch 2.6's weights-only default.
    // Authorize the legacy loader only for these exact historical bytes.
    const HISTORICAL_HTDEMUCS: &str =
        "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4";
    if weights.sha256.eq_ignore_ascii_case(HISTORICAL_HTDEMUCS) {
        command.env("TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD", "1");
    } else {
        command.env_remove("TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD");
    }
    let status = command
        .args([
            "-n", "htdemucs", "--device", "cpu", "--shifts", "0", "--repo",
        ])
        .arg(repo.path())
        .arg("--out")
        .arg(output)
        .arg(source)
        .env("CUDA_VISIBLE_DEVICES", "-1")
        .env("OMP_NUM_THREADS", "2")
        .env("MKL_NUM_THREADS", "2")
        .status()
        .context("launching verified Demucs adapter")?;
    if !status.success() {
        bail!("Demucs adapter exited with {status}");
    }
    Ok(())
}

/// Launch a registered CAIMITOS-compatible private Pop2Piano request.
pub fn run_pop2piano(
    registry: &Registry,
    repo_root: &Path,
    request: &Path,
    output: &Path,
) -> Result<()> {
    let installation = registry
        .resolve(Extension::Pop2Piano)?
        .context("Pop2Piano extension is unconfigured")?;
    let snapshot = installation.files["weights"]
        .path
        .parent()
        .context("model snapshot directory is missing")?;
    if snapshot.join("config.json").canonicalize()?
        != installation.files["config"].path.canonicalize()?
    {
        bail!("Pop2Piano config must be config.json in the pinned model snapshot");
    }
    if output.exists() {
        bail!("Pop2Piano output already exists");
    }
    let status = offline_command(installation)
        .arg("--repo-root")
        .arg(repo_root)
        .arg("--request")
        .arg(request)
        .arg("--model-path")
        .arg(snapshot)
        .arg("--output-dir")
        .arg(output)
        .env("CUDA_VISIBLE_DEVICES", "-1")
        .env("OMP_NUM_THREADS", "2")
        .env("MKL_NUM_THREADS", "2")
        .status()
        .context("launching verified Pop2Piano adapter")?;
    if !status.success() {
        bail!("Pop2Piano adapter exited with {status}");
    }
    Ok(())
}

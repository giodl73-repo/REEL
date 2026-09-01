use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRef,
    hash::{canonical_sha256, sha256_path},
    nonempty,
    time::{AudioTimebase, MusicalTimebase},
    validate_authority, validate_sha256,
};

pub const SCHEMA: &str = "reel.music-source.v0.1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceManifest {
    pub schema: String,
    pub source_id: String,
    pub media: Media,
    pub musical_timebase: MusicalTimebase,
    pub authority: AuthorityRef,
    pub egress: Egress,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Media {
    pub path: PathBuf,
    pub sha256: String,
    pub format: RawPcmFormat,
    pub timebase: AudioTimebase,
    pub decoded_pcm_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RawPcmFormat {
    RawPcmU8,
    RawPcmS16le,
    RawPcmS24le,
    RawPcmS32le,
    RawPcmF32le,
}

impl RawPcmFormat {
    pub fn bytes_per_sample(self) -> u64 {
        match self {
            Self::RawPcmU8 => 1,
            Self::RawPcmS16le => 2,
            Self::RawPcmS24le => 3,
            Self::RawPcmS32le | Self::RawPcmF32le => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Egress {
    pub private: bool,
    pub network_policy: NetworkPolicy,
    pub third_party_upload: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkPolicy {
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub source_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub media_sha256: String,
    pub decoded_pcm_sha256: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub samples_per_channel: u64,
    pub bytes: u64,
    pub private: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<SourceManifest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes)
        .with_context(|| format!("music source is not valid YAML: {}", path.display()))
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    validate_loaded(path, &manifest)
}

pub fn validate_loaded(path: &Path, manifest: &SourceManifest) -> Result<ValidationReport> {
    if manifest.schema != SCHEMA {
        bail!("music source schema must be {SCHEMA}");
    }
    nonempty("source_id", &manifest.source_id)?;
    validate_sha256("media.sha256", &manifest.media.sha256)?;
    validate_sha256(
        "media.decoded_pcm_sha256",
        &manifest.media.decoded_pcm_sha256,
    )?;
    manifest.media.timebase.validate()?;
    manifest.musical_timebase.validate()?;
    validate_authority(&manifest.authority)?;
    if !manifest.egress.private {
        bail!("egress.private must be true for the foundation contract");
    }
    if manifest.egress.third_party_upload {
        bail!("egress.third_party_upload must be false");
    }
    let media_path = resolve(path, &manifest.media.path);
    let media_hash = sha256_path(&media_path)?;
    if media_hash != manifest.media.sha256.to_lowercase() {
        bail!("media sha256 does not match media.sha256");
    }
    if media_hash != manifest.media.decoded_pcm_sha256.to_lowercase() {
        bail!("raw PCM source must have identical media and decoded PCM hashes");
    }
    let bytes = fs::metadata(&media_path)?.len();
    let expected = manifest
        .media
        .timebase
        .samples_per_channel
        .checked_mul(u64::from(manifest.media.timebase.channels))
        .and_then(|value| value.checked_mul(manifest.media.format.bytes_per_sample()))
        .ok_or_else(|| anyhow::anyhow!("raw PCM byte count overflows u64"))?;
    if bytes != expected {
        bail!("raw PCM byte count {bytes} does not match declared timebase {expected}");
    }
    Ok(ValidationReport {
        schema: SCHEMA.into(),
        source_id: manifest.source_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(manifest)?,
        media_sha256: media_hash.clone(),
        decoded_pcm_sha256: media_hash,
        sample_rate_hz: manifest.media.timebase.sample_rate_hz,
        channels: manifest.media.timebase.channels,
        samples_per_channel: manifest.media.timebase.samples_per_channel,
        bytes,
        private: manifest.egress.private,
        shareable: false,
        verified: true,
    })
}

pub(crate) fn resolve(manifest_path: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        value.to_path_buf()
    } else {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(value)
    }
}

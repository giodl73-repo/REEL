//! Explicit bounded metadata worker. Never constructs/starts an audio stream.
use super::*;
#[cfg(windows)]
use crate::media_runtime::ByteIdentity;
use crate::scene_delivery::FileRef;
#[cfg(windows)]
use std::path::Path;

pub const PROBE_SCHEMA: &str = "reel.installation-endpoint-probe.v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRequest {
    pub schema: String,
    pub endpoint_id: String,
    pub runtime: FileRef,
}
impl ProbeRequest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == PROBE_SCHEMA
                && explicit_endpoint_id(&self.endpoint_id)
                && self.endpoint_id.len() <= 1024
                && !self.endpoint_id.chars().any(char::is_control),
            "installation probe request profile"
        );
        ensure!(
            self.runtime.bytes > 0
                && self.runtime.bytes <= 1024 * 1024 * 1024
                && self.runtime.sha256.len() == 64
                && self
                    .runtime
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "installation probe runtime pin"
        );
        ensure!(
            self.runtime.path.is_absolute(),
            "installation probe runtime path must be absolute"
        );
        Ok(())
    }
}
#[cfg(windows)]
fn read<T: serde::de::DeserializeOwned>(path: &Path, max: usize) -> Result<T> {
    Ok(serde_json::from_slice(&producer::bounded(path, max)?)?)
}
#[cfg(any(windows, test))]
fn verify_observation(request: &ProbeRequest, observed: &EndpointObservation) -> Result<()> {
    observed.binding.validate()?;
    ensure!(
        observed.binding.device_id == request.endpoint_id
            && observed.sample_rate == 48000
            && observed.channels == 2
            && observed.format == "f32"
            && observed.requested_buffer_frames == 480
            && !observed.effective_buffer_checked
            && !observed.device_playback_checked,
        "installation probe observation differs from requested metadata scope"
    );
    Ok(())
}

/// Explicit metadata execution, not validation. Holds the exact current worker
/// image and owns its child tree; no cache, default endpoint or playback.
pub fn probe(
    request: &ProbeRequest,
    mut cancellation: impl FnMut() -> Result<()>,
) -> Result<EndpointObservation> {
    request.validate()?;
    cancellation()?;
    #[cfg(not(windows))]
    {
        anyhow::bail!("installation native probe requires Windows")
    }
    #[cfg(windows)]
    {
        use std::{fs, time::Duration};
        let current = std::fs::canonicalize(std::env::current_exe()?)?;
        let tool = crate::media_runtime::BoundTool::bind(&request.runtime)?;
        ensure!(
            std::fs::canonicalize(tool.path())? == current,
            "installation probe must use the current retained runtime"
        );
        let actual = ByteIdentity::observe_file(&current)?;
        ensure!(
            actual == *tool.identity(),
            "installation current worker bytes differ"
        );
        let workspace = tempfile::tempdir()?;
        fs::write(
            workspace.path().join("probe.json"),
            serde_json::to_vec(request)?,
        )?;
        super::owned::run(
            tool.path(),
            vec!["__probe-worker".into()],
            workspace.path(),
            Duration::from_secs(2),
            &mut cancellation,
        )?;
        cancellation()?;
        let observed: EndpointObservation =
            read(&workspace.path().join("observation.json"), 16 * 1024)?;
        verify_observation(request, &observed)?;
        ensure!(
            observed.binding.os_build == device::os_build()?,
            "installation probe OS observation changed"
        );
        Ok(observed)
    }
}

/// Internal child entry. The public launcher must provide process ownership;
/// calling this directly is not a bounded foreground installation session.
pub fn probe_worker() -> Result<()> {
    #[cfg(not(windows))]
    {
        anyhow::bail!("installation native worker requires Windows")
    }
    #[cfg(windows)]
    {
        use std::{fs::OpenOptions, io::Write};
        let request: ProbeRequest = read(Path::new("probe.json"), 4096)?;
        request.validate()?;
        let actual = ByteIdentity::observe_file(&std::env::current_exe()?)?;
        ensure!(
            actual.sha256 == request.runtime.sha256 && actual.bytes == request.runtime.bytes,
            "installation probe worker identity differs"
        );
        let (_, observed) = device::observe(&request.endpoint_id)?;
        verify_observation(&request, &observed)?;
        let raw = serde_json::to_vec(&observed)?;
        ensure!(raw.len() <= 16 * 1024, "installation probe output budget");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open("observation.json")?;
        file.write_all(&raw)?;
        file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_binding_and_probe_scope_reject_implicit_output_or_success_claims() {
        let mut request = ProbeRequest {
            schema: PROBE_SCHEMA.into(),
            endpoint_id: "wasapi:{0.0.0.00000000}.{11111111-1111-1111-1111-111111111111}".into(),
            runtime: FileRef {
                path: std::env::current_exe().unwrap(),
                sha256: "a".repeat(64),
                bytes: 1,
            },
        };
        request.validate().unwrap();
        let mut observation = EndpointObservation {
            binding: EndpointBinding {
                profile: PROFILE.into(),
                device_id: request.endpoint_id.clone(),
                os_build: 26300,
                driver_version: "10.0.test".into(),
                configuration_sha256: "b".repeat(64),
            },
            sample_rate: 48000,
            channels: 2,
            format: "f32".into(),
            requested_buffer_frames: 480,
            effective_buffer_checked: false,
            device_playback_checked: false,
        };
        verify_observation(&request, &observation).unwrap();
        observation.binding.profile = "another-profile".into();
        assert!(verify_observation(&request, &observation).is_err());
        observation.binding.profile = PROFILE.into();
        observation.device_playback_checked = true;
        assert!(verify_observation(&request, &observation).is_err());
        observation.device_playback_checked = false;
        observation.binding.device_id = "another".into();
        assert!(verify_observation(&request, &observation).is_err());
        request.endpoint_id.clear();
        assert!(request.validate().is_err());
    }
}

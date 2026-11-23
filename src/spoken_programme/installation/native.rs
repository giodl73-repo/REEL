//! Explicit native output ports. Metadata validation opens no device.
use super::*;
use serde::{Deserialize, Serialize};

mod callback;
#[cfg(windows)]
pub mod console;
#[cfg(windows)]
pub mod controller;
#[cfg(windows)]
pub mod input;
pub use callback::{
    Callback, CallbackCommandError, CallbackControl, CallbackDelivery, CallbackEmission,
    CallbackReport, callback_pair,
};
#[cfg(windows)]
mod device;
#[cfg(windows)]
pub mod output;
#[cfg(windows)]
mod owned;
#[cfg(windows)]
pub mod parent;
mod probe;
pub mod protocol;
#[cfg(windows)]
pub mod source;
#[cfg(windows)]
pub mod worker;
pub use probe::{ProbeRequest, probe, probe_worker};
#[cfg(windows)]
pub mod pipe;
mod session;
pub mod wire;
pub use session::{
    MAX_REQUEST as MAX_SESSION_REQUEST_BYTES, OriginalProgramme, PlaybackBuild, SessionCheck,
    SessionRequest,
};

pub const PROFILE: &str = "reel.installation.win11-wasapi-pcm24.v1";
pub(super) fn explicit_endpoint_id(id: &str) -> bool {
    let Some(guid) = id
        .strip_prefix("wasapi:{0.0.0.00000000}.{")
        .and_then(|s| s.strip_suffix('}'))
    else {
        return false;
    };
    guid.len() == 36
        && guid.bytes().enumerate().all(|(n, b)| {
            if [8, 13, 18, 23].contains(&n) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointBinding {
    pub profile: String,
    pub device_id: String,
    pub os_build: u32,
    pub driver_version: String,
    pub configuration_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointObservation {
    pub binding: EndpointBinding,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: String,
    pub requested_buffer_frames: u32,
    pub effective_buffer_checked: bool,
    pub device_playback_checked: bool,
}
impl EndpointBinding {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.profile == PROFILE,
            "installation endpoint binding profile"
        );
        ensure!(
            explicit_endpoint_id(&self.device_id)
                && self.device_id.len() <= 1024
                && !self.device_id.chars().any(char::is_control),
            "installation endpoint ID profile"
        );
        ensure!(
            self.os_build >= 22000
                && !self.driver_version.trim().is_empty()
                && self.driver_version.len() <= 128
                && !self.driver_version.chars().any(char::is_control),
            "installation host/driver profile"
        );
        ensure!(
            self.configuration_sha256.len() == 64
                && self
                    .configuration_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "installation endpoint configuration pin"
        );
        Ok(())
    }
}

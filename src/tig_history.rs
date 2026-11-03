//! Thin admitted Reel CAS host; generic orchestration stays in Tig.
use crate::tig_publication::{ExistingPublisher, Profile};
use reel_assets::Config;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tig_git::{history_command, source::native::updates::publication::Error};
use tig_store::{
    Key,
    existing::{Layout, Limits, RequiredProtection},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    schema: String,
    artifact_root: PathBuf,
    namespace: Key,
    storage: Config,
    #[serde(default)]
    reader_limits: Limits,
    protection: RequiredProtection,
}
fn open(enrollment: &history_command::Publication) -> Result<ExistingPublisher, Error> {
    if !matches!(enrollment.layout, Layout::ReelCas) {
        return Err(Error::Unsupported);
    }
    let settings: Settings =
        serde_json::from_value(enrollment.settings.clone()).map_err(|_| Error::Invalid)?;
    if settings.schema != "reel.history-publisher.experimental-1" {
        return Err(Error::Unsupported);
    }
    ExistingPublisher::open(Profile {
        artifact_root: settings.artifact_root,
        namespace: settings.namespace,
        storage: settings.storage,
        destination: enrollment.root.clone(),
        reader_limits: settings.reader_limits,
        protection: settings.protection,
    })
    .map_err(|e| match e {
        tig_store::Error::Unsupported => Error::Unsupported,
        tig_store::Error::Busy | tig_store::Error::Retained => Error::Busy,
        tig_store::Error::Budget => Error::Budget,
        tig_store::Error::InvalidRoot | tig_store::Error::InvalidRequest => Error::Invalid,
        _ => Error::Provider,
    })
}
pub fn execute(action: &str, profile: &Path, request: &Path) -> Result<serde_json::Value, String> {
    history_command::execute(action, profile, request, open)
}

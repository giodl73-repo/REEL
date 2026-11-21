//! Shared private placement profile; request/event documents never contain paths.
use git_asset_store::{ledger::Checkpoint, placement::Config, source_claims};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamespaceRoot {
    pub root: PathBuf,
    pub minimum: source_claims::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderRoot {
    pub root: PathBuf,
    pub ledger_root: PathBuf,
    pub minimum: Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Storage {
    pub schema: String,
    pub config: Config,
    pub namespaces: Vec<NamespaceRoot>,
    pub providers: Vec<ProviderRoot>,
    pub byte_source: Value,
    #[serde(default)]
    pub allow_copy: bool,
    #[serde(default)]
    pub allow_eviction: bool,
    #[serde(default)]
    pub allow_reacquisition: bool,
    #[serde(default)]
    pub allow_cancellation: bool,
}

//! Explicit Git/LFS adapters. No setup, daemon, cache census or lifecycle inference.
#![forbid(unsafe_code)]
#[cfg(test)]
extern crate self as git_asset;
#[cfg(feature = "annex-read")]
pub mod annex;
pub mod cache;
#[cfg(feature = "schema")]
pub mod catalogue;
#[cfg(feature = "github")]
pub mod github;
#[cfg(feature = "schema")]
pub mod history_command;
pub mod lifecycle;
pub mod pointer;
#[cfg(feature = "assets")]
pub mod project;
pub mod protocol;
#[cfg(feature = "assets")]
pub mod restore;
pub mod source;
#[cfg(feature = "upload")]
pub mod upload;

//! Explicit Git/LFS adapters. No setup, daemon, cache census or lifecycle inference.
#![forbid(unsafe_code)]
pub mod cache;
#[cfg(feature = "schema")]
pub mod catalogue;
#[cfg(feature = "schema")]
pub mod history_command;
pub mod pointer;
#[cfg(feature = "assets")]
pub mod project;
pub mod protocol;
#[cfg(feature = "assets")]
pub mod restore;
pub mod source;

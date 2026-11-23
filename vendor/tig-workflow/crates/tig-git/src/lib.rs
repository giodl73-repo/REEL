//! Explicit Git/LFS adapters. No setup, daemon, cache census or lifecycle inference.
#![forbid(unsafe_code)]
pub mod annex;
pub mod cache;
pub mod pointer;
pub mod protocol;
#[cfg(feature = "remote-lfs")]
pub mod remote_lfs;
pub mod source;

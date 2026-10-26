//! Explicit Git/LFS adapters. No setup, daemon, cache census or lifecycle inference.
#![forbid(unsafe_code)]
pub mod cache;
pub mod pointer;
pub mod protocol;
pub mod source;

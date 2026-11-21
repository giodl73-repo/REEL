//! W3a source intake, isolated from accepted producers' SDK dependency graph.
//!
//! This slice compiles exact qualified dependencies and verifies vendored source.
//! It supplies no admission, capacity mutation, producer execution, retention,
//! recovery or adoption entry point. Consumer adapters follow in separate PRs.

/// Exact qualified working-tree manifest; the ancestry commit alone is not a pin.
pub const LIFECYCLE_SOURCE_MANIFEST: &str =
    "f89d836d82b1a6240fbe77f4fd53b488ed57ceb303111642be53fd668ffe2cfe";
/// EVID-151 packet covering the original REEL lifecycle module and its context.
pub const LIFECYCLE_QUALIFICATION: &str =
    "1c3d0b7db734bc946c3fd482134cabaac1c023f67ea7d2f19a0bf4c3f63d88e3";
/// Merged git-asset capacity implementation source; not an installed distribution.
pub const CAPACITY_SOURCE_COMMIT: &str = "fa6697cf62eb61bf9477fd62b6493a024ead4e49";
pub const CAPACITY_SOURCE_MANIFEST: &str =
    "25e4a2173488842fe61201a48149ac4eb5f91f736f832a1a82e9e3e0401fddb4";

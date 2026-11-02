//! Optional typed-record intake. Pure, bounded and independent of storage/REEL.
//! Envelope/pin validation is not authored-schema, producer or custody validation.
#![forbid(unsafe_code)]
#[cfg(feature = "schema")]
pub mod contracts;
#[cfg(feature = "schema")]
pub mod delivery;
#[cfg(feature = "schema")]
pub mod edits;
pub mod graph;
pub mod native;
#[cfg(feature = "schema")]
pub mod publication;
pub mod resolve;
#[cfg(feature = "schema")]
pub mod revisions;
#[cfg(feature = "schema")]
pub mod schema;
pub mod variants;

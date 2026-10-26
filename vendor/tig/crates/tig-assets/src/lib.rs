//! Optional typed-record intake. Pure, bounded and independent of storage/REEL.
//! Envelope/pin validation is not authored-schema, producer or custody validation.
#![forbid(unsafe_code)]
pub mod graph;
pub mod native;
pub mod resolve;

//! Actual retained parent byte reads. This live borrowed custody is NOT durable
//! child input admission; a waiting child must acquire its own declared claims.
use crate::{NativeProfile, admission, error, io_error, retention};
use reel_workflow::{
    Artifact, ErrorKind, Packet, Result, ReturnedResult, publication::InputReader,
    upstream as contract,
};
use std::{collections::BTreeMap, io::Read};
use tig_core::{Digest, Object};
use tig_git::annex::workflow::reader;

pub struct Parent<'a> {
    pub packet: &'a Packet,
    pub result: &'a ReturnedResult,
    pub source: &'a NativeProfile,
    pub admission: &'a admission::Profile,
    pub receipt: &'a admission::Receipt,
    pub acknowledgement: &'a retention::Acknowledgement,
}
pub struct BorrowedInputs<'a> {
    bound: BTreeMap<String, contract::BoundInput>,
    parents: BTreeMap<String, Parent<'a>>,
    held: Vec<reader::VerifiedRead>,
    captured: BTreeMap<Object, Vec<u8>>,
    reads: usize,
    // Drop object gates before tool pins and finally retained-owner fences.
    _pins: Vec<retention::ReadPin>,
    _fences: admission::RetainedFences,
}
impl<'a> BorrowedInputs<'a> {
    pub fn open(
        child: &Packet,
        bindings: &contract::Bindings,
        parents: BTreeMap<String, Parent<'a>>,
    ) -> Result<Self> {
        let observed = parents
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    contract::Parent {
                        packet: p.packet,
                        result: p.result,
                    },
                )
            })
            .collect();
        let bound = contract::bind(child, bindings, &observed)?;
        // The currently qualified native protocol declares one exact reader
        // store/pool/controller configuration. Do not imply multi-pool support.
        let mut source_pin = None;
        let mut fences = admission::RetainedFences::default();
        let mut pins = Vec::new();
        for parent in parents.values() {
            let pin = reel_workflow::canonical_digest(&parent.source.source)?;
            if source_pin.as_ref().is_some_and(|expected| expected != &pin) {
                return Err(error(
                    ErrorKind::Unsupported,
                    "upstream-single-reader-profile-required",
                ));
            }
            source_pin = Some(pin);
            pins.push(retention::pin_for_read(parent, &mut fences)?);
        }
        Ok(Self {
            bound,
            parents,
            _fences: fences,
            _pins: pins,
            held: Vec::new(),
            captured: BTreeMap::new(),
            reads: 0,
        })
    }
}
impl InputReader for BorrowedInputs<'_> {
    fn read(&mut self, asset: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        let bound = self
            .bound
            .get(&asset.logical_id)
            .ok_or_else(|| error(ErrorKind::Conflict, "upstream-undeclared-read"))?;
        if &bound.artifact != asset {
            return Err(error(
                ErrorKind::Conflict,
                "upstream-exact-artifact-required",
            ));
        }
        if asset.bytes > maximum || self.reads >= 256 {
            return Err(error(ErrorKind::Budget, "upstream-read-allowance"));
        }
        self.reads += 1;
        let object = Object {
            sha256: Digest::parse(&asset.sha256)
                .map_err(|_| error(ErrorKind::Invalid, "upstream-input-digest"))?,
            bytes: asset.bytes,
        };
        // Logical task/result/selector identity was checked before physical reuse.
        if let Some(raw) = self.captured.get(&object) {
            return Ok(raw.clone());
        }
        let parent = &self.parents[&bound.reference.task_id];
        let mut held = reader::open(&parent.source.source, &object).map_err(io_error)?;
        let mut raw = Vec::new();
        Read::by_ref(&mut held)
            .take(asset.bytes + 1)
            .read_to_end(&mut raw)
            .map_err(io_error)?;
        if raw.len() as u64 != asset.bytes || reel_workflow::digest(&raw) != asset.sha256 {
            return Err(error(ErrorKind::Integrity, "upstream-input-content"));
        }
        self.held.push(held);
        self.captured.insert(object, raw.clone());
        Ok(raw)
    }
}

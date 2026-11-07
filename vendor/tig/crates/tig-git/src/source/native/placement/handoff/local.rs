//! Verification adapter for already-enrolled LocalStore grants. No cache creation.
use super::{Error, Protection, Result};
use std::collections::BTreeMap;
use tig_core::{Object, Placement};
use tig_store::{
    AckLevel,
    ledger::CapacityLedger,
    local::LocalStore,
    retention::{self, RetainedReplica, RetentionGrant},
};
pub struct Place {
    pub store: LocalStore,
    pub ledger: CapacityLedger,
    pub grants: BTreeMap<Object, RetentionGrant>,
}
pub struct Local {
    pub places: BTreeMap<String, Place>,
}
pub struct Guard {
    place: String,
    retained: RetainedReplica,
}
impl Protection for Local {
    type Guard = Guard;
    fn verify(&mut self, placement: &Placement, required: AckLevel) -> Result<Guard> {
        let place = self
            .places
            .get_mut(&placement.place)
            .ok_or(Error::Protection)?;
        if place.store.place().as_str() != placement.place {
            return Err(Error::Protection);
        }
        let grant = place
            .grants
            .get(&placement.object)
            .ok_or(Error::Protection)?;
        if grant.request().publication.object != placement.object
            || grant.request().minimum_ack < required
        {
            return Err(Error::Protection);
        }
        let retained = retention::verify(&mut place.ledger, &place.store, grant)
            .map_err(|_| Error::Protection)?;
        Ok(Guard {
            place: placement.place.clone(),
            retained,
        })
    }
    fn revalidate(&mut self, guard: &mut Guard) -> Result<()> {
        let place = self.places.get_mut(&guard.place).ok_or(Error::Protection)?;
        if place.store.place().as_str() != guard.place {
            return Err(Error::Protection);
        }
        guard
            .retained
            .revalidate(&mut place.ledger, &place.store)
            .map_err(|_| Error::Protection)
    }
}

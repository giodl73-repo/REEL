//! Open existing native authorities only; payload providers remain lazy.
use super::model::{Authorities, Enrollment};
use tig_store::{
    metadata::MetadataStore,
    source_claims::{SourceJournal, protection::Index},
};

pub struct Session {
    pub index: Index,
    pub journal: SourceJournal,
    pub nodes: MetadataStore,
}
impl Session {
    pub fn open(authorities: &Authorities) -> Result<Self, String> {
        authorities.validate()?;
        let index = Index::open(
            &authorities.index.root,
            &authorities.index.minimum_checkpoint,
        )
        .map_err(|_| "index unavailable or below required witness")?;
        let journal = SourceJournal::open(
            &authorities.journal.root,
            &authorities.journal.minimum_checkpoint,
        )
        .map_err(|_| "journal unavailable or below required witness")?;
        let nodes = MetadataStore::open(
            &authorities.metadata.root,
            &authorities.metadata.minimum_checkpoint,
        )
        .map_err(|_| "metadata unavailable or below required witness")?;
        if index.identity() != &authorities.index.identity
            || journal.identity() != &authorities.journal.identity
        {
            return Err("native authority identity changed".into());
        }
        Ok(Self {
            index,
            journal,
            nodes,
        })
    }

    /// Validate every supplied witness against the original native histories.
    /// Comparing sequence numbers alone cannot validate forked checkpoints.
    pub fn reconcile(authorities: &Authorities, required: &Authorities) -> Result<Self, String> {
        if !authorities.same_binding(required) {
            return Err("changed native authority binding".into());
        }
        drop(Self::open(required)?);
        Self::open(authorities)
    }

    pub fn witnesses(&self, bindings: &Authorities) -> Authorities {
        let mut result = bindings.clone();
        result.index.minimum_checkpoint = self.index.checkpoint();
        result.journal.minimum_checkpoint = self.journal.checkpoint();
        result.metadata.minimum_checkpoint = self.nodes.checkpoint();
        result
    }

    pub fn enrollment(enrollment: &Enrollment) -> tig_store::retirement_intent::Enrollment {
        tig_store::retirement_intent::Enrollment {
            owner: enrollment.owner.clone(),
            subject: enrollment.subject.clone(),
        }
    }
}

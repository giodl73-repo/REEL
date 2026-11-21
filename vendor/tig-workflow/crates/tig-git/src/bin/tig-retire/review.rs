use super::{codec, model};
use tig_core::retirement::{Catalogue, Preview, review::Review};

/// Pure admitted-fact review only: no provider observation or execution authority.
pub fn review(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let value: serde_json::Value = codec::decode(bytes)?;
    if value["schema"] == model::EVICTION_REVIEW_INPUT {
        let input: model::EvictionReviewInput = codec::decode(bytes)?;
        if input.schema != model::EVICTION_REVIEW_INPUT {
            return Err("unsupported eviction review schema".into());
        }
        let selection = tig_core::retirement::review::eviction::Selection::from_json(
            &codec::encode(&input.selection)?,
        )
        .map_err(|_| "invalid exact placement selection")?;
        let review = Review::create_eviction(
            selection,
            input.owner,
            input.places,
            input.protection,
            input.inventory,
            input.releases,
        )
        .map_err(|e| format!("eviction review refused ({e:?})"))?;
        return review
            .to_json()
            .map_err(|_| "cannot encode bounded eviction review".into());
    }
    let input: model::ReviewInput = codec::decode(bytes)?;
    if input.schema != model::REVIEW_INPUT {
        return Err("unsupported retirement review schema".into());
    }
    let catalogue = Catalogue {
        snapshot: input.catalogue.snapshot,
        bindings: input.catalogue.bindings,
    };
    let preview = Preview::create(&catalogue, input.selected, input.policy, input.at)
        .map_err(|e| format!("retirement selection refused ({e:?})"))?;
    let review = Review::create(
        preview,
        input.owner,
        input.places,
        input.protection,
        input.inventory,
        input.releases,
    )
    .map_err(|e| format!("retirement review refused ({e:?})"))?;
    review
        .to_json()
        .map_err(|_| "cannot encode bounded retirement review".into())
}

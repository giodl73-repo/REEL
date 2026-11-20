//! Exact ordered viewing-material declarations, without reading images or
//! granting native custody, viewing, selection or creative authority.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComicSources {
    schema: String,
    task_key: String,
    edition_id: String,
    language: String,
    page_order: Vec<String>,
    reading: BTreeMap<String, Artifact>,
}

/// Extract the exact PNG input pins in reading order. Consumers must declare
/// these as task inputs and separately bind/protect their original native
/// records or retained producer selectors; a manifest is not byte authority.
pub fn comic_reading_inputs(copy: &Copy, raw: &[u8]) -> Result<Vec<Artifact>> {
    check_copy(copy)?;
    let Format::Comic { page_order } = &copy.format else {
        return fail(ErrorKind::Unsupported, "review-material-comic-required");
    };
    if copy.review.media_type != "application/json" || raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "review-material-manifest-bound");
    }
    if copy.review.bytes != raw.len() as u64 || copy.review.sha256 != digest(raw) {
        return fail(ErrorKind::Integrity, "review-material-manifest-pin");
    }
    let manifest: ComicSources = decode(raw)?;
    if manifest.schema != "reel.workflow-comic-review.v1"
        || !hex(&manifest.task_key, &[64])
        || !token(&manifest.edition_id)
        || manifest.language != copy.language
        || &manifest.page_order != page_order
        || manifest.reading.len() != page_order.len()
    {
        return fail(ErrorKind::Conflict, "review-material-edition-order");
    }
    let mut ids = BTreeSet::new();
    page_order
        .iter()
        .map(|page| {
            let pin = manifest.reading.get(page).ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "review-material-page".into(),
            })?;
            artifact(pin)?;
            if pin.media_type != "image/png" || !ids.insert(pin.logical_id.clone()) {
                return fail(ErrorKind::Conflict, "review-material-reading-identity");
            }
            Ok(pin.clone())
        })
        .collect()
}

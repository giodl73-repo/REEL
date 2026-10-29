//! Ordered byte acknowledgements, not atomic batch custody or output selection.
use super::{OutputBinding, RenderedCandidate, receipt_bytes};
use anyhow::{Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Cursor,
};
use tig_assets::graph::recipe::produced::Claim;
use tig_core::{Digest, LogicalPath, Object, identity};
use tig_store::{AckLevel, Key, PublicationBackend, Receipt, Request};

/// Caller-owned operation identity. Use the same identity for exact retries;
/// a changed render needs its own operation, never an implicit content-key retry.
pub struct Publication {
    pub operation: Key,
    pub owner: Key,
    pub required: AckLevel,
}

/// Explicit complete-delivery budget, including the receipt and claim. Object
/// count and bytes are deduplicated by hash, not allocated-disk measurements.
pub struct DeliveryBudget {
    pub max_objects: usize,
    pub max_bytes: u64,
}

/// Issued after all selected payloads, native receipt and claim are acknowledged.
/// No Deserialize/constructor, selection, retained custody or removal authority.
pub struct PublishedCandidate {
    claim: Claim,
    native_receipt: Object,
    acknowledgements: Vec<Receipt>,
    delivery_outputs: Option<BTreeMap<String, Object>>,
}
impl PublishedCandidate {
    pub fn claim(&self) -> &Claim {
        &self.claim
    }
    pub fn receipt_object(&self) -> &Object {
        &self.native_receipt
    }
    pub fn acknowledgements(&self) -> &[Receipt] {
        &self.acknowledgements
    }
    /// Present only after explicit complete-delivery publication. Acknowledged
    /// objects still need owner adoption/retention; this is not permanent custody.
    pub fn delivery_outputs(&self) -> Option<&BTreeMap<String, Object>> {
        self.delivery_outputs.as_ref()
    }
}

impl Publication {
    pub(crate) fn request(&self, role: &str, name: &str, object: Object) -> Result<Request> {
        let bytes = identity::canonical_json(&serde_json::json!({
            "schema":"reel.native-candidate-publication-key.v1",
            "operation":self.operation.as_str(),"role":role,"name":name,
        }))?;
        Ok(Request {
            operation: Key::parse(Digest::blob(&bytes).as_str())?,
            owner: self.owner.clone(),
            object,
            required: self.required,
        })
    }
}

impl RenderedCandidate {
    /// Revalidate local candidate bytes, preflight all requests, then acknowledge
    /// selected payloads -> native receipt -> claim. Never remove source files.
    /// Caller retains the original context/runtime/job obligations separately.
    pub fn publish(
        &self,
        bindings: &[OutputBinding<'_>],
        publication: &Publication,
        backend: &mut impl PublicationBackend,
    ) -> Result<PublishedCandidate> {
        self.publish_impl(bindings, publication, backend, None)
    }

    /// Explicitly publish every independently checked native output, then receipt
    /// and claim. Preflight all local pins/capabilities/budgets before any write.
    /// Errors may leave acknowledged prefixes; exact retries use the same keys.
    /// No source removal, atomic-batch custody or new creative selection occurs.
    pub fn publish_delivery(
        &self,
        bindings: &[OutputBinding<'_>],
        publication: &Publication,
        backend: &mut impl PublicationBackend,
        budget: &DeliveryBudget,
    ) -> Result<PublishedCandidate> {
        self.publish_impl(bindings, publication, backend, Some(budget))
    }

    fn publish_impl(
        &self,
        bindings: &[OutputBinding<'_>],
        publication: &Publication,
        backend: &mut impl PublicationBackend,
        budget: Option<&DeliveryBudget>,
    ) -> Result<PublishedCandidate> {
        if budget.is_some_and(|b| b.max_objects == 0 || b.max_bytes == 0)
            || (budget.is_some() && self.receipt.outputs.len() > 4096)
        {
            bail!("complete delivery publication exceeds budget");
        }
        // claim validates all mappings and source pins before any provider write.
        let claim = self.claim(bindings)?;
        let native_receipt = receipt_bytes(&self.output)?;
        if Object::from_bytes(&native_receipt) != self.receipt_object {
            bail!("rendered candidate receipt changed before publication");
        }
        let names = if budget.is_some() {
            self.receipt
                .outputs
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
        } else {
            bindings.iter().map(|binding| binding.output_name).collect()
        };
        let mut outputs = Vec::new();
        let mut delivery_outputs = BTreeMap::new();
        for name in names {
            let item = &self.receipt.outputs[name];
            let object = Object {
                sha256: Digest::parse(&item.sha256)?,
                bytes: item.bytes,
            };
            let path = if budget.is_some() {
                LogicalPath::parse(&format!("native-outputs/{name}"))?;
                if item.path.to_str() != Some(name) {
                    bail!("native delivery output name mismatch");
                }
                crate::scene_delivery::checked_file(&self.output, item)?
            } else {
                self.output.join(&item.path)
            };
            let request = publication.request("output", name, object.clone())?;
            delivery_outputs.insert(name.to_owned(), object);
            outputs.push((request, path));
        }
        publish_checked(
            claim,
            &native_receipt,
            &self.receipt_object,
            outputs,
            budget.map(|_| delivery_outputs),
            publication,
            backend,
            budget,
        )
    }
}

/// Shared publication mechanics for sealed native scene/episode candidates.
/// Callers independently validate every local payload and exact receipt first.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_checked(
    claim: Claim,
    native_receipt: &[u8],
    receipt_object: &Object,
    mut outputs: Vec<(Request, std::path::PathBuf)>,
    delivery_outputs: Option<BTreeMap<String, Object>>,
    publication: &Publication,
    backend: &mut impl PublicationBackend,
    budget: Option<&DeliveryBudget>,
) -> Result<PublishedCandidate> {
    if Object::from_bytes(native_receipt) != *receipt_object {
        bail!("native candidate receipt changed before publication");
    }
    // Stable ordering also makes partial retry independent of mapping order.
    outputs.sort_by(|a, b| a.0.operation.as_str().cmp(b.0.operation.as_str()));
    let receipt_request = publication.request("receipt", "", receipt_object.clone())?;
    let claim_request = publication.request("claim", "", claim.object().clone())?;
    let capabilities = backend.capabilities();
    let mut sizes = BTreeMap::new();
    for request in outputs
        .iter()
        .map(|(request, _)| request)
        .chain([&receipt_request, &claim_request])
    {
        capabilities.check(request)?;
        if sizes
            .insert(request.object.sha256.clone(), request.object.bytes)
            .is_some_and(|previous| previous != request.object.bytes)
        {
            bail!("conflicting delivery object sizes");
        }
    }
    if let Some(budget) = budget {
        let bytes = sizes
            .values()
            .try_fold(0_u64, |sum, bytes| sum.checked_add(*bytes))
            .ok_or_else(|| anyhow::anyhow!("delivery publication byte budget overflow"))?;
        if sizes.len() > budget.max_objects || bytes > budget.max_bytes {
            bail!("complete delivery publication exceeds budget");
        }
    }
    let mut acknowledgements = Vec::with_capacity(outputs.len() + 2);
    for (request, path) in outputs {
        let mut source = File::open(path)?;
        acknowledgements.push(tig_store::publish(backend, &request, &mut source)?);
    }
    acknowledgements.push(tig_store::publish(
        backend,
        &receipt_request,
        &mut Cursor::new(native_receipt),
    )?);
    acknowledgements.push(tig_store::publish(
        backend,
        &claim_request,
        &mut Cursor::new(claim.bytes()),
    )?);
    Ok(PublishedCandidate {
        claim,
        native_receipt: receipt_object.clone(),
        acknowledgements,
        delivery_outputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_keys_bind_caller_role_and_name_without_hiding_content_conflicts() {
        let mut publication = Publication {
            operation: Key::parse("render-job").unwrap(),
            owner: Key::parse("producer").unwrap(),
            required: AckLevel::ProcessRestart,
        };
        let first = Object::from_bytes(b"original");
        let request = publication
            .request("output", "master.mkv", first.clone())
            .unwrap();
        let changed = publication
            .request("output", "master.mkv", Object::from_bytes(b"new version"))
            .unwrap();
        assert_eq!(request.operation, changed.operation);
        assert_ne!(request.object, changed.object);
        let other_role = publication
            .request("receipt", "master.mkv", first.clone())
            .unwrap();
        assert_ne!(request.operation, other_role.operation);
        let other_name = publication
            .request("output", "other.mkv", first.clone())
            .unwrap();
        assert_ne!(request.operation, other_name.operation);
        publication.operation = Key::parse("next-render-job").unwrap();
        assert_ne!(
            request.operation,
            publication
                .request("output", "master.mkv", first)
                .unwrap()
                .operation
        );
    }
}

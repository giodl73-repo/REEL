//! Ordered byte acknowledgements, not atomic batch custody or output selection.
use super::{OutputBinding, RenderedCandidate, receipt_bytes};
use anyhow::{Result, bail};
use std::{collections::BTreeSet, fs::File, io::Cursor};
use tig_assets::graph::recipe::produced::Claim;
use tig_core::{Digest, Object, identity};
use tig_store::{AckLevel, Key, PublicationBackend, Receipt, Request};

/// Caller-owned operation identity. Use the same identity for exact retries;
/// a changed render needs its own operation, never an implicit content-key retry.
pub struct Publication {
    pub operation: Key,
    pub owner: Key,
    pub required: AckLevel,
}

/// Issued after all selected payloads, native receipt and claim are acknowledged.
/// No Deserialize/constructor, selection, retained custody or removal authority.
pub struct PublishedCandidate {
    claim: Claim,
    native_receipt: Object,
    acknowledgements: Vec<Receipt>,
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
}

impl Publication {
    fn request(&self, role: &str, name: &str, object: Object) -> Result<Request> {
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
        // claim validates all mappings and source pins before any provider write.
        let claim = self.claim(bindings)?;
        let native_receipt = receipt_bytes(&self.output)?;
        if Object::from_bytes(&native_receipt) != self.receipt_object {
            bail!("rendered candidate receipt changed before publication");
        }
        let mut seen = BTreeSet::new();
        let mut outputs = Vec::new();
        for binding in bindings {
            if seen.insert(binding.output_name) {
                let item = &self.receipt.outputs[binding.output_name];
                let request = publication.request(
                    "output",
                    binding.output_name,
                    binding.subject.subject().object().clone(),
                )?;
                outputs.push((request, self.output.join(&item.path)));
            }
        }
        // Stable ordering also makes partial retry independent of mapping order.
        outputs.sort_by(|a, b| a.0.operation.as_str().cmp(b.0.operation.as_str()));
        let receipt_request = publication.request("receipt", "", self.receipt_object.clone())?;
        let claim_request = publication.request("claim", "", claim.object().clone())?;
        let capabilities = backend.capabilities();
        for request in outputs
            .iter()
            .map(|(request, _)| request)
            .chain([&receipt_request, &claim_request])
        {
            capabilities.check(request)?;
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
            native_receipt: self.receipt_object.clone(),
            acknowledgements,
        })
    }
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

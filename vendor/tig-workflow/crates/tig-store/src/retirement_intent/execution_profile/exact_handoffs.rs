use super::*;
use crate::retention::handoff::HandoffRequest;

impl Context {
    /// Opt into exact retained-handoff specifications. This grants no authority.
    /// All selected releases must use handoff; final loss and history refuse.
    pub fn bind_exact_handoffs(mut self, requests: &[HandoffRequest]) -> Result<Self> {
        if requests.is_empty() || requests.len() > 256 {
            return Err(Error::Budget);
        }
        let mut specs = BTreeMap::new();
        for request in requests {
            if specs
                .insert(
                    request.previous.claim_id().clone(),
                    request.binding_fingerprint().map_err(|_| Error::Invalid)?,
                )
                .is_some()
            {
                return Err(Error::Invalid);
            }
        }
        self.schema = EXACT_SCHEMA;
        self.exact_handoffs = Some(specs);
        self.to_json()?;
        Ok(self)
    }
    pub fn exact_handoffs(&self) -> Option<&BTreeMap<Digest, Digest>> {
        self.exact_handoffs.as_ref()
    }
    pub(super) fn validate_exact_handoffs(&self) -> Result<()> {
        match (self.schema, &self.exact_handoffs) {
            (SCHEMA, None) => Ok(()),
            (EXACT_SCHEMA | POLICY_SCHEMA, Some(specs)) => {
                let Some(refs) = self.bindings.get(&Role::HandoffRelease) else {
                    return Err(Error::Invalid);
                };
                if specs.is_empty()
                    || specs.len() > 256
                    || !self.history_releases.is_empty()
                    || self.bindings.contains_key(&Role::RetirementRelease)
                    || specs.keys().ne(refs.keys())
                {
                    return Err(Error::Invalid);
                }
                Ok(())
            }
            (POLICY_SCHEMA, None)
                if self.history_releases.is_empty()
                    && !self.bindings.contains_key(&Role::HandoffRelease)
                    && !self.bindings.contains_key(&Role::RetirementRelease) =>
            {
                Ok(())
            }
            _ => Err(Error::Invalid),
        }
    }
    /// Check an exact specification when opted in. This is no custody or
    /// authority observation; native work must still validate its own request.
    pub fn verify_handoff_binding(&self, request: &HandoffRequest) -> Result<()> {
        if let Some(specs) = &self.exact_handoffs {
            let observed = request.binding_fingerprint().map_err(|_| Error::Invalid)?;
            if specs.get(request.previous.claim_id()) != Some(&observed) {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }
}

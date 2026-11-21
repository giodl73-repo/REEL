//! Admitted, versioned capability evidence. A profile is not provider authority.
use super::{
    ExactSource, Located, Lookup, Opened, ReadCapabilities, custody, publication, removal,
};
use crate::{Key, acquire::SourceError};
use serde::{Deserialize, Serialize};
use tig_core::{Digest, Object, identity};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Schema {
    #[serde(rename = "tig.adapter-profile.experimental-1")]
    Experimental1,
}

/// Trusted configuration identity, never a URL, credential or content version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub place: Digest,
    pub authority: Digest,
    pub generation: u64,
}

/// IO-free current configuration view. Enrollment/authenticity and coherent
/// generation updates are caller responsibilities, not proved by this trait.
pub trait BindingView {
    fn current(&self) -> Result<Binding, SourceError>;
}
impl BindingView for Binding {
    fn current(&self) -> Result<Binding, SourceError> {
        Ok(self.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Qualification<T> {
    Unsupported,
    Unqualified,
    Qualified { capabilities: T, evidence: Digest },
}
impl<T: PartialEq> Qualification<T> {
    fn check(&self, current: &T) -> Result<(), SourceError> {
        match self {
            Self::Unsupported => Err(SourceError::Unsupported),
            Self::Unqualified => Err(SourceError::Unknown),
            Self::Qualified { capabilities, .. } if capabilities == current => Ok(()),
            Self::Qualified { .. } => Err(SourceError::Stale),
        }
    }
}

/// Bounded typed profile; serialization/fingerprint only, no permissive import.
/// Evidence identifiers point to externally admitted qualification records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: Schema,
    pub binding: Binding,
    pub adapter: Key,
    pub adapter_build: Digest,
    pub provider: Key,
    pub provider_build: Digest,
    pub read: Qualification<ReadCapabilities>,
    pub publish: Qualification<publication::Capabilities>,
    pub custody: Qualification<custody::Capabilities>,
    pub issue: Qualification<custody::create::Capabilities>,
    pub release: Qualification<custody::release::Capabilities>,
    pub remove: Qualification<removal::Capabilities>,
}
impl Profile {
    pub fn fingerprint(&self) -> Result<Digest, SourceError> {
        if self.binding.generation == 0 {
            return Err(SourceError::Invalid);
        }
        macro_rules! place {
            ($field:ident) => {
                if let Qualification::Qualified { capabilities, .. } = &self.$field {
                    if capabilities.place != self.binding.place {
                        return Err(SourceError::Invalid);
                    }
                }
            };
        }
        place!(read);
        place!(publish);
        place!(custody);
        place!(issue);
        place!(release);
        place!(remove);
        let value = serde_json::to_value(("tig.adapter-profile-fingerprint.experimental-1", self))
            .map_err(|_| SourceError::Invalid)?;
        let bytes = identity::canonical_json(&value).map_err(|_| SourceError::Invalid)?;
        if bytes.len() > 16_384 {
            return Err(SourceError::Budget);
        }
        Ok(Digest::blob(&bytes))
    }
    /// No provider/configuration calls: a satisfied lazy destination may skip
    /// source execution even if its old source binding is now unavailable.
    pub fn admit<'a, B: BindingView>(
        &'a self,
        expected: &Digest,
        binding: &'a B,
    ) -> Result<Guard<'a, B>, SourceError> {
        if self.fingerprint()? != *expected {
            return Err(SourceError::Stale);
        }
        Ok(Guard {
            profile: self,
            binding,
        })
    }
}

pub struct Guard<'a, B: BindingView> {
    profile: &'a Profile,
    binding: &'a B,
}
impl<'g, B: BindingView> Guard<'g, B> {
    /// Configuration/capability preflight only; grants no provider authority.
    pub fn check_publication(&self, caps: &publication::Capabilities) -> Result<(), SourceError> {
        self.current()?;
        self.profile.publish.check(caps)
    }
    fn current(&self) -> Result<(), SourceError> {
        if self.binding.current()? != self.profile.binding {
            return Err(SourceError::Stale);
        }
        Ok(())
    }
    fn read(&self, caps: &ReadCapabilities) -> Result<(), SourceError> {
        self.current()?;
        self.profile.read.check(caps)
    }
    pub fn publish<P: publication::ExactPublisher>(
        &self,
        publisher: &mut P,
        place: &Digest,
        request: &crate::Request,
        source: &mut dyn std::io::Read,
    ) -> Result<publication::Published<P::Evidence>, publication::Error> {
        self.check_publication(&publisher.describe())
            .map_err(pub_error)?;
        let result = publication::publish_exact(publisher, place, request, source);
        self.check_publication(&publisher.describe())
            .map_err(pub_error)?;
        result
    }
    /// Configuration/capability preflight only; not permission to create a claim.
    pub fn check_issue(&self, caps: &custody::create::Capabilities) -> Result<(), SourceError> {
        self.current()?;
        self.profile.issue.check(caps)
    }
    pub fn issue<I: custody::create::Issuer>(
        &self,
        issuer: &mut I,
        request: &custody::create::Request,
        cooperative: bool,
    ) -> Result<custody::create::Issued<I::Evidence>, custody::create::Error> {
        self.check_issue(&issuer.describe()).map_err(issue_error)?;
        let result = custody::create::issue_exact(issuer, request, cooperative);
        self.check_issue(&issuer.describe()).map_err(issue_error)?;
        result
    }
    pub fn release<E: custody::release::Executor>(
        &self,
        executor: &mut E,
        request: &custody::release::Request,
        cooperative: bool,
    ) -> Result<custody::release::Released<E::Evidence>, custody::release::Error> {
        self.current()
            .and_then(|()| self.profile.release.check(&executor.describe()))
            .map_err(release_error)?;
        let result = custody::release::release_exact(executor, request, cooperative);
        self.current()
            .and_then(|()| self.profile.release.check(&executor.describe()))
            .map_err(release_error)?;
        result
    }
    pub fn remove<R: removal::ExactRemover>(
        &self,
        remover: &mut R,
        request: &removal::Request,
        cooperative: bool,
    ) -> Result<removal::Removed<R::Evidence>, removal::Error> {
        self.current()
            .and_then(|()| self.profile.remove.check(&remover.describe()))
            .map_err(remove_error)?;
        let result = removal::remove_exact(remover, request, cooperative);
        self.current()
            .and_then(|()| self.profile.remove.check(&remover.describe()))
            .map_err(remove_error)?;
        result
    }
    pub fn acquire<'a, R: custody::Retainer>(
        &'a self,
        retainer: &'a mut R,
        wanted: &custody::Commitment,
        cooperative: bool,
    ) -> Result<Box<dyn custody::Held + 'a>, SourceError> {
        self.current()?;
        let caps = retainer.describe();
        self.check_custody(&caps)?;
        caps.check(wanted, cooperative)?;
        let held = retainer.acquire(wanted)?;
        self.bind_custody(&caps, wanted, cooperative, held)
    }
    /// Preflight only. Actual custody still requires the provider's live hold.
    pub fn check_custody(&self, caps: &custody::Capabilities) -> Result<(), SourceError> {
        self.current()?;
        self.profile.custody.check(caps)
    }
    /// Attach admitted configuration checks to an existing effective hold.
    /// The returned wrapper borrows the binding, not this temporary Guard. Its
    /// underlying protection survives failed revalidation until the hold drops.
    pub fn bind_custody<'h>(
        &self,
        caps: &custody::Capabilities,
        wanted: &custody::Commitment,
        cooperative: bool,
        held: Box<dyn custody::Held + 'h>,
    ) -> Result<Box<dyn custody::Held + 'h>, SourceError>
    where
        'g: 'h,
    {
        self.check_custody(caps)?;
        caps.check(wanted, cooperative)?;
        if held.commitment() != wanted {
            return Err(SourceError::Stale);
        }
        held.fresh()?;
        self.current()?;
        Ok(Box::new(Held {
            held,
            binding: self.binding,
            expected: self.profile.binding.clone(),
        }))
    }
}

struct Held<'a, B: BindingView> {
    held: Box<dyn custody::Held + 'a>,
    binding: &'a B,
    expected: Binding,
}
impl<B: BindingView> Held<'_, B> {
    fn current(&self) -> Result<(), SourceError> {
        if self.binding.current()? != self.expected {
            return Err(SourceError::Stale);
        }
        Ok(())
    }
}
impl<B: BindingView> custody::Held for Held<'_, B> {
    fn commitment(&self) -> &custody::Commitment {
        self.held.commitment()
    }
    fn revalidate(&mut self) -> Result<(), SourceError> {
        self.current()?;
        self.held.revalidate()?;
        self.current()
    }
    fn fresh(&self) -> Result<(), SourceError> {
        self.current()?;
        self.held.fresh()?;
        self.current()
    }
}

pub struct Source<'a, 'g, S: ExactSource, B: BindingView> {
    source: &'a mut S,
    guard: &'a Guard<'g, B>,
}
impl<'a, 'g, S: ExactSource, B: BindingView> Source<'a, 'g, S, B> {
    pub fn new(source: &'a mut S, guard: &'a Guard<'g, B>) -> Self {
        Self { source, guard }
    }
}
impl<S: ExactSource, B: BindingView> ExactSource for Source<'_, '_, S, B> {
    type Reader = S::Reader;
    fn describe(&self) -> ReadCapabilities {
        self.source.describe()
    }
    fn lookup(&mut self, object: &Object) -> Result<Lookup, SourceError> {
        self.guard.read(&self.source.describe())?;
        let result = self.source.lookup(object);
        self.guard.read(&self.source.describe())?;
        result
    }
    fn open_version(&mut self, location: &Located) -> Result<Opened<Self::Reader>, SourceError> {
        self.guard.read(&self.source.describe())?;
        let result = self.source.open_version(location);
        self.guard.read(&self.source.describe())?;
        result
    }
}

macro_rules! errors {
    ($name:ident, $module:path) => {
        fn $name(e: SourceError) -> $module {
            use $module as E;
            match e {
                SourceError::Invalid => E::Invalid,
                SourceError::Unsupported => E::Unsupported,
                SourceError::Stale => E::Stale,
                SourceError::Budget => E::Budget,
                SourceError::Denied => E::Denied,
                SourceError::Unavailable => E::Unavailable,
                SourceError::Busy => E::Busy,
                SourceError::Integrity => E::Integrity,
                _ => E::Unknown,
            }
        }
    };
}
errors!(pub_error, publication::Error);
errors!(issue_error, custody::create::Error);
errors!(release_error, custody::release::Error);
errors!(remove_error, removal::Error);

/// Bridge a guard into the existing lazy acquisition coordinator.
pub struct Publisher<'a, 'g, P: publication::ExactPublisher, B: BindingView> {
    publisher: &'a mut P,
    guard: &'a Guard<'g, B>,
}
impl<'a, 'g, P: publication::ExactPublisher, B: BindingView> Publisher<'a, 'g, P, B> {
    pub fn new(publisher: &'a mut P, guard: &'a Guard<'g, B>) -> Self {
        Self { publisher, guard }
    }
}
impl<P: publication::ExactPublisher, B: BindingView> publication::ExactPublisher
    for Publisher<'_, '_, P, B>
{
    type Evidence = P::Evidence;
    fn describe(&self) -> publication::Capabilities {
        self.publisher.describe()
    }
    fn publish(
        &mut self,
        request: &crate::Request,
        source: &mut dyn std::io::Read,
    ) -> Result<publication::Published<Self::Evidence>, publication::Error> {
        let place = self.publisher.describe().place;
        self.guard.publish(self.publisher, &place, request, source)
    }
}

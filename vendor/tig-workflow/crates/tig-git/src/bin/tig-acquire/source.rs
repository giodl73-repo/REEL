use super::source_config::Config;
use std::io::Read;
use tig_core::Object;
use tig_store::{
    acquire::{ObjectSource, PublicationSource, SourceError},
    existing,
    local::LocalStore,
};

pub struct Deferred {
    config: Option<Config>,
    max: u64,
    opened: bool,
}
impl Deferred {
    pub fn new(config: Config) -> Self {
        let max = match &config {
            Config::Native {
                max_object_bytes, ..
            } => *max_object_bytes,
            Config::Existing { profile } => profile.limits.object_bytes,
        };
        Self {
            config: Some(config),
            max,
            opened: false,
        }
    }
    pub fn was_opened(&self) -> bool {
        self.opened
    }
}
impl ObjectSource for Deferred {
    type Reader = Box<dyn Read>;
    fn max_object_bytes(&self) -> u64 {
        self.max
    }
    fn open(&mut self, object: &Object) -> Result<Self::Reader, SourceError> {
        self.opened = true;
        match self.config.take().ok_or(SourceError::Unavailable)? {
            Config::Native {
                root,
                place,
                publication,
                ..
            } => {
                let store = LocalStore::open(&root).map_err(|_| SourceError::Unavailable)?;
                if store.identity() != &place {
                    return Err(SourceError::Stale);
                }
                PublicationSource::new(&store, publication)
                    .open(object)
                    .map(|reader| Box::new(reader) as Self::Reader)
            }
            Config::Existing { profile } => {
                let mut cache =
                    existing::ReadCache::new(profile).map_err(|_| SourceError::Invalid)?;
                cache
                    .open(object)
                    .map(|reader| Box::new(reader) as Self::Reader)
            }
        }
    }
}

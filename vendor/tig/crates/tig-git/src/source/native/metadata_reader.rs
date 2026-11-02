use std::io::{Read, Seek};
use tig_assets::revisions::{Error, Reader};
use tig_core::Object;
use tig_store::existing::{ByteSource, Error as StoreError, Profile, ReadCache};

pub struct CacheMetadataReader {
    cache: Option<ReadCache>,
    failure: Option<String>,
}
impl CacheMetadataReader {
    pub fn new(profile: Option<Profile>) -> Result<Self, StoreError> {
        Ok(Self {
            cache: profile.map(ReadCache::new).transpose()?,
            failure: None,
        })
    }
    pub fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}
impl Reader for CacheMetadataReader {
    fn read(&mut self, object: &Object, maximum: usize) -> Result<Vec<u8>, Error> {
        let maximum = maximum.min(16 * 1024 * 1024);
        if object.bytes > maximum as u64 {
            return Err(Error::Budget);
        }
        let Some(cache) = &mut self.cache else {
            self.failure = Some("history requires explicit byte_source profile".into());
            return Err(Error::Missing);
        };
        let mut held = match cache.open_verified(object) {
            Ok(held) => held,
            Err(StoreError::Budget) => return Err(Error::Budget),
            Err(StoreError::Integrity) => return Err(Error::Pin),
            Err(StoreError::Missing) => return Err(Error::Missing),
            Err(error) => {
                self.failure = Some(format!("history byte source: {error}"));
                return Err(Error::Missing);
            }
        };
        let mut raw = Vec::new();
        if held.rewind().is_err() || held.take(maximum as u64 + 1).read_to_end(&mut raw).is_err() {
            self.failure = Some("cannot read verified history metadata".into());
            return Err(Error::Pin);
        }
        Ok(raw)
    }
}

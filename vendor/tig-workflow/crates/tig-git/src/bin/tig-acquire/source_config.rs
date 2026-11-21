use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tig_core::{Digest, Object};
use tig_store::{Request, existing};

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Config {
    Native {
        root: PathBuf,
        place: Digest,
        publication: Request,
        max_object_bytes: u64,
    },
    Existing {
        profile: existing::Profile,
    },
}
impl Config {
    pub fn validate(&self, object: &Object) -> Result<(), String> {
        match self {
            Self::Native {
                root,
                publication,
                max_object_bytes,
                ..
            } => {
                if !root.is_absolute()
                    || publication.object != *object
                    || *max_object_bytes > tig_store::local::MAX_CONFIGURED_OBJECT_BYTES
                {
                    return Err("invalid native source binding or object limit".into());
                }
            }
            Self::Existing { profile } => {
                // Existing construction validates lexical bounds/configuration only.
                existing::ReadCache::new(profile.clone())
                    .map_err(|_| "invalid existing source profile")?;
            }
        }
        Ok(())
    }
}

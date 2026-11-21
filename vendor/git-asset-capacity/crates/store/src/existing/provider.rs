//! Explicit provider locations through the original guarded byte reader.
use super::*;

/// Private physical suffix issued by an admitted provider, never a logical ID.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ProviderPath(String);
impl ProviderPath {
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty() || value.len() > 4096 || value.split('/').count() > 128 {
            return Err(Error::Invalid);
        }
        for part in value.split('/') {
            if part.eq_ignore_ascii_case(".git") || part.eq_ignore_ascii_case(".tig") {
                continue;
            }
            git_asset_core::LogicalPath::parse(part).map_err(|_| Error::Invalid)?;
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for ProviderPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProviderPath(<private>)")
    }
}
impl<'de> Deserialize<'de> for ProviderPath {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
impl ReadCache {
    /// No IO or layout enrollment. Budgets are shared across explicit calls.
    pub fn provider(required: RequiredProtection, limits: Limits) -> Result<Self> {
        Self::configured(
            Profile {
                places: vec![],
                required,
                limits,
            },
            false,
        )
    }
    /// Open exactly one provider location, without layout guessing or fallback.
    /// Only provider-only instances accept this call; place_index is zero.
    pub fn open_provider_file(
        &mut self,
        root: &Path,
        path: &ProviderPath,
        object: &Object,
    ) -> Result<VerifiedRead> {
        if !self.profile.places.is_empty() {
            return Err(Error::Unsupported);
        }
        root_spelling(root)?;
        let permit = self.admit(object)?;
        let strict = self.profile.required == RequiredProtection::PreventMutation;
        self.open_path(root.join(path.as_str()), object, strict)?
            .map(|opened| opened.finish(object, 0, strict, permit))
            .ok_or(Error::Missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cache(requests: u32, readers: u32) -> ReadCache {
        ReadCache::provider(
            RequiredProtection::CooperativeImmutable,
            Limits {
                requests,
                live_readers: readers,
                total_bytes: 64,
                object_bytes: 64,
                ..Limits::default()
            },
        )
        .unwrap()
    }
    #[test]
    fn provider_paths_admit_private_namespaces_but_reject_escapes() {
        for good in [".git/annex/objects/key", ".tig/bytes", "annex/objects/key"] {
            assert!(ProviderPath::parse(good).is_ok());
        }
        for bad in [
            "", "/x", "../x", "a/../x", "a//b", "a\\b", "C:/x", "a:stream", "CON", "x.", "x ", ".",
        ] {
            assert!(ProviderPath::parse(bad).is_err(), "{bad}");
        }
        let p = ProviderPath::parse("private-name").unwrap();
        assert!(!format!("{p:?}").contains("private-name"));
    }
    #[test]
    fn exact_provider_bytes_share_request_and_reader_budgets() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join(".git/data"), b"bytes").unwrap();
        let path = ProviderPath::parse(".git/data").unwrap();
        let object = Object::from_bytes(b"bytes");
        let mut c = cache(3, 1);
        let mut held = c.open_provider_file(root.path(), &path, &object).unwrap();
        let mut data = vec![];
        held.read_to_end(&mut data).unwrap();
        assert_eq!(data, b"bytes");
        assert!(matches!(
            c.open_provider_file(root.path(), &path, &object),
            Err(Error::Budget)
        ));
        drop(held);
        assert_eq!(c.active_readers(), 0);
        assert!(c.open_provider_file(root.path(), &path, &object).is_ok());
        assert_eq!(c.requests(), 3);
        assert!(matches!(
            c.open_provider_file(root.path(), &path, &object),
            Err(Error::Budget)
        ));
        assert!(matches!(c.open_verified(&object), Err(Error::Unsupported)));
    }
    #[test]
    fn corrupt_and_missing_provider_bytes_are_not_layout_fallbacks() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("chosen"), b"wrong").unwrap();
        let mut c = cache(3, 2);
        let object = Object::from_bytes(b"right");
        assert!(matches!(
            c.open_provider_file(
                root.path(),
                &ProviderPath::parse("chosen").unwrap(),
                &object
            ),
            Err(Error::Integrity)
        ));
        assert!(matches!(
            c.open_provider_file(
                root.path(),
                &ProviderPath::parse("absent").unwrap(),
                &object
            ),
            Err(Error::Missing)
        ));
        let mut legacy = ReadCache::new(Profile {
            places: vec![Place {
                root: root.path().into(),
                layout: Layout::Flat,
            }],
            required: RequiredProtection::CooperativeImmutable,
            limits: Limits::default(),
        })
        .unwrap();
        assert!(matches!(
            legacy.open_provider_file(
                root.path(),
                &ProviderPath::parse("chosen").unwrap(),
                &object
            ),
            Err(Error::Unsupported)
        ));
        assert!(
            ReadCache::new(Profile {
                places: vec![],
                required: RequiredProtection::CooperativeImmutable,
                limits: Limits::default()
            })
            .is_err()
        );
    }
}

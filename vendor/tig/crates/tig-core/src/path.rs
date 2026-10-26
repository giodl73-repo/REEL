//! Exact UTF-8 logical paths, independent of provider-owned physical paths.
use crate::identity;
use serde::{Deserialize, Deserializer, Serialize, de};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct LogicalPath(String);

impl LogicalPath {
    pub fn parse(value: &str) -> identity::Result<Self> {
        if value.is_empty() || value.len() > 1024 || value.split('/').count() > 32 {
            return Err(identity::Error("logical path budget exceeded".into()));
        }
        for segment in value.split('/') {
            if segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.eq_ignore_ascii_case(".git")
                || segment.eq_ignore_ascii_case(".tig")
                || segment.ends_with(['.', ' '])
                || segment
                    .chars()
                    .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
            {
                return Err(identity::Error("invalid logical path component".into()));
            }
            let stem = segment.split('.').next().unwrap_or_default();
            let upper = stem.to_ascii_uppercase();
            if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ["COM", "LPT"].iter().any(|prefix| {
                    upper
                        .strip_prefix(prefix)
                        .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
                })
            {
                return Err(identity::Error("reserved logical path component".into()));
            }
        }
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl<'de> Deserialize<'de> for LogicalPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

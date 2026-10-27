//! A retained authoring request, never Git execution or selection during rendering.
use super::*;
use tig_store::existing::{Profile as ByteProfile, ReadCache, RequiredProtection};
pub(super) const FILE: &str = "git-proposal.json";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Handoff {
    git_executable: PathBuf,
    repository: PathBuf,
    base: String,
    identity: Author,
    message: String,
    paths: BTreeMap<String, String>,
    protection: RequiredProtection,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Author {
    name: String,
    email: String,
    unix_seconds: u64,
}
impl Handoff {
    pub fn validate(&self, specs: &[records::Spec]) -> Result<()> {
        let token = |s: &str| {
            !s.trim().is_empty()
                && s.len() <= 256
                && !s.chars().any(|c| c.is_control() || c == '<' || c == '>')
        };
        tig_git::source::GitId::parse(&self.base)?;
        if !self.git_executable.is_absolute()
            || !self.repository.is_absolute()
            || !token(&self.identity.name)
            || !token(&self.identity.email)
            || !self.identity.email.is_ascii()
            || !self.identity.email.contains('@')
            || self.identity.email.chars().any(char::is_whitespace)
            || self.identity.unix_seconds > 253_402_300_799
            || self.message.trim().is_empty()
            || self.message.len() > 4096
            || self.message.contains('\0')
        {
            bail!("invalid native Git handoff identity or destination");
        }
        let ids: BTreeSet<_> = specs.iter().map(|s| s.template.id.as_str()).collect();
        if ids.is_empty()
            || ids.len() != self.paths.len()
            || self.paths.keys().any(|id| !ids.contains(id.as_str()))
        {
            bail!("native Git handoff must map every candidate asset exactly once");
        }
        let mut paths = BTreeSet::new();
        let mut ancestors = BTreeSet::from([String::new()]);
        for path in self.paths.values() {
            if !paths.insert(LogicalPath::parse(path)?) {
                bail!("duplicate native Git handoff path");
            }
            for (offset, _) in path.match_indices('/') {
                ancestors.insert(path[..offset].to_owned());
            }
        }
        if ancestors.len() > 128 {
            bail!("native Git handoff tree budget exceeded");
        }
        Ok(())
    }
    pub fn profile<'a>(
        &self,
        records: impl IntoIterator<Item = (&'a str, &'a Object, &'a Object)>,
        roots: &[PathBuf],
    ) -> Result<Vec<u8>> {
        let byte_source = json!({"places":roots.iter().map(|root|json!({"root":root,"layout":"reel-cas"})).collect::<Vec<_>>(),"required":self.protection,
            "limits":{"object_bytes":65536,"total_bytes":8388608,"requests":128,"live_readers":128,"verify_millis":30000}});
        // Validate the existing reader's enrollment syntax without opening roots.
        ReadCache::new(serde_json::from_value::<ByteProfile>(byte_source.clone())?)?;
        let mut seen = BTreeSet::new();
        let mut changes = Vec::new();
        for (id, expected, object) in records {
            let path = self
                .paths
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("foreign native Git handoff asset"))?;
            if !seen.insert(id) {
                bail!("duplicate native Git handoff asset");
            }
            changes.push(json!({"path":path,"expected":expected,"object":object}));
        }
        if changes.is_empty() || changes.len() != self.paths.len() {
            bail!("incomplete native Git handoff");
        }
        let bytes = identity::canonical_json(
            &json!({"schema":"tig.git-proposal-profile.experimental-1",
            "git_executable":self.git_executable,"repository":self.repository,"base":self.base,
            "identity":self.identity,"message":self.message,"byte_source":byte_source,"changes":changes}),
        )?;
        if bytes.len() > 65536 {
            bail!("native Git handoff profile exceeds budget");
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Handoff, records::Spec, PathBuf) {
        let root = std::env::temp_dir().join("reel-handoff-no-initialization");
        let h = serde_json::from_value(json!({"git_executable":root.join("git.exe"),"repository":root.join("repo"),"base":"1".repeat(40),
            "identity":{"name":"Author","email":"author@example.invalid","unix_seconds":1700000000},"message":"new record",
            "paths":{"delivery/main":"episodes/01/es.tig"},"protection":"cooperative-immutable"})).unwrap();
        let spec = records::Spec {
            output: "master.mkv".into(),
            template: tig_assets::graph::Reference {
                id: "delivery/main".into(),
                object: Object::from_bytes(b"old"),
            },
            selector: tig_assets::graph::recipe::produced::Selector::Selected {},
            stream_class: None,
        };
        (h, spec, root)
    }
    #[test]
    fn handoff_requires_exact_asset_path_mapping_and_bounded_git_metadata() {
        let (mut h, s, _) = fixture();
        assert!(h.validate(std::slice::from_ref(&s)).is_ok());
        assert!(h.validate(&[]).is_err());
        h.paths.insert("foreign".into(), "foreign.tig".into());
        assert!(h.validate(std::slice::from_ref(&s)).is_err());
        h.paths.remove("foreign");
        h.paths
            .insert("delivery/main".into(), "../escape.tig".into());
        assert!(h.validate(std::slice::from_ref(&s)).is_err());
        h.paths
            .insert("delivery/main".into(), "episodes/01/es.tig".into());
        let mut second = s.clone();
        second.template.id = "delivery/second".into();
        h.paths
            .insert(second.template.id.clone(), "episodes/01/es.tig".into());
        assert!(h.validate(&[s.clone(), second]).is_err());
        h.paths.remove("delivery/second");
        h.identity.name = "bad\nidentity".into();
        assert!(h.validate(std::slice::from_ref(&s)).is_err());
        h.identity.name = "Author".into();
        h.message = "x".repeat(4097);
        assert!(h.validate(&[s]).is_err());
    }
    #[test]
    fn handoff_refuses_excessive_git_tree_rewrites_before_rendering() {
        let (mut h, s, _) = fixture();
        h.paths.clear();
        let specs: Vec<_> = (0..5)
            .map(|i| {
                let mut spec = s.clone();
                spec.template.id = format!("delivery/{i}");
                let path = format!("tree{i}/{}/record.tig", vec!["a"; 30].join("/"));
                assert!(LogicalPath::parse(&path).is_ok());
                h.paths.insert(spec.template.id.clone(), path);
                spec
            })
            .collect();
        assert!(
            h.validate(&specs)
                .unwrap_err()
                .to_string()
                .contains("tree budget")
        );
    }
    #[test]
    fn handoff_profile_uses_exact_pins_and_enrolled_roots_without_git_access() {
        let (h, s, root) = fixture();
        let new = Object::from_bytes(b"new");
        let records = [(s.template.id.as_str(), &s.template.object, &new)];
        let bytes = h.profile(records, std::slice::from_ref(&root)).unwrap();
        let value = identity::parse_json(&bytes).unwrap();
        assert_eq!(value["changes"][0]["expected"], json!(s.template.object));
        assert_eq!(value["changes"][0]["object"], json!(new));
        assert_eq!(value["changes"][0]["path"], "episodes/01/es.tig");
        assert_eq!(
            value["byte_source"]["places"][0],
            json!({"root":root,"layout":"reel-cas"})
        );
        assert!(h.profile(records, &[]).is_err());
        assert!(
            h.profile([("foreign", &s.template.object, &new)], &[root])
                .is_err()
        );
    }
}

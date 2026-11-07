//! Opt-in named metadata traversal. Associations are not production or custody claims.
use super::*;
use std::io::{Read, Seek};
use std::time::{Duration, Instant};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub max_documents: u32,
    pub max_document_bytes: u64,
    pub max_total_bytes: u64,
    pub max_references: usize,
    pub max_depth: usize,
    #[serde(default = "default_loaded_scopes")]
    pub max_loaded_scopes: usize,
    pub adapters: Vec<Adapter>,
}
fn default_loaded_scopes() -> usize {
    16
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Profile {
        serde_json::from_value(json!({"max_documents":16,"max_document_bytes":1048576,"max_total_bytes":4194304,"max_references":100,"max_depth":4,"adapters":[{"id":"bundle","schema":"fixture.bundle.v1","objects":[{"pointer":"/assets/*"}],"documents":[{"selector":{"pointer":"/child","optional":true},"adapter":"bundle"}]}]})).unwrap()
    }
    fn doc(value: Value) -> (Object, Vec<u8>) {
        let raw = serde_json::to_vec(&value).unwrap();
        (Object::from_bytes(&raw), raw)
    }
    fn request(objects: &[Object]) -> Request {
        Request {
            project: "fixture".into(),
            commit: "a".repeat(40),
            collection: "works".into(),
            work: "demo".into(),
            basis: Object::from_bytes(b"basis"),
            profile: profile(),
            roots: objects
                .iter()
                .enumerate()
                .map(|(i, o)| Root {
                    asset: format!("asset/{i}"),
                    slot: "selected".into(),
                    object: o.clone(),
                    adapter: "bundle".into(),
                })
                .collect(),
            source: ByteProfile {
                places: vec![],
                required: tig_store::existing::RequiredProtection::CooperativeImmutable,
                limits: Default::default(),
            },
        }
    }
    fn run(r: &Request, docs: &[(Object, Vec<u8>)]) -> Result<Loaded> {
        expand(r, |o| {
            docs.iter()
                .find(|(p, _)| p == o)
                .map(|(_, v)| v.clone())
                .ok_or(Problem::Missing)
        })
    }
    #[test]
    fn shared_child_provenance_and_exact_paths_do_not_read_leaf_bytes() {
        let a = Object::from_bytes(b"leaf a");
        let b = Object::from_bytes(b"leaf b");
        let child = doc(json!({"schema":"fixture.bundle.v1","assets":{"b":b}}));
        let first = doc(
            json!({"schema":"fixture.bundle.v1","assets":{"a":a},"child":child.0,"url":"https://never.invalid","cache_uri":"C:/never"}),
        );
        let second =
            doc(json!({"schema":"fixture.bundle.v1","assets":{"alias":a},"child":child.0}));
        let req = request(&[first.0.clone(), second.0.clone()]);
        let docs = [first, second, child];
        let mut calls = vec![];
        let result = expand(&req, |o| {
            calls.push(o.clone());
            docs.iter()
                .find(|(p, _)| p == o)
                .map(|(_, v)| v.clone())
                .ok_or(Problem::Missing)
        })
        .unwrap();
        assert_eq!(calls.len(), 3);
        assert!(!calls.contains(&a));
        assert!(!calls.contains(&b));
        assert_eq!(result.summary["successful_roots"], 2);
        assert_eq!(result.summary["unique_nested_objects"], 3);
        assert_eq!(result.refs.len(), 6);
        assert_eq!(result.refs.iter().filter(|r| r.object == b).count(), 2);
        assert!(result.refs.iter().any(|r| r.pointer == "/assets/a"));
    }
    #[test]
    fn schema_corruption_missing_and_bad_descriptors_remain_unknown() {
        for value in [
            json!({"schema":"wrong","assets":{}}),
            json!({"schema":"fixture.bundle.v1","assets":{"a":{"sha256":"bad","bytes":2}}}),
            json!({"schema":"fixture.bundle.v1","assets":{"a":{"sha256":"a".repeat(64),"bytes":-1}}}),
            json!({"schema":"fixture.bundle.v1","assets":{"a":{"sha256":"a".repeat(64),"bytes":1.5}}}),
        ] {
            let d = doc(value);
            let result = run(&request(std::slice::from_ref(&d.0)), &[d]).unwrap();
            assert!(result.refs.is_empty());
            assert_eq!(result.summary["failures"].as_array().unwrap().len(), 1);
        }
        let missing = request(&[Object::from_bytes(b"missing")]);
        assert_eq!(run(&missing, &[]).unwrap().summary["successful_roots"], 0);
        let corrupt = expand(&missing, |_| Ok(b"corrupt".to_vec())).unwrap();
        assert_eq!(
            corrupt.summary["failures"][0]["reason"],
            "metadata integrity refused"
        );
        let raw = br#"{"schema":"fixture.bundle.v1","assets":{},"assets":{}}"#.to_vec();
        let pin = Object::from_bytes(&raw);
        assert_eq!(
            run(&request(std::slice::from_ref(&pin)), &[(pin, raw)])
                .unwrap()
                .summary["successful_roots"],
            0
        );
    }
    #[test]
    fn failed_root_discards_partial_associations_but_other_roots_survive() {
        let leaf = Object::from_bytes(b"leaf");
        let missing = Object::from_bytes(b"missing");
        let a = doc(json!({"schema":"fixture.bundle.v1","assets":{"good":leaf},"child":missing}));
        let b = doc(json!({"schema":"fixture.bundle.v1","assets":{"good":leaf}}));
        let req = request(&[a.0.clone(), b.0.clone()]);
        let result = run(&req, &[a, b]).unwrap();
        assert_eq!(result.refs.len(), 1);
        assert_eq!(result.refs.iter().next().unwrap().asset, "asset/1");
        assert_eq!(result.summary["successful_roots"], 1);
    }
    #[test]
    fn document_byte_depth_reference_and_failed_attempt_budgets_are_hard() {
        let leaf = Object::from_bytes(b"leaf");
        let d = doc(json!({"schema":"fixture.bundle.v1","assets":{"a":leaf,"b":leaf}}));
        let mut req = request(std::slice::from_ref(&d.0));
        req.profile.max_references = 1;
        assert!(run(&req, std::slice::from_ref(&d)).is_err());
        req.profile = profile();
        req.profile.max_document_bytes = d.0.bytes - 1;
        let mut calls = 0;
        assert!(
            expand(&req, |_| {
                calls += 1;
                Ok(d.1.clone())
            })
            .is_err()
        );
        assert_eq!(calls, 0);
        let mut missing = request(&[Object::from_bytes(b"a"), Object::from_bytes(b"b")]);
        missing.profile.max_documents = 1;
        let mut calls = 0;
        assert!(
            expand(&missing, |_| {
                calls += 1;
                Err(Problem::Missing)
            })
            .is_err()
        );
        assert_eq!(calls, 1);
        let child = doc(json!({"schema":"fixture.bundle.v1","assets":{}}));
        let parent = doc(json!({"schema":"fixture.bundle.v1","assets":{},"child":child.0}));
        let root = doc(json!({"schema":"fixture.bundle.v1","assets":{},"child":parent.0}));
        let mut req = request(std::slice::from_ref(&root.0));
        req.profile.max_depth = 1;
        assert!(run(&req, &[root, parent, child]).is_err());
    }
    #[test]
    fn escaped_map_and_array_selectors_are_explicit_and_optional() {
        let leaf = Object::from_bytes(b"leaf");
        let d = doc(
            json!({"schema":"fixture.bundle.v1","assets":{"a/b~c":leaf},"items":[{"object":leaf}]}),
        );
        let mut req = request(std::slice::from_ref(&d.0));
        req.profile.adapters[0].objects.push(Selector {
            pointer: "/items/*/object".into(),
            optional: false,
        });
        let result = run(&req, std::slice::from_ref(&d)).unwrap();
        assert_eq!(result.refs.len(), 2);
        assert!(result.refs.iter().any(|r| r.pointer == "/assets/a~1b~0c"));
        req.profile.adapters[0].objects[0].pointer = "/absent".into();
        assert_eq!(
            run(&req, std::slice::from_ref(&d)).unwrap().summary["successful_roots"],
            0
        );
        req.profile.adapters[0].objects[0].optional = true;
        assert_eq!(run(&req, &[d]).unwrap().summary["successful_roots"], 1);
    }
    #[test]
    fn large_metadata_and_unselected_floats_work_without_exposing_fields() {
        let d = doc(
            json!({"schema":"fixture.bundle.v1","assets":{"leaf":Object::from_bytes(b"leaf")},"private":"NEVER_EXPOSE".repeat(7000),"gain":0.75}),
        );
        assert!(d.0.bytes > 65536);
        let result = run(&request(std::slice::from_ref(&d.0)), &[d]).unwrap();
        assert_eq!(result.summary["successful_roots"], 1);
        assert!(
            !serde_json::to_string(&result.summary)
                .unwrap()
                .contains("NEVER_EXPOSE")
        );
    }
    #[test]
    fn conflicting_sizes_and_invalid_profiles_refuse() {
        assert_eq!(profile().max_loaded_scopes, 16);
        for limit in [0, 257] {
            let mut p = profile();
            p.max_loaded_scopes = limit;
            assert!(validate(&p).is_err());
        }
        let d = doc(
            json!({"schema":"fixture.bundle.v1","assets":{"a":{"sha256":"a".repeat(64),"bytes":2},"b":{"sha256":"a".repeat(64),"bytes":3}}}),
        );
        assert!(run(&request(std::slice::from_ref(&d.0)), &[d]).is_err());
        let mut p = profile();
        p.adapters[0].objects[0].pointer = "/bad~2".into();
        assert!(validate(&p).is_err());
        p = profile();
        p.adapters[0].documents[0].adapter = "unknown".into();
        assert!(validate(&p).is_err());
        p = profile();
        p.max_depth = 100;
        assert!(validate(&p).is_err());
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    pub id: String,
    pub schema: String,
    #[serde(default)]
    pub roots: Vec<RootRule>,
    pub objects: Vec<Selector>,
    #[serde(default)]
    pub documents: Vec<Child>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RootRule {
    pub definition: String,
    pub asset_pattern: String,
    pub slot_pattern: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub pointer: String,
    #[serde(default)]
    pub optional: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Child {
    pub selector: Selector,
    pub adapter: String,
}
fn pointer(p: &str) -> bool {
    !p.is_empty()
        && p.starts_with('/')
        && p.len() <= 512
        && p.split('/').count() <= 17
        && p.split('/').skip(1).all(|s| {
            let mut it = s.chars();
            while let Some(c) = it.next() {
                if c == '~' && !matches!(it.next(), Some('0' | '1')) {
                    return false;
                }
            }
            true
        })
}
pub fn validate(p: &Profile) -> Result<()> {
    if p.max_loaded_scopes == 0 || p.max_loaded_scopes > 256 {
        return Err("Invalid loaded scope budget".into());
    }
    if p.max_documents == 0
        || p.max_documents > 2048
        || p.max_document_bytes == 0
        || p.max_document_bytes > 4 * 1024 * 1024
        || p.max_total_bytes < p.max_document_bytes
        || p.max_total_bytes > 64 * 1024 * 1024
        || p.max_references == 0
        || p.max_references > 100_000
        || p.max_depth == 0
        || p.max_depth > 16
        || p.adapters.is_empty()
        || p.adapters.len() > 32
    {
        return Err("Invalid nested metadata budgets".into());
    }
    let ids: BTreeSet<_> = p.adapters.iter().map(|a| a.id.as_str()).collect();
    if ids.len() != p.adapters.len() {
        return Err("Duplicate nested adapter".into());
    }
    for a in &p.adapters {
        if !token(&a.id)
            || a.schema.is_empty()
            || a.schema.len() > 128
            || a.roots.len() > 16
            || a.objects.is_empty()
            || a.objects.len() > 32
            || a.documents.len() > 32
            || a.objects
                .iter()
                .chain(a.documents.iter().map(|c| &c.selector))
                .any(|s| !pointer(&s.pointer))
            || a.documents
                .iter()
                .any(|c| !ids.contains(c.adapter.as_str()))
        {
            return Err("Invalid nested metadata adapter".into());
        }
        for r in &a.roots {
            if r.definition.is_empty() || r.definition.len() > 256 {
                return Err("Invalid nested root definition".into());
            }
            for s in [&r.asset_pattern, &r.slot_pattern] {
                if !s.starts_with('^') || s.len() > 512 {
                    return Err("Invalid nested root selector".into());
                }
                regex::RegexBuilder::new(s)
                    .size_limit(1024 * 1024)
                    .build()
                    .map_err(|_| "Invalid nested root expression")?;
            }
        }
    }
    Ok(())
}
#[derive(Clone, Serialize)]
pub struct Root {
    pub asset: String,
    pub slot: String,
    pub object: Object,
    pub adapter: String,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Association {
    pub asset: String,
    pub root_slot: String,
    pub root: Object,
    pub object: Object,
    pub document: Object,
    pub adapter: String,
    pub pointer: String,
}
pub struct Request {
    project: String,
    commit: String,
    collection: String,
    work: String,
    basis: Object,
    profile: Profile,
    roots: Vec<Root>,
    source: ByteProfile,
}
pub struct Loaded {
    pub(super) basis: Object,
    pub(super) content: Object,
    pub(super) refs: BTreeSet<Association>,
    pub(super) summary: Value,
}
// Asset-side JSON is larger than identity records and may contain unrelated
// floating-point fields. Reject duplicate keys without canonicalizing those fields.
struct Document(Value);
impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Document;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("metadata JSON")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Document, E> {
                Ok(Document(Value::Null))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Document, E> {
                Ok(Document(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Document, E> {
                Ok(Document(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Document, E> {
                Ok(Document(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Document, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Document(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Document, E> {
                Ok(Document(v.into()))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Document, A::Error> {
                let mut out = vec![];
                while let Some(Document(v)) = a.next_element()? {
                    out.push(v);
                }
                Ok(Document(Value::Array(out)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Document, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if out.contains_key(&k) {
                        return Err(serde::de::Error::custom("duplicate metadata key"));
                    }
                    let Document(v) = a.next_value()?;
                    out.insert(k, v);
                }
                Ok(Document(Value::Object(out)))
            }
        }
        d.deserialize_any(Visitor)
    }
}
#[derive(Clone, Copy, Debug)]
enum Problem {
    Budget,
    Missing,
    Integrity,
    Shape,
    Schema,
    Cycle,
}
impl Problem {
    fn code(self) -> &'static str {
        match self {
            Self::Budget => "metadata budget exceeded",
            Self::Missing => "metadata unavailable",
            Self::Integrity => "metadata integrity refused",
            Self::Shape => "descriptor or selector shape refused",
            Self::Schema => "document schema refused",
            Self::Cycle => "document cycle refused",
        }
    }
}
type NResult<T> = std::result::Result<T, Problem>;
fn selected<'a>(
    v: &'a Value,
    s: &Selector,
    steps: &mut usize,
    max: usize,
) -> NResult<Vec<(String, &'a Value)>> {
    let mut values = vec![(String::new(), v)];
    for token in s.pointer.split('/').skip(1) {
        let key = token.replace("~1", "/").replace("~0", "~");
        let mut next = vec![];
        for (path, v) in values {
            *steps = steps.checked_add(1).ok_or(Problem::Budget)?;
            if *steps > max {
                return Err(Problem::Budget);
            }
            if token == "*" {
                let entries: Vec<(String, &Value)> = match v {
                    Value::Object(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
                    Value::Array(a) => a
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (i.to_string(), v))
                        .collect(),
                    _ => return Err(Problem::Shape),
                };
                for (key, v) in entries {
                    next.push((
                        format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                        v,
                    ));
                }
            } else if let Some(v) = v.pointer(&format!("/{token}")) {
                next.push((
                    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                    v,
                ));
            } else if !s.optional {
                return Err(Problem::Shape);
            }
            if next.len() > max {
                return Err(Problem::Budget);
            }
        }
        values = next;
    }
    Ok(values)
}
fn descriptor(v: &Value) -> NResult<Object> {
    serde_json::from_value(json!({"sha256":v.get("sha256").ok_or(Problem::Shape)?,"bytes":v.get("bytes").ok_or(Problem::Shape)?})).map_err(|_|Problem::Shape)
}
struct Walk<'a, F> {
    profile: &'a Profile,
    read: F,
    documents: BTreeMap<Object, std::sync::Arc<Value>>,
    attempts: u32,
    bytes: u64,
    steps: usize,
    references: usize,
    nodes: usize,
    start: Instant,
}
impl<F: FnMut(&Object) -> NResult<Vec<u8>>> Walk<'_, F> {
    fn visit(
        &mut self,
        root: &Root,
        object: &Object,
        adapter: &str,
        depth: usize,
        active: &mut BTreeSet<(Object, String)>,
        refs: &mut BTreeSet<Association>,
    ) -> NResult<()> {
        if depth > self.profile.max_depth || self.start.elapsed() > Duration::from_secs(60) {
            return Err(Problem::Budget);
        }
        if !active.insert((object.clone(), adapter.into())) {
            return Err(Problem::Cycle);
        }
        let a = self
            .profile
            .adapters
            .iter()
            .find(|a| a.id == adapter)
            .ok_or(Problem::Schema)?
            .clone();
        if !self.documents.contains_key(object) {
            if object.bytes > self.profile.max_document_bytes
                || self.attempts >= self.profile.max_documents
                || self
                    .bytes
                    .checked_add(object.bytes)
                    .is_none_or(|n| n > self.profile.max_total_bytes)
            {
                return Err(Problem::Budget);
            }
            // Charge even unavailable/corrupt named attempts, not only successful parses.
            self.bytes += object.bytes;
            self.attempts += 1;
            let raw = (self.read)(object)?;
            if Object::from_bytes(&raw) != *object {
                return Err(Problem::Integrity);
            }
            let Document(value) = serde_json::from_slice(&raw).map_err(|_| Problem::Shape)?;
            let mut pending = vec![&value];
            while let Some(v) = pending.pop() {
                self.nodes += 1;
                if self.nodes + pending.len() > 500_000 {
                    return Err(Problem::Budget);
                }
                match v {
                    Value::Array(a) => pending.extend(a),
                    Value::Object(m) => pending.extend(m.values()),
                    _ => {}
                }
            }
            self.documents
                .insert(object.clone(), std::sync::Arc::new(value));
        }
        let doc = self.documents.get(object).ok_or(Problem::Missing)?.clone();
        if doc.get("schema").and_then(Value::as_str) != Some(a.schema.as_str()) {
            return Err(Problem::Schema);
        }
        for (s, child) in a.objects.iter().map(|s| (s, None)).chain(
            a.documents
                .iter()
                .map(|c| (&c.selector, Some(c.adapter.as_str()))),
        ) {
            for (path, v) in selected(&doc, s, &mut self.steps, self.profile.max_references * 32)? {
                self.references += 1;
                if self.references > self.profile.max_references || path.len() > 1024 {
                    return Err(Problem::Budget);
                }
                let o = descriptor(v)?;
                refs.insert(Association {
                    asset: root.asset.clone(),
                    root_slot: root.slot.clone(),
                    root: root.object.clone(),
                    object: o.clone(),
                    document: object.clone(),
                    adapter: a.id.clone(),
                    pointer: path,
                });
                if let Some(adapter) = child {
                    self.visit(root, &o, adapter, depth + 1, active, refs)?;
                }
            }
        }
        active.remove(&(object.clone(), adapter.into()));
        Ok(())
    }
}
fn expand<F: FnMut(&Object) -> NResult<Vec<u8>>>(request: &Request, read: F) -> Result<Loaded> {
    let mut walk = Walk {
        profile: &request.profile,
        read,
        documents: BTreeMap::new(),
        attempts: 0,
        bytes: 0,
        steps: 0,
        references: 0,
        nodes: 0,
        start: Instant::now(),
    };
    let mut refs = BTreeSet::new();
    let mut failures = vec![];
    let mut success = 0;
    for root in &request.roots {
        let mut candidate = BTreeSet::new();
        match walk.visit(
            root,
            &root.object,
            &root.adapter,
            0,
            &mut BTreeSet::new(),
            &mut candidate,
        ) {
            Ok(()) => {
                refs.extend(candidate);
                success += 1;
            }
            Err(Problem::Budget) => {
                return Err("Nested metadata budget exceeded; previous view preserved".into());
            }
            Err(e) => failures.push(
                json!({"asset":root.asset,"slot":root.slot,"object":root.object,"reason":e.code()}),
            ),
        }
    }
    let mut sizes = BTreeMap::new();
    for r in &refs {
        if sizes
            .insert(r.object.sha256.clone(), r.object.bytes)
            .is_some_and(|n| n != r.object.bytes)
        {
            return Err("Conflicting nested object sizes; previous view preserved".into());
        }
    }
    let content = Object::from_bytes(
        &serde_json::to_vec(&refs).map_err(|_| "Nested content encoding refused")?,
    );
    let summary = json!({"basis":request.basis,"content":content,"roots":request.roots.len(),"successful_roots":success,"failures":failures,"documents_read":walk.documents.len(),"read_attempts":walk.attempts,"metadata_bytes":walk.bytes,"associations":refs.len(),"unique_nested_objects":sizes.len(),"complete_dependency_coverage":false,"removal_authorized":false});
    Ok(Loaded {
        basis: request.basis.clone(),
        content,
        refs,
        summary,
    })
}
pub fn execute(request: &Request) -> Result<Loaded> {
    let mut profile = request.source.clone();
    profile.limits.object_bytes = profile
        .limits
        .object_bytes
        .min(request.profile.max_document_bytes);
    profile.limits.total_bytes = profile
        .limits
        .total_bytes
        .min(request.profile.max_total_bytes);
    profile.limits.requests = profile.limits.requests.min(request.profile.max_documents);
    let mut reader = ReadCache::new(profile).map_err(|_| "Nested metadata reader refused")?;
    expand(request, |object| {
        let mut held = reader.open_verified(object).map_err(|e| match e {
            tig_store::existing::Error::Budget => Problem::Budget,
            tig_store::existing::Error::Integrity => Problem::Integrity,
            _ => Problem::Missing,
        })?;
        held.rewind().map_err(|_| Problem::Integrity)?;
        let mut raw = vec![];
        held.take(object.bytes + 1)
            .read_to_end(&mut raw)
            .map_err(|_| Problem::Integrity)?;
        Ok(raw)
    })
}
fn roots_for(p: &Project, w: &works::Work, profile: &Profile) -> Result<Vec<Root>> {
    let mut rules = vec![];
    for a in &profile.adapters {
        for r in &a.roots {
            rules.push((
                a.id.as_str(),
                r.definition.as_str(),
                Regex::new(&r.asset_pattern).map_err(|_| "Invalid root selector")?,
                Regex::new(&r.slot_pattern).map_err(|_| "Invalid root selector")?,
            ));
        }
    }
    let mut roots = vec![];
    for i in &w.members {
        let c = &p.cards[*i];
        let a = p
            .snapshot
            .resolve_id(&c.id)
            .map_err(|_| "Record unavailable")?;
        for (slot, o) in a
            .selected()
            .ok()
            .map(|o| ("selected", o))
            .into_iter()
            .chain(
                a.stream_bindings()
                    .iter()
                    .map(|s| (s.slot.as_str(), &s.object)),
            )
        {
            let matched: BTreeSet<_> = rules
                .iter()
                .filter(|(_, d, id, s)| {
                    *d == c.definition && id.is_match(&c.id) && s.is_match(slot)
                })
                .map(|(a, _, _, _)| *a)
                .collect();
            if matched.len() > 1 {
                return Err("Ambiguous nested root adapters".into());
            }
            if let Some(adapter) = matched.first() {
                roots.push(Root {
                    asset: c.id.clone(),
                    slot: slot.into(),
                    object: o.clone(),
                    adapter: (*adapter).into(),
                });
            }
            if roots.len() > 2048 {
                return Err("Nested root budget exceeded".into());
            }
        }
    }
    Ok(roots)
}
pub(super) fn root_count(p: &Project, w: &works::Work) -> Result<Option<usize>> {
    p.enrollment
        .storage
        .as_ref()
        .and_then(|s| s.nested_content.as_ref())
        .map(|n| roots_for(p, w, n).map(|roots| roots.len()))
        .transpose()
}
impl Catalogue {
    pub(super) fn version_metadata(
        &self,
        scope: (&str, &str, &str, &str),
        pair: [(&str, &Object); 2],
    ) -> Result<Option<Request>> {
        let (project, commit, collection, work) = scope;
        let [(from, old), (to, new)] = pair;
        let p = self.projects.get(project).ok_or("Unknown project")?;
        let w = p
            .works
            .iter()
            .find(|w| w.collection == collection && w.id == work)
            .ok_or("Unknown work")?;
        let Some(profile) = p
            .enrollment
            .storage
            .as_ref()
            .and_then(|s| s.nested_content.as_ref())
        else {
            return Ok(None);
        };
        let roots = roots_for(p, w, profile)?;
        let adapters: BTreeSet<_> = roots
            .iter()
            .filter(|r| r.asset == w.root)
            .map(|r| r.adapter.clone())
            .collect();
        if adapters.is_empty() {
            return Ok(None);
        }
        if adapters.len() != 1 {
            return Err("Ambiguous version metadata adapter".into());
        }
        let adapter = adapters.first().ok_or("No version adapter")?.clone();
        let mut request = self.nested_request(project, commit, collection, work)?;
        request.roots = vec![
            Root {
                asset: w.root.clone(),
                slot: from.into(),
                object: old.clone(),
                adapter: adapter.clone(),
            },
            Root {
                asset: w.root.clone(),
                slot: to.into(),
                object: new.clone(),
                adapter,
            },
        ];
        request.basis=Object::from_bytes(&serde_json::to_vec(&json!({"commit":commit,"collection":collection,"work":work,"profile":request.profile,"roots":request.roots})).map_err(|_|"Version metadata basis refused")?);
        Ok(Some(request))
    }
    pub fn nested_request(
        &self,
        project: &str,
        commit: &str,
        collection: &str,
        work: &str,
    ) -> Result<Request> {
        let p = self.projects.get(project).ok_or("Unknown project")?;
        p.check_snapshot(commit)?;
        let storage = p
            .enrollment
            .storage
            .as_ref()
            .ok_or("No storage enrollment")?;
        let profile = storage
            .nested_content
            .clone()
            .ok_or("No nested content adapters enrolled")?;
        validate(&profile)?;
        let w = p
            .works
            .iter()
            .find(|w| w.collection == collection && w.id == work)
            .ok_or("Unknown work scope")?;
        let roots = roots_for(p, w, &profile)?;
        if roots.is_empty() || roots.len() > 2048 {
            return Err("No admitted nested roots or root budget exceeded".into());
        }
        let mut source = p
            .profile
            .byte_source
            .clone()
            .ok_or("No metadata byte sources")?;
        source.places = source
            .places
            .into_iter()
            .enumerate()
            .filter(|(i, _)| {
                storage
                    .places
                    .iter()
                    .any(|p| p.place_index == *i && p.live_probe)
            })
            .map(|(_, p)| p)
            .collect();
        let basis=Object::from_bytes(&serde_json::to_vec(&json!({"commit":commit,"collection":collection,"work":work,"profile":profile,"roots":roots})).map_err(|_|"Nested basis refused")?);
        Ok(Request {
            project: project.into(),
            commit: commit.into(),
            collection: collection.into(),
            work: work.into(),
            basis,
            profile,
            roots,
            source,
        })
    }
    pub fn adopt_nested(&mut self, request: &Request, loaded: Loaded) -> Result<Value> {
        let current = self.nested_request(
            &request.project,
            &request.commit,
            &request.collection,
            &request.work,
        )?;
        if current.basis != request.basis || loaded.basis != request.basis {
            return Err("Nested snapshot/profile changed; result refused".into());
        }
        let p = self
            .projects
            .get_mut(&request.project)
            .ok_or("Unknown project")?;
        let key = format!("{}/{}", request.collection, request.work);
        if p.nested.contains_key(&key)
            && loaded.summary["failures"]
                .as_array()
                .is_none_or(|v| !v.is_empty())
        {
            return Err("Nested refresh incomplete; previous view preserved".into());
        }
        if !p.nested.contains_key(&key) && p.nested.len() >= request.profile.max_loaded_scopes {
            return Err("Loaded nested scope budget exceeded".into());
        }
        if p.nested
            .iter()
            .filter(|(k, _)| *k != &key)
            .map(|(_, v)| v.refs.len())
            .sum::<usize>()
            + loaded.refs.len()
            > 500_000
        {
            return Err("Loaded association budget exceeded".into());
        }
        let summary = loaded.summary.clone();
        p.nested.insert(key, loaded);
        Ok(summary)
    }
}

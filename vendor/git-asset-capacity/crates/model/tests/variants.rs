//! Behavioral adaptation of Art119c6644's14 MIT variant cases; native wire differs.
use git_asset_core::{Digest, Object};
use git_asset_model::{graph::Reference, native::Instance, resolve::Resolved, variants::*};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn parameters(p: &[(&str, &str)]) -> Parameters {
    p.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
struct Fixture {
    space: Vec<u8>,
    index: Value,
    record: Value,
    definition: Vec<u8>,
    schema: Vec<u8>,
}
impl Fixture {
    fn new(uses: &[&str], entries: Value) -> Self {
        let space=serde_json::to_vec(&json!({"schema":"tig.variant-space.experimental-1","axes":{"language":["en","es"],"style":["theatrical","realistic"]}})).unwrap();
        let schema = br#"{"type":"object"}"#.to_vec();
        let definition=serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/example","authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
        let def = Reference {
            id: "types/example".into(),
            object: Object::from_bytes(&definition),
        };
        let index = json!({"schema":"tig.variant-index.experimental-1","asset":"cue/1","definition":def,"space":Object::from_bytes(&space),"semantics":Digest::blob(b"selection/v1"),"uses":uses,"entries":entries});
        let mut pin = json!(def.object);
        pin["id"] = json!(def.id);
        let record = json!({"schema":"tig.instance.design-1","id":"cue/1","definition":pin,"authored":{},"selected":null,
            "streams":[{"slot":"en","role":"audio","group":"language","object":Object::from_bytes(b"en1")},{"slot":"es","role":"audio","group":"language","object":Object::from_bytes(b"es1")}],"status":{},"metadata":{}});
        Self {
            space,
            index,
            record,
            definition,
            schema,
        }
    }
    fn cue() -> Self {
        Self::new(
            &["language"],
            json!([{"parameters":{"language":"en"},"slot":"en"},{"parameters":{"language":"es"},"slot":"es"}]),
        )
    }
    fn admission(&self) -> Admission {
        Admission {
            definition: Reference {
                id: "types/example".into(),
                object: Object::from_bytes(&self.definition),
            },
            space: Object::from_bytes(&self.space),
            semantics: Digest::blob(b"selection/v1"),
            uses: BTreeSet::from(["language".into()]),
        }
    }
    fn resolved(&self) -> Resolved {
        let raw = serde_json::to_vec(&self.index).unwrap();
        let mut record = self.record.clone();
        record["selected"] = json!(Object::from_bytes(&raw));
        let bound = Instance::parse(&serde_json::to_vec(&record).unwrap())
            .unwrap()
            .bind(&self.definition, &self.schema)
            .unwrap();
        Resolved::from_bound(Digest::blob(b"checkpoint"), &bound)
    }
    fn run(&self, context: &[(&str, &str)]) -> Result<Selection, Error> {
        let raw = serde_json::to_vec(&self.index).unwrap();
        let space = Space::parse(&Object::from_bytes(&self.space), &self.space)?;
        let index = Index::parse(&Object::from_bytes(&raw), &raw)?;
        resolve(
            &self.resolved(),
            &self.admission(),
            &space,
            &index,
            &parameters(context),
        )
    }
}
#[test]
fn variants_one_cue_resolves_both_languages() {
    let f = Fixture::cue();
    let en = f.run(&[("language", "en")]).unwrap();
    let es = f.run(&[("language", "es")]).unwrap();
    assert_eq!(en.record(), es.record());
    assert_ne!(en.object(), es.object());
    assert_ne!(en.subject(), es.subject());
}
#[test]
fn variants_visual_context_does_not_change_audio() {
    let f = Fixture::cue();
    let a = f
        .run(&[("language", "en"), ("style", "theatrical")])
        .unwrap();
    let b = f
        .run(&[("language", "en"), ("style", "realistic")])
        .unwrap();
    assert_eq!(a.subject(), b.subject());
    assert_ne!(a.context(), b.context());
    assert_eq!(a.parameters(), &parameters(&[("language", "en")]));
}
#[test]
fn variants_cel_style_ignores_language_under_admission() {
    let f = Fixture::new(
        &["style"],
        json!([{"parameters":{"style":"theatrical"},"slot":"en"},{"parameters":{"style":"realistic"},"slot":"es"}]),
    );
    let raw = serde_json::to_vec(&f.index).unwrap();
    let i = Index::parse(&Object::from_bytes(&raw), &raw).unwrap();
    let s = Space::parse(&Object::from_bytes(&f.space), &f.space).unwrap();
    let mut a = f.admission();
    a.uses = BTreeSet::from(["style".into()]);
    let x = resolve(
        &f.resolved(),
        &a,
        &s,
        &i,
        &parameters(&[("style", "theatrical"), ("language", "en")]),
    )
    .unwrap();
    let y = resolve(
        &f.resolved(),
        &a,
        &s,
        &i,
        &parameters(&[("style", "theatrical"), ("language", "es")]),
    )
    .unwrap();
    assert_eq!(x.subject(), y.subject());
}
#[test]
fn variants_sparse_matrix_never_invents_combinations() {
    let mut f = Fixture::cue();
    f.index["entries"] = json!([{"parameters":{"language":"en"},"slot":"en"}]);
    assert!(f.run(&[("language", "en")]).is_ok());
    assert!(f.run(&[("language", "es")]).is_err());
}
#[test]
fn variants_required_language_has_no_fallback() {
    let f = Fixture::cue();
    assert!(f.run(&[]).is_err());
    assert!(f.run(&[("style", "realistic")]).is_err());
}
#[test]
fn variants_unknown_unconsumed_axes_and_values_refuse() {
    let f = Fixture::cue();
    for p in [
        vec![("langauge", "en")],
        vec![("language", "fr")],
        vec![("language", "en"), ("style", "typo")],
    ] {
        assert!(f.run(&p).is_err());
    }
}
#[test]
fn variants_unrelated_stream_change_keeps_subject_with_new_record_provenance() {
    let mut f = Fixture::cue();
    let old = f.run(&[("language", "en")]).unwrap();
    f.record["streams"][1]["object"] = json!(Object::from_bytes(b"es2"));
    let new = f.run(&[("language", "en")]).unwrap();
    assert_eq!(old.subject(), new.subject());
    assert_ne!(old.record(), new.record());
    assert_eq!(old.index(), new.index());
}
#[test]
fn variants_owner_definition_and_selected_document_are_exact() {
    let mut f = Fixture::cue();
    f.index["asset"] = json!("other");
    assert!(f.run(&[("language", "en")]).is_err());
    f.index["asset"] = json!("cue/1");
    f.index["definition"]["object"] = json!(Object::from_bytes(b"wrong"));
    assert!(f.run(&[("language", "en")]).is_err());
    let f = Fixture::cue();
    let raw = serde_json::to_vec(&f.index).unwrap();
    let index = Index::parse(&Object::from_bytes(&raw), &raw).unwrap();
    let space = Space::parse(&Object::from_bytes(&f.space), &f.space).unwrap();
    let mut record = f.record.clone();
    record["selected"] = json!(Object::from_bytes(b"wrong index"));
    let bound = Instance::parse(&serde_json::to_vec(&record).unwrap())
        .unwrap()
        .bind(&f.definition, &f.schema)
        .unwrap();
    let r = Resolved::from_bound(Digest::blob(b"checkpoint"), &bound);
    assert!(
        resolve(
            &r,
            &f.admission(),
            &space,
            &index,
            &parameters(&[("language", "en")])
        )
        .is_err()
    );
}
#[test]
fn variants_space_change_requires_new_exact_admission() {
    let mut f = Fixture::cue();
    let mut value: Value = serde_json::from_slice(&f.space).unwrap();
    value["axes"]["language"] = json!(["en", "es", "fr"]);
    f.space = serde_json::to_vec(&value).unwrap();
    assert!(f.run(&[("language", "en")]).is_err());
}
#[test]
fn variants_duplicate_malformed_or_unadmitted_contexts_refuse() {
    let mut missing = Fixture::cue();
    missing.index["entries"][1]["slot"] = json!("missing");
    assert!(missing.run(&[("language", "en")]).is_err());
    for entries in [
        json!([{"parameters":{"language":"en"},"slot":"en"},{"parameters":{"language":"en"},"slot":"es"}]),
        json!([{"parameters":{},"slot":"en"}]),
        json!([{"parameters":{"language":"en","style":"theatrical"},"slot":"en"}]),
        json!([{"parameters":{"language":"typo"},"slot":"en"}]),
    ] {
        let mut f = Fixture::cue();
        f.index["entries"] = entries;
        assert!(f.run(&[("language", "en")]).is_err());
    }
    let mut f = Fixture::cue();
    f.index["uses"] = json!([]);
    f.index["entries"] = json!([{"parameters":{},"slot":"es"}]);
    assert!(f.run(&[("language", "en")]).is_err());
}
#[test]
fn variants_neutral_selection_is_explicit_and_zero_axis() {
    let f = Fixture::new(&[], json!([{"parameters":{},"slot":"en"}]));
    let raw = serde_json::to_vec(&f.index).unwrap();
    let index = Index::parse(&Object::from_bytes(&raw), &raw).unwrap();
    let space = Space::parse(&Object::from_bytes(&f.space), &f.space).unwrap();
    let mut admission = f.admission();
    admission.uses.clear();
    let s = resolve(
        &f.resolved(),
        &admission,
        &space,
        &index,
        &parameters(&[("language", "en")]),
    )
    .unwrap();
    assert!(s.parameters().is_empty());
}
#[test]
fn variants_semantics_slot_role_group_changes_affect_subject() {
    let mut f = Fixture::cue();
    let old = f.run(&[("language", "en")]).unwrap();
    f.record["streams"][0]["role"] = json!("other-role");
    assert_ne!(
        old.subject(),
        f.run(&[("language", "en")]).unwrap().subject()
    );
    let mut f = Fixture::cue();
    f.index["semantics"] = json!(Digest::blob(b"new semantics"));
    assert!(f.run(&[("language", "en")]).is_err());
    let raw = serde_json::to_vec(&f.index).unwrap();
    let index = Index::parse(&Object::from_bytes(&raw), &raw).unwrap();
    let space = Space::parse(&Object::from_bytes(&f.space), &f.space).unwrap();
    let mut admission = f.admission();
    admission.semantics = Digest::blob(b"new semantics");
    let new = resolve(
        &f.resolved(),
        &admission,
        &space,
        &index,
        &parameters(&[("language", "en")]),
    )
    .unwrap();
    assert_ne!(old.subject(), new.subject());
}
#[test]
fn variants_strict_wire_pins_sets_and_budgets_refuse() {
    for value in [
        json!({"schema":"future","axes":{}}),
        json!({"schema":"tig.variant-space.experimental-1","axes":{},"extra":true}),
        json!({"schema":"tig.variant-space.experimental-1","axes":{"language":["en","en"]}}),
        json!({"schema":"tig.variant-space.experimental-1","axes":{"invalid axis":["en"]}}),
    ] {
        let raw = serde_json::to_vec(&value).unwrap();
        assert!(Space::parse(&Object::from_bytes(&raw), &raw).is_err());
    }
    assert!(Space::parse(&Object::from_bytes(b"other"), b"{}").is_err());
    let mut f = Fixture::cue();
    f.index["uses"] = json!(["language", "language"]);
    assert!(f.run(&[("language", "en")]).is_err());
    let mut f = Fixture::cue();
    f.index["entries"] = json!(vec![f.index["entries"][0].clone(); 257]);
    assert!(f.run(&[("language", "en")]).is_err());
    let raw = br#"{"schema":"x","schema":"y"}"#;
    assert!(Space::parse(&Object::from_bytes(raw), raw).is_err());
}
#[test]
fn variants_selection_does_not_grant_freshness_approval_or_custody_and_debug_is_private() {
    let mut f = Fixture::cue();
    let old = f.run(&[("language", "en")]).unwrap();
    f.record["metadata"] = json!({"private":"DO NOT PRINT"});
    f.record["status"] = json!({"private":"DO NOT PRINT"});
    let new = f.run(&[("language", "en")]).unwrap();
    assert_eq!(old.subject(), new.subject());
    assert_ne!(old.record(), new.record());
    assert!(!new.complete_semantic_coverage());
    assert_eq!(new.controls().len(), 3);
    let debug = format!("{new:?}");
    assert!(
        !debug.contains("cue/1") && !debug.contains("language") && !debug.contains("DO NOT PRINT")
    );
    assert_eq!(new.role(), "audio");
    assert_eq!(new.group(), "language");
    assert_eq!(new.slot(), "en");
    assert_eq!(new.checkpoint(), &Digest::blob(b"checkpoint"));
}

#[test]
fn variants_project_space_extension_reuses_old_neutral_bindings_but_never_falls_back_recordings() {
    let f = Fixture::new(
        &["style"],
        json!([{"parameters":{"style":"theatrical"},"slot":"en"}]),
    );
    let raw = serde_json::to_vec(&f.index).unwrap();
    let index = Index::parse(&Object::from_bytes(&raw), &raw).unwrap();
    let space = Space::parse(&Object::from_bytes(&f.space), &f.space).unwrap();
    let mut admission = f.admission();
    admission.uses = BTreeSet::from(["style".into()]);
    let original = resolve(
        &f.resolved(),
        &admission,
        &space,
        &index,
        &parameters(&[("language", "en"), ("style", "theatrical")]),
    )
    .unwrap();
    let mut current: Value = serde_json::from_slice(&f.space).unwrap();
    current["axes"]["language"] = json!(["en", "es", "fr"]);
    let current = serde_json::to_vec(&current).unwrap();
    let project = Space::parse(&Object::from_bytes(&current), &current).unwrap();
    let context = project
        .admit(&parameters(&[("language", "fr"), ("style", "theatrical")]))
        .unwrap();
    let extended = resolve_in_context(
        &f.resolved(),
        &admission,
        &space,
        &index,
        &context,
        project.object(),
    )
    .unwrap();
    assert_eq!(original.subject(), extended.subject());
    assert_eq!(original.record(), extended.record());
    assert_eq!(original.index(), extended.index());
    assert_ne!(original.context_space(), extended.context_space());
    assert_eq!(extended.controls().len(), 4);
    let restored = Context::restore(context.object(), context.bytes(), &project).unwrap();
    assert_eq!(restored.parameters(), context.parameters());
    assert!(Context::restore(&Object::from_bytes(b"wrong"), context.bytes(), &project).is_err());
    assert!(
        resolve_in_context(
            &f.resolved(),
            &admission,
            &space,
            &index,
            &context,
            space.object()
        )
        .is_err()
    );
    assert!(project.admit(&parameters(&[("langauge", "fr")])).is_err());
    let cue = Fixture::cue();
    let raw = serde_json::to_vec(&cue.index).unwrap();
    let cue_index = Index::parse(&Object::from_bytes(&raw), &raw).unwrap();
    assert!(
        resolve_in_context(
            &cue.resolved(),
            &cue.admission(),
            &space,
            &cue_index,
            &context,
            project.object()
        )
        .is_err()
    );
}

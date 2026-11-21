use super::*;
use git_asset::source::native::placement::{Demand, Error, Policy, Witnesses};
use git_asset_core::{Claim, Digest, Placement, View};
#[path = "placement_archive_cases.rs"]
mod archive_cases;

struct Expander {
    objects: BTreeMap<Object, BTreeSet<Object>>,
    reads: BTreeMap<Object, usize>,
}
impl Witnesses for Expander {
    fn expand(&mut self, witness: &Object, maximum: usize) -> Result<BTreeSet<Object>, Error> {
        assert!(maximum <= 65_536);
        *self.reads.entry(witness.clone()).or_default() += 1;
        self.objects.get(witness).cloned().ok_or(Error::Unknown)
    }
}
fn policy() -> Policy {
    Policy {
        generation: Digest::blob(b"admitted synthetic policy v1"),
        working: BTreeSet::from(["working".into()]),
        retained: BTreeSet::from(["archive".into()]),
        maximum_objects: 4096,
        maximum_logical_bytes: 1_000_000,
    }
}
fn inspection(f: &HistoryFixture) -> lifecycle::Inspection {
    let source = capture(&reader(f.source.root()), f.commit.clone());
    let mut bytes = MemoryReader {
        objects: f
            .objects
            .iter()
            .map(|o| {
                (
                    o.clone(),
                    fs::read(f.layout.object_path(f.cache.path(), &o.sha256)).unwrap(),
                )
            })
            .collect(),
        calls: 0,
    };
    source
        .inspect_history(
            "private-asset-id",
            Dialect::Draft202012,
            revisions::Limits::default(),
            &mut bytes,
        )
        .unwrap()
}
fn expander(i: &lifecycle::Inspection) -> Expander {
    Expander {
        objects: i
            .revisions()
            .unwrap()
            .provenance()
            .iter()
            .chain(i.delivery().unwrap().opaque())
            .map(|o| (o.clone(), BTreeSet::new()))
            .collect(),
        reads: BTreeMap::new(),
    }
}
fn next(f: &mut HistoryFixture, select: bool) -> (Object, Object) {
    let subject = Object::from_bytes(b"new selected output");
    let witness = Object::from_bytes(b"new frozen context");
    let revision = cache_put(
        f.cache.path(),
        &f.layout,
        json!({"schema":"tig.asset-revision.experimental-1","asset":"private-asset-id",
            "revision":"run-b","parents":[f.revision],
            "representations":[{"key":"main","object":subject}],
            "provenance":{"kind":"imported","source":witness}}),
        &mut f.objects,
    );
    let raw = fs::read(f.source.root().join("records/history.asset")).unwrap();
    let record: Value = serde_json::from_slice(&raw).unwrap();
    let previous = record["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slot"] == "revision-history")
        .unwrap()["object"]
        .clone();
    let root = cache_put(
        f.cache.path(),
        &f.layout,
        json!({"schema":"tig.revision-history.experimental-1","asset":"private-asset-id",
            "coverage":"complete","parents":[previous],"revisions":[revision]}),
        &mut f.objects,
    );
    f.edit_record(|r| {
        for stream in r["streams"].as_array_mut().unwrap() {
            if stream["slot"] == "revision-history" {
                stream["object"] = json!(root);
            }
            if select && stream["slot"] == "selected-revision" {
                stream["object"] = json!(revision);
            }
        }
        if select {
            r["selected"] = json!(subject);
        }
    });
    (revision, witness)
}
fn demand(i: &lifecycle::Inspection, name: &str, e: &mut Expander) -> Demand {
    i.placement(name, &policy(), &BTreeSet::new(), e).unwrap()
}

#[test]
fn five_of_thousand_replacements_keep_shared_input_and_history_out_of_working_cache() {
    let mut f = HistoryFixture::new(Layout::ReelCas, false);
    let (_, witness) = next(&mut f, true);
    let i = inspection(&f);
    let mut e = expander(&i);
    let old_witness = Object::from_bytes(b"opaque historical witness");
    let old: Vec<_> = (0..1000)
        .map(|n| Object::from_bytes(format!("input-{n}").as_bytes()))
        .collect();
    let new: BTreeSet<_> = old
        .iter()
        .skip(5)
        .cloned()
        .chain((0..5).map(|n| Object::from_bytes(format!("replacement-{n}").as_bytes())))
        .collect();
    e.objects.insert(old_witness, old.iter().cloned().collect());
    e.objects.insert(witness, new.clone());
    let current = demand(&i, "main", &mut e);
    assert!(old.iter().all(|o| current.retained().contains(o)));
    assert!(new.iter().all(|o| current.working().contains(o)));
    assert!(old[..5].iter().all(|o| !current.working().contains(o)));
    assert!(current.working().is_subset(current.retained()));
    assert_eq!(current.source_checkpoint(), &i.summary().source_checkpoint);
    assert_eq!(current.record(), &i.summary().record);
    assert_eq!(current.policy(), &policy().generation);
    assert!(!current.authorizes_eviction() && !current.complete_claim_coverage());
    assert!(e.reads.values().all(|count| *count == 1));
    // A separate branch/job still needs one replaced input. No global count is inferred.
    let peer = Claim {
        id: "independent-job".into(),
        placements: BTreeSet::from([Placement {
            object: old[0].clone(),
            place: "working".into(),
        }]),
    };
    let observed: BTreeSet<_> = current
        .claim()
        .placements
        .iter()
        .cloned()
        .chain(old.iter().cloned().map(|object| Placement {
            object,
            place: "working".into(),
        }))
        .collect();
    let scoped = View::new(7, true, vec![current.claim().clone(), peer.clone()]).unwrap();
    let plan = scoped.plan(&observed).unwrap();
    let old_candidates: BTreeSet<_> = plan
        .release_candidates
        .iter()
        .filter(|p| old.contains(&p.object))
        .map(|p| p.object.clone())
        .collect();
    assert_eq!(old_candidates, old[1..5].iter().cloned().collect());
    let unknown = View::new(7, false, vec![current.claim().clone(), peer]).unwrap();
    assert!(
        unknown
            .plan(&observed)
            .unwrap()
            .release_candidates
            .is_empty()
    );
    let mut missing_cold = observed.clone();
    missing_cold.remove(&Placement {
        object: old[1].clone(),
        place: "archive".into(),
    });
    assert!(
        !scoped
            .plan(&missing_cold)
            .unwrap()
            .release_candidates
            .iter()
            .any(|p| p.object == old[1])
    );
}

#[test]
fn authorized_draft_is_exact_and_does_not_change_selected_revision_or_source() {
    let mut f = HistoryFixture::new(Layout::GitLfs, true);
    let (draft, _) = next(&mut f, false);
    let i = inspection(&f);
    let mut e = expander(&i);
    let before = fs::read(f.source.root().join("records/history.asset")).unwrap();
    let a = demand(&i, "branch", &mut e);
    let b = i
        .placement(
            "branch",
            &policy(),
            &BTreeSet::from([draft.clone()]),
            &mut e,
        )
        .unwrap();
    let draft_payload = Object::from_bytes(b"new selected output");
    assert!(!a.working().contains(&draft_payload));
    assert!(a.retained().contains(&draft_payload));
    assert!(b.working().contains(&draft_payload));
    assert!(b.working().contains(&draft));
    assert_eq!(
        i.revisions().unwrap().selected_revision(),
        Some(&f.revision)
    );
    assert_eq!(
        fs::read(f.source.root().join("records/history.asset")).unwrap(),
        before
    );
    assert!(matches!(
        i.placement(
            "branch",
            &policy(),
            &BTreeSet::from([Object::from_bytes(b"foreign draft")]),
            &mut e
        ),
        Err(Error::Draft)
    ));
    assert_eq!(head(f.source.root()), f.commit);
}

#[test]
fn unresolved_old_or_delivery_witness_refuses_instead_of_losing_retained_closure() {
    let mut f = HistoryFixture::new(Layout::ReelCas, true);
    next(&mut f, true);
    let i = inspection(&f);
    for witness in i
        .revisions()
        .unwrap()
        .provenance()
        .iter()
        .chain(i.delivery().unwrap().opaque())
    {
        let mut e = expander(&i);
        e.objects.remove(witness);
        assert!(matches!(
            i.placement("main", &policy(), &BTreeSet::new(), &mut e),
            Err(Error::Unknown)
        ));
    }
}

#[test]
fn placement_bounds_size_conflicts_and_invalid_policy_refuse() {
    let f = HistoryFixture::new(Layout::ReelCas, true);
    let i = inspection(&f);
    let mut e = expander(&i);
    for p in [
        Policy {
            maximum_objects: 1,
            ..policy()
        },
        Policy {
            maximum_logical_bytes: 1,
            ..policy()
        },
        Policy {
            maximum_objects: 65_537,
            ..policy()
        },
        Policy {
            retained: BTreeSet::new(),
            ..policy()
        },
        Policy {
            working: BTreeSet::from(["../bad place".into()]),
            ..policy()
        },
    ] {
        assert!(i.placement("main", &p, &BTreeSet::new(), &mut e).is_err());
    }
    let mut wrong = i.summary().record.clone();
    wrong.bytes += 1;
    e.objects.insert(
        Object::from_bytes(b"opaque historical witness"),
        BTreeSet::from([wrong]),
    );
    assert!(matches!(
        i.placement("main", &policy(), &BTreeSet::new(), &mut e),
        Err(Error::Conflict)
    ));
}

#[test]
fn partial_history_is_unknown_even_when_selected_bytes_are_known() {
    let mut f = HistoryFixture::new(Layout::ReelCas, true);
    let root = cache_put(
        f.cache.path(),
        &f.layout,
        json!({"schema":"tig.revision-history.experimental-1","asset":"private-asset-id",
            "coverage":"partial","parents":[],"revisions":[f.revision]}),
        &mut f.objects,
    );
    f.edit_record(|r| {
        for stream in r["streams"].as_array_mut().unwrap() {
            if stream["slot"] == "revision-history" {
                stream["object"] = json!(root);
            }
        }
    });
    let i = inspection(&f);
    let mut e = expander(&i);
    assert!(matches!(
        i.placement("main", &policy(), &BTreeSet::new(), &mut e),
        Err(Error::Unknown)
    ));
    assert!(e.reads.is_empty());
}

#[test]
fn unclassified_selected_output_and_untracked_history_never_become_zero_working_demand() {
    for untracked in [false, true] {
        let mut f = HistoryFixture::new(Layout::ReelCas, false);
        f.edit_record(|r| {
            if untracked {
                r["metadata"] = json!({});
            } else {
                r["metadata"]["revisions"]
                    .as_object_mut()
                    .unwrap()
                    .remove("selection");
            }
        });
        let i = inspection(&f);
        let mut e = Expander {
            objects: BTreeMap::new(),
            reads: BTreeMap::new(),
        };
        assert!(i.selected_subject().is_some());
        assert!(matches!(
            i.placement("main", &policy(), &BTreeSet::new(), &mut e),
            Err(Error::Unknown)
        ));
        assert!(e.reads.is_empty());
    }
}

#[test]
fn cumulative_witness_reference_budget_counts_repeated_closures_not_only_unique_objects() {
    let mut f = HistoryFixture::new(Layout::ReelCas, true);
    let mut revisions = vec![f.revision.clone()];
    let mut witnesses = Vec::new();
    for n in 0..25 {
        let witness = Object::from_bytes(format!("retained witness {n}").as_bytes());
        revisions.push(cache_put(
            f.cache.path(),
            &f.layout,
            json!({"schema":"tig.asset-revision.experimental-1","asset":"private-asset-id",
                "revision":format!("retained-{n}"),"parents":[],
                "representations":[{"key":"main","object":Object::from_bytes(b"shared result")}],
                "provenance":{"kind":"imported","source":witness}}),
            &mut f.objects,
        ));
        witnesses.push(witness);
    }
    let root = cache_put(
        f.cache.path(),
        &f.layout,
        json!({"schema":"tig.revision-history.experimental-1","asset":"private-asset-id",
            "coverage":"complete","parents":[],"revisions":revisions}),
        &mut f.objects,
    );
    f.edit_record(|r| {
        for s in r["streams"].as_array_mut().unwrap() {
            if s["slot"] == "revision-history" {
                s["object"] = json!(root);
            }
        }
    });
    let i = inspection(&f);
    let mut e = expander(&i);
    let shared: BTreeSet<_> = (0..3000)
        .map(|n| Object::from_bytes(format!("shared-{n}").as_bytes()))
        .collect();
    for witness in witnesses {
        e.objects.insert(witness, shared.clone());
    }
    assert!(matches!(
        i.placement("main", &policy(), &BTreeSet::new(), &mut e),
        Err(Error::Budget)
    ));
    assert!(e.reads.len() < 26);
    assert!(e.reads.values().all(|count| *count == 1));
}

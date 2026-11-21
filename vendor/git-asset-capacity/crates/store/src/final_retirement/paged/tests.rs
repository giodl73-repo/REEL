use super::*;
use crate::{
    metadata::{self, MetadataStore},
    source_claims::{
        Command, Places,
        admission::{Policy, Select, Work as ProtectionWork, bulk as projection},
    },
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn source_profile(f: &Fixture) -> source_claims::subset::Enrollment {
    source_claims::subset::Enrollment {
        authority: f.e.authority.clone(),
        namespace: f.namespaces[0].identity().clone(),
        policy: f.roster.members[0].policy.clone(),
        owner: f.e.owner.clone(),
        classes: vec![Kind::Branch, Kind::Draft],
        limits: f.limits.source.clone(),
        writes: 65536,
        write_bytes: 32 * 1024 * 1024,
    }
}
#[test]
fn prepared_whole_block_reopens_and_actual_page_receipts_match_predictions() {
    use crate::source_claims::subset;
    for alias in [false, true] {
        let mut f = Fixture::new(17, 3, alias);
        let (_, manifest) = f.compile(Paging {
            max_bindings: 2,
            ..Paging::default()
        });
        let source = source_profile(&f);
        let before = f.namespaces[0].checkpoint();
        let prepared = preparation::prepare(
            &f.e,
            &manifest,
            std::slice::from_ref(&source),
            &f.roster,
            &mut f.namespaces,
            &mut f.metadata,
            &f.limits,
        )
        .unwrap();
        assert_eq!(f.namespaces[0].checkpoint(), before);
        let minimum = f.metadata.finish().unwrap();
        drop(f.metadata);
        f.metadata = MetadataStore::open(&f.temp.path().join("metadata"), &minimum).unwrap();
        let (h, steps) = preparation::read(&f.e, &prepared, &mut f.metadata, &f.limits).unwrap();
        assert_eq!(h.count, 34);
        assert_eq!(h.bindings, 51);
        assert_eq!(f.namespaces[0].checkpoint(), before);
        for (row, plan) in steps {
            let result =
                subset::apply(&source, &mut f.namespaces[0], &mut f.metadata, &plan).unwrap();
            assert_eq!(
                result.historical_retirement.unwrap().checkpoint,
                row.retirement_checkpoint
            );
        }
        assert!(
            f.namespaces[0]
                .source(&key("main"))
                .unwrap()
                .unwrap()
                .predecessors
                .is_empty()
        );
        let mut protection =
            ProtectionWork::new(&f.metadata, &f.limits.source, Instant::now()).unwrap();
        let candidates = [(f.object.sha256.clone(), f.object.clone())]
            .into_iter()
            .collect();
        let remaining =
            projection::remaining_selected(&mut f.namespaces[0], &[], &candidates, &mut protection)
                .unwrap();
        assert_eq!(!remaining.1.is_empty(), alias);
        // Persisted predictions remain inspectable after source effects. They
        // still do not claim that those effects happened or grant removal.
        assert_eq!(
            preparation::read(&f.e, &prepared, &mut f.metadata, &f.limits)
                .unwrap()
                .0
                .count,
            34
        );
    }
}
#[test]
fn stale_manifest_wrong_profiles_and_aggregate_budget_cannot_prepare_source_effects() {
    let mut f = Fixture::new(2, 3, false);
    let (_, manifest) = f.compile(Paging {
        max_bindings: 1,
        ..Paging::default()
    });
    let source = source_profile(&f);
    let before = f.namespaces[0].checkpoint();
    let mut wrong = source.clone();
    wrong.owner = key("foreign");
    assert!(
        preparation::prepare(
            &f.e,
            &manifest,
            &[wrong],
            &f.roster,
            &mut f.namespaces,
            &mut f.metadata,
            &f.limits
        )
        .is_err()
    );
    let mut small = f.limits.clone();
    small.source.reads = 1;
    assert!(matches!(
        preparation::prepare(
            &f.e,
            &manifest,
            std::slice::from_ref(&source),
            &f.roster,
            &mut f.namespaces,
            &mut f.metadata,
            &small
        ),
        Err(Error::Budget)
    ));
    assert_eq!(f.namespaces[0].checkpoint(), before);
    f.namespaces[0]
        .apply(
            key("new-source"),
            Command::Enroll {
                source: key("other"),
                owner: key("owner"),
                class: Kind::Job,
            },
            &f.metadata,
            &f.limits.source,
        )
        .unwrap();
    let changed = f.namespaces[0].checkpoint();
    assert!(matches!(
        preparation::prepare(
            &f.e,
            &manifest,
            &[source],
            &f.roster,
            &mut f.namespaces,
            &mut f.metadata,
            &f.limits
        ),
        Err(Error::Stale)
    ));
    assert_eq!(f.namespaces[0].checkpoint(), changed);
}

#[test]
fn prepared_readback_rejects_truncated_reordered_and_changed_original_pages() {
    let mut f = Fixture::new(2, 3, false);
    let (_, manifest) = f.compile(Paging {
        max_bindings: 2,
        ..Paging::default()
    });
    let source = source_profile(&f);
    let prepared = preparation::prepare(
        &f.e,
        &manifest,
        &[source],
        &f.roster,
        &mut f.namespaces,
        &mut f.metadata,
        &f.limits,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let (header, steps) = preparation::read(&f.e, &prepared, &mut f.metadata, &f.limits).unwrap();
    let before = f.namespaces[0].checkpoint();
    for mode in 0..5 {
        let mut work = Work::new(&mut f.metadata, &f.limits).unwrap();
        let mut h = header.clone();
        let mut index = work.entries(&h.steps, 4096).unwrap();
        match mode {
            0 => {
                index.pop_last();
            }
            1 => {
                let first = index.remove(&path("00000000").unwrap()).unwrap();
                let last = index.remove(&path("00000003").unwrap()).unwrap();
                index.insert(path("00000000").unwrap(), last);
                index.insert(path("00000003").unwrap(), first);
            }
            2 => {
                let mut row = steps[0].0.clone();
                row.bindings[0].object = Object::from_bytes(b"substituted");
                index.insert(path("00000000").unwrap(), work.doc(&row).unwrap());
            }
            3 => {
                let mut row = steps[0].0.clone();
                row.plan = steps[2].0.plan.clone();
                index.insert(path("00000000").unwrap(), work.doc(&row).unwrap());
            }
            _ => {
                let mut row = steps[0].0.clone();
                let mut value = serde_json::to_value(&steps[0].1).unwrap();
                value["summary"]["survivor_bindings"] = serde_json::json!(0);
                let raw = identity::canonical_json(&value).unwrap();
                let changed = source_claims::subset::Plan::from_json(&raw).unwrap();
                row.plan = work.doc(&changed).unwrap();
                index.insert(path("00000000").unwrap(), work.doc(&row).unwrap());
            }
        }
        h.steps = work.tree(&index, 4096).unwrap();
        let changed = work.doc(&h).unwrap();
        f.metadata.finish().unwrap();
        assert!(preparation::read(&f.e, &changed, &mut f.metadata, &f.limits).is_err());
    }
    assert_eq!(f.namespaces[0].checkpoint(), before);
}

struct PreparationDrift<'a> {
    nodes: &'a mut MetadataStore,
    peer: Namespace,
    limits: protection::Limits,
    changed: bool,
}
impl NodeStore for PreparationDrift<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.nodes.get(o)
    }
    fn put(&mut self, raw: &[u8]) -> TreeResult<Object> {
        if !self.changed
            && raw
                .windows(b"tig.subset-plan.design-2".len())
                .any(|v| v == b"tig.subset-plan.design-2")
        {
            self.peer
                .apply(
                    key("during-plan"),
                    Command::Enroll {
                        source: key("late-job"),
                        owner: key("owner"),
                        class: Kind::Job,
                    },
                    self.nodes,
                    &self.limits,
                )
                .unwrap();
            self.changed = true;
        }
        self.nodes.put(raw)
    }
}
#[test]
fn source_drift_during_plan_persistence_invalidates_the_whole_preparation() {
    let mut f = Fixture::new(2, 3, false);
    let (_, manifest) = f.compile(Paging {
        max_bindings: 2,
        ..Paging::default()
    });
    let profile = source_profile(&f);
    let before = f.namespaces[0].checkpoint();
    let peer = Namespace::open(&f.temp.path().join("ns"), &before).unwrap();
    let mut nodes = PreparationDrift {
        nodes: &mut f.metadata,
        peer,
        limits: f.limits.source.clone(),
        changed: false,
    };
    assert!(matches!(
        preparation::prepare(
            &f.e,
            &manifest,
            &[profile],
            &f.roster,
            &mut f.namespaces,
            &mut nodes,
            &f.limits
        ),
        Err(Error::Stale)
    ));
    assert!(nodes.changed);
    let source = f.namespaces[0].source(&key("main")).unwrap().unwrap();
    assert_eq!(source.predecessors.len(), 2);
    assert_eq!(
        f.namespaces[0].checkpoint().sequence(),
        before.sequence() + 1
    );
}

#[test]
fn interleaved_namespace_pages_keep_independent_checkpoint_sequences() {
    use crate::source_claims::subset;
    let mut f = Fixture::new(2, 3, false);
    let first = source_profile(&f);
    let original = f.namespaces[0]
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .predecessors[0]
        .selection
        .clone();
    let mut peer = Namespace::create(
        &f.temp.path().join("second"),
        key("second-project"),
        key("epoch"),
        f.namespaces[0].policy().clone(),
    )
    .unwrap();
    peer.apply(
        key("enroll"),
        Command::Enroll {
            source: key("main"),
            owner: key("owner"),
            class: Kind::Branch,
        },
        &f.metadata,
        &f.limits.source,
    )
    .unwrap();
    peer.select(
        &Select {
            prepare: key("prepare"),
            adopt: key("adopt"),
            source: key("main"),
            owner: key("owner"),
            expected_revision: 1,
            selection: original,
        },
        &f.metadata,
        &f.limits.source,
    )
    .unwrap();
    peer.apply(
        key("withdraw"),
        Command::Withdraw {
            source: key("main"),
            owner: key("owner"),
            expected_revision: 3,
        },
        &f.metadata,
        &f.limits.source,
    )
    .unwrap();
    f.selections.push(Selection {
        namespace: peer.identity().clone(),
        source: key("main"),
        predecessor_revision: 3,
        bindings: f.selections[0].bindings.clone(),
    });
    let second = subset::Enrollment {
        namespace: peer.identity().clone(),
        policy: hash(peer.policy()).unwrap(),
        ..first.clone()
    };
    f.namespaces.push(peer);
    f.roster = claims::Roster::flat(
        f.e.authority.clone(),
        f.e.epoch.clone(),
        Some(d("complete-two")),
        f.namespaces
            .iter()
            .map(|n| claims::NamespacePin::current(n).unwrap())
            .collect(),
        f.limits.source.clone(),
    )
    .unwrap();
    f.e.scope = f.roster.scope().unwrap();
    let mut profiles = vec![first, second];
    profiles.sort_by(|a, b| a.namespace.cmp(&b.namespace));
    f.namespaces.reverse();
    let (_, manifest) = f.compile(Paging {
        max_bindings: 2,
        ..Paging::default()
    });
    let prepared = preparation::prepare(
        &f.e,
        &manifest,
        &profiles,
        &f.roster,
        &mut f.namespaces,
        &mut f.metadata,
        &f.limits,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let (_, steps) = preparation::read(&f.e, &prepared, &mut f.metadata, &f.limits).unwrap();
    assert_eq!(steps.len(), 6);
    for (s, p) in steps {
        let profile = profiles
            .iter()
            .find(|p| p.namespace == s.origin.namespace)
            .unwrap();
        let ns = f
            .namespaces
            .iter_mut()
            .find(|n| n.identity() == &s.origin.namespace)
            .unwrap();
        let actual = subset::apply(profile, ns, &mut f.metadata, &p).unwrap();
        assert_eq!(
            actual.historical_retirement.unwrap().checkpoint,
            s.retirement_checkpoint
        );
    }
    for ns in &mut f.namespaces {
        assert!(
            ns.source(&key("main"))
                .unwrap()
                .unwrap()
                .predecessors
                .is_empty()
        );
    }
}
#[test]
fn independent_vectors_pin_all_new_record_frames_and_cursor_identity() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/vectors/paged-retirement.design-1.json"
    )))
    .unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let raw = case["canonical"].as_str().unwrap().as_bytes();
        let value = identity::parse_json(raw).unwrap();
        let expected: Digest = serde_json::from_value(case["id"].clone()).unwrap();
        assert_eq!(Digest::blob(raw).as_str(), case["sha256"].as_str().unwrap());
        match case["kind"].as_str().unwrap() {
            "enrollment" => {
                let v: Enrollment = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
                assert_eq!(v.id().unwrap(), expected);
            }
            "selection" => {
                let v: Selection = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
                assert_eq!(v.id().unwrap(), expected);
            }
            "origin" => {
                let v: Origin = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
                assert_eq!(v.id().unwrap(), expected);
            }
            "cohort" => {
                let v: Cohort = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
            }
            "request" => {
                let v: Request = serde_json::from_value(value).unwrap();
                assert_eq!(v.schema, REQUEST_SCHEMA);
                assert_eq!(encode(&v).unwrap(), raw);
            }
            "subject_held" | "subject_unprotected" => {
                let v: Subject = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
            }
            "manifest" => {
                let v: Header = serde_json::from_value(value).unwrap();
                assert_eq!(v.schema, MANIFEST_SCHEMA);
                v.basis.id().unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
            }
            "cursor" => {
                let v: Cursor = serde_json::from_value(value).unwrap();
                assert_eq!(encode(&v).unwrap(), raw);
            }
            "page" => {
                let p = Page {
                    schema: PAGE_SCHEMA,
                    manifest: serde_json::from_value(value["manifest"].clone()).unwrap(),
                    cohort: serde_json::from_value(value["cohort"].clone()).unwrap(),
                    bindings: serde_json::from_value(value["bindings"].clone()).unwrap(),
                    next: None,
                    historical_only: true,
                    authorizes_claim_release: false,
                    authorizes_removal: false,
                };
                assert_eq!(encode(&p).unwrap(), raw);
            }
            _ => panic!("unknown vector"),
        }
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    metadata: MetadataStore,
    namespaces: Vec<Namespace>,
    roster: claims::Roster,
    e: Enrollment,
    selections: Vec<Selection>,
    limits: Limits,
    object: Object,
}
impl Fixture {
    fn new(roots: usize, bindings: usize, alias: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let limits = Limits::default();
        let mut metadata = MetadataStore::create(
            &temp.path().join("metadata"),
            metadata::Limits {
                nodes: 100000,
                bytes: 64 * 1024 * 1024,
                database_bytes: 128 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let object = Object::from_bytes(b"shared");
        let picked: BTreeMap<_, _> = (0..bindings)
            .map(|i| (path(&format!("binding-{i:05}")).unwrap(), object.clone()))
            .collect();
        let selected = Tree::build(&picked, &mut metadata, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        let mut original = picked;
        if alias {
            original.insert(path("unselected-alias").unwrap(), object.clone());
        }
        let original = source_claims::Selection {
            root: Tree::build(&original, &mut metadata, TreeLimits::default())
                .unwrap()
                .0
                .root()
                .clone(),
            context: d("original"),
        };
        metadata.finish().unwrap();
        let places = Places {
            current: vec![key("F"), key("M")],
            pending: vec![key("F"), key("M")],
            predecessor: vec![key("D"), key("M")],
        };
        let policy = Policy {
            prefer_active: true,
            classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .map(|k| (k, places.clone()))
                .collect(),
        };
        let mut ns = Namespace::create(
            &temp.path().join("ns"),
            key("project"),
            key("epoch"),
            policy,
        )
        .unwrap();
        ns.apply(
            key("enroll"),
            Command::Enroll {
                source: key("main"),
                owner: key("owner"),
                class: Kind::Branch,
            },
            &metadata,
            &limits.source,
        )
        .unwrap();
        let mut selections = Vec::new();
        for i in 0..roots {
            let s = ns.source(&key("main")).unwrap().unwrap();
            ns.select(
                &Select {
                    prepare: key(&format!("prepare-{i}")),
                    adopt: key(&format!("adopt-{i}")),
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: s.revision,
                    selection: original.clone(),
                },
                &metadata,
                &limits.source,
            )
            .unwrap();
            let revision = ns.source(&key("main")).unwrap().unwrap().revision;
            ns.apply(
                key(&format!("withdraw-{i}")),
                Command::Withdraw {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: revision,
                },
                &metadata,
                &limits.source,
            )
            .unwrap();
            selections.push(Selection {
                namespace: ns.identity().clone(),
                source: key("main"),
                predecessor_revision: revision,
                bindings: selected.clone(),
            });
        }
        let roster = claims::Roster::flat(
            d("authority"),
            d("epoch"),
            Some(d("complete-synthetic-boundary")),
            vec![claims::NamespacePin::current(&ns).unwrap()],
            limits.source.clone(),
        )
        .unwrap();
        let e = Enrollment {
            authority: d("authority"),
            epoch: d("epoch"),
            owner: key("owner"),
            metadata: metadata.checkpoint().identity().clone(),
            scope: roster.scope().unwrap(),
            classes: [Kind::Branch, Kind::Draft].into_iter().collect(),
        };
        Self {
            temp,
            metadata,
            namespaces: vec![ns],
            roster,
            e,
            selections,
            limits,
            object,
        }
    }
    fn compile(&mut self, paging: Paging) -> (Object, Object) {
        let r = request(
            self.selections.clone(),
            paging,
            &mut self.metadata,
            &self.limits,
        )
        .unwrap();
        self.metadata.finish().unwrap();
        let manifest = build(
            &self.e,
            &r,
            &self.roster,
            &mut self.namespaces,
            &mut self.metadata,
            &self.limits,
        )
        .unwrap();
        self.metadata.finish().unwrap();
        (r, manifest)
    }
}
#[test]
fn whole_scope_beyond_sixteen_roots_has_stable_complete_pages_and_no_authority() {
    let mut f = Fixture::new(17, 3, false);
    let before = f.namespaces[0].checkpoint();
    let (_, manifest) = f.compile(Paging {
        max_bindings: 2,
        max_bytes: 8192,
    });
    let mut reader = f.metadata.finished_reader().unwrap();
    let inventory = inspect(&f.e, &manifest, &mut reader, &f.limits).unwrap();
    assert_eq!(
        inventory.summary(),
        &Summary {
            roots: 17,
            bindings: 51,
            unique_objects: 1,
            held_objects: 0,
            unique_bytes: 6
        }
    );
    assert!(!inventory.authorizes_claim_release() && !inventory.authorizes_removal());
    // The old bounded projection still holds the object through the seventeenth
    // root. The new whole-selection observation does not change that contract.
    let exclusions: Vec<_> = inventory
        .cohorts()
        .take(16)
        .map(|c| projection::Exclusion {
            source: c.origin.source.clone(),
            owner: c.origin.owner.clone(),
            predecessor: c.origin.predecessor_revision,
            original: c.origin.original.clone(),
            paths: (0..3)
                .map(|i| path(&format!("binding-{i:05}")).unwrap())
                .collect(),
        })
        .collect();
    let candidates = BTreeMap::from([(f.object.sha256.clone(), f.object.clone())]);
    let mut work = ProtectionWork::new(&reader, &f.limits.source, Instant::now()).unwrap();
    assert!(
        projection::remaining(&mut f.namespaces[0], &exclusions, &candidates, &mut work)
            .unwrap()
            .1
            .contains(&f.object.sha256)
    );
    let mut cursor = None;
    let mut seen = BTreeSet::new();
    let mut pages = 0;
    loop {
        let p = page(&f.e, &manifest, cursor.as_ref(), &mut reader, &f.limits)
            .unwrap()
            .unwrap();
        pages += 1;
        assert!(encode(&p).unwrap().len() <= 8192);
        assert!(p.historical_only && !p.authorizes_claim_release && !p.authorizes_removal);
        for b in &p.bindings {
            assert!(seen.insert((p.cohort.origin.id().unwrap(), b.path.clone())));
        }
        cursor = p.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen.len(), 51);
    assert_eq!(pages, 34);
    assert_eq!(before, f.namespaces[0].checkpoint());
}
#[test]
fn one_large_root_spans_pages_without_resetting_original_identity() {
    let mut f = Fixture::new(1, 1100, false);
    let (_, manifest) = f.compile(Paging {
        max_bindings: 128,
        max_bytes: 8192,
    });
    let minimum = f.metadata.checkpoint();
    drop(f.metadata);
    let mut reader =
        metadata::MetadataReader::open(&f.temp.path().join("metadata"), &minimum).unwrap();
    assert_eq!(
        inspect(&f.e, &manifest, &mut reader, &f.limits)
            .unwrap()
            .summary()
            .bindings,
        1100
    );
    let mut cursor = None;
    let mut paths = BTreeSet::new();
    let mut origin = None;
    let mut pages = 0;
    loop {
        let p = page(&f.e, &manifest, cursor.as_ref(), &mut reader, &f.limits)
            .unwrap()
            .unwrap();
        pages += 1;
        if let Some(old) = &origin {
            assert_eq!(old, &p.cohort.origin)
        } else {
            origin = Some(p.cohort.origin.clone())
        }
        assert!(encode(&p).unwrap().len() <= 8192);
        for b in p.bindings {
            assert!(paths.insert(b.path));
        }
        cursor = p.next;
        if cursor.is_none() {
            break;
        }
    }
    assert!(pages > 9);
    assert_eq!(paths.len(), 1100);
    assert_eq!(reader.checkpoint(), &minimum);
}
#[test]
fn unselected_aliases_current_jobs_and_retention_remain_protection() {
    let mut f = Fixture::new(17, 1, true);
    let (_, manifest) = f.compile(Paging::default());
    assert_eq!(
        inspect(
            &f.e,
            &manifest,
            &mut f.metadata.finished_reader().unwrap(),
            &f.limits
        )
        .unwrap()
        .summary()
        .held_objects,
        1
    );
    for class in [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention] {
        let mut f = Fixture::new(17, 1, false);
        let n = &mut f.namespaces[0];
        n.apply(
            key("foreign-enroll"),
            Command::Enroll {
                source: key("foreign"),
                owner: key("foreign-owner"),
                class,
            },
            &f.metadata,
            &f.limits.source,
        )
        .unwrap();
        let original = n.source(&key("main")).unwrap().unwrap().predecessors[0]
            .selection
            .clone();
        n.select(
            &Select {
                prepare: key("foreign-prepare"),
                adopt: key("foreign-adopt"),
                source: key("foreign"),
                owner: key("foreign-owner"),
                expected_revision: 1,
                selection: original,
            },
            &f.metadata,
            &f.limits.source,
        )
        .unwrap();
        let (_, manifest) = f.compile(Paging::default());
        assert_eq!(
            inspect(
                &f.e,
                &manifest,
                &mut f.metadata.finished_reader().unwrap(),
                &f.limits
            )
            .unwrap()
            .summary()
            .held_objects,
            1
        );
    }
}
#[test]
fn malformed_positions_duplicate_occurrences_and_small_budgets_refuse() {
    let mut f = Fixture::new(2, 3, false);
    let mut duplicate = f.selections.clone();
    duplicate.push(duplicate[0].clone());
    assert!(request(duplicate, Paging::default(), &mut f.metadata, &f.limits).is_err());
    // A failed constructor may stage private metadata; reopen rather than flush
    // another operation's pending writes implicitly.
    f.metadata =
        MetadataStore::open(&f.temp.path().join("metadata"), &f.metadata.checkpoint()).unwrap();
    let (_, manifest) = f.compile(Paging {
        max_bindings: 1,
        max_bytes: 8192,
    });
    let mut reader = f.metadata.finished_reader().unwrap();
    let first = page(&f.e, &manifest, None, &mut reader, &f.limits)
        .unwrap()
        .unwrap();
    let mut cursor = first.next.unwrap();
    cursor.manifest = d("wrong");
    assert!(page(&f.e, &manifest, Some(&cursor), &mut reader, &f.limits).is_err());
    cursor.manifest = manifest.sha256.clone();
    cursor.after = path("missing").unwrap();
    assert!(page(&f.e, &manifest, Some(&cursor), &mut reader, &f.limits).is_err());
    let mut limits = f.limits.clone();
    limits.source.reads = 1;
    assert!(inspect(&f.e, &manifest, &mut reader, &limits).is_err());
    limits = f.limits.clone();
    limits.writes = 0;
    assert!(
        request(
            f.selections.clone(),
            Paging::default(),
            &mut f.metadata,
            &limits
        )
        .is_err()
    );
    let mut e = f.e.clone();
    e.epoch = d("other");
    assert!(inspect(&e, &manifest, &mut reader, &f.limits).is_err());
}

struct Drift<'a> {
    nodes: &'a mut MetadataStore,
    trigger: Object,
    root: std::path::PathBuf,
    minimum: source_claims::Checkpoint,
    selection: source_claims::Selection,
    called: Cell<bool>,
    limits: protection::Limits,
}
impl NodeStore for Drift<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if o == &self.trigger && !self.called.replace(true) {
            let mut peer = Namespace::open(&self.root, &self.minimum).unwrap();
            let revision = peer.source(&key("main")).unwrap().unwrap().revision;
            let nodes = self.nodes.finished_reader().unwrap();
            let receipt = peer
                .apply(
                    key("transient-prepare"),
                    Command::Prepare {
                        source: key("main"),
                        owner: key("owner"),
                        expected_revision: revision,
                        selection: self.selection.clone(),
                    },
                    &nodes,
                    &self.limits,
                )
                .unwrap();
            peer.apply(
                key("transient-abort"),
                Command::Abort {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: revision + 1,
                    prepared: receipt.reference,
                },
                &nodes,
                &self.limits,
            )
            .unwrap();
        }
        self.nodes.get(o)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
        self.nodes.put(b)
    }
}
#[test]
fn actual_transient_admission_during_projection_invalidates_the_candidate() {
    let mut f = Fixture::new(17, 1, false);
    let r = request(
        f.selections.clone(),
        Paging::default(),
        &mut f.metadata,
        &f.limits,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let original = f.namespaces[0]
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .predecessors[0]
        .selection
        .clone();
    let before = f.namespaces[0].checkpoint();
    let mut nodes = Drift {
        nodes: &mut f.metadata,
        trigger: f.selections[0].bindings.clone(),
        root: f.temp.path().join("ns"),
        minimum: before.clone(),
        selection: original,
        called: Cell::new(false),
        limits: f.limits.source.clone(),
    };
    assert!(matches!(
        build(
            &f.e,
            &r,
            &f.roster,
            &mut f.namespaces,
            &mut nodes,
            &f.limits
        ),
        Err(Error::Stale)
    ));
    assert!(nodes.called.get());
    let source = f.namespaces[0].source(&key("main")).unwrap().unwrap();
    assert!(source.pending.is_none() && source.current.is_none());
    assert_eq!(source.predecessors.len(), 17);
    assert_eq!(
        f.namespaces[0].checkpoint().sequence(),
        before.sequence() + 2
    );
}

struct Missing<'a> {
    nodes: &'a mut metadata::MetadataReader,
    hidden: Object,
    corrupt: bool,
}
impl NodeStore for Missing<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if o == &self.hidden {
            Ok(self.corrupt.then(|| b"corrupt".to_vec()))
        } else {
            self.nodes.get(o)
        }
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("inspection writes metadata")
    }
}
#[test]
fn missing_corrupt_or_inconsistent_metadata_never_becomes_empty_coverage() {
    let mut f = Fixture::new(17, 1, false);
    let (r, manifest) = f.compile(Paging::default());
    let mut nodes = f.metadata.finished_reader().unwrap();
    let work = Work::new(&mut nodes, &f.limits).unwrap();
    let header: Header = work.read(&manifest).unwrap();
    for hidden in [
        manifest.clone(),
        r,
        header.cohorts.clone(),
        header.subjects.clone(),
        f.selections[0].bindings.clone(),
    ] {
        for corrupt in [false, true] {
            assert!(
                inspect(
                    &f.e,
                    &manifest,
                    &mut Missing {
                        nodes: &mut nodes,
                        hidden: hidden.clone(),
                        corrupt
                    },
                    &f.limits
                )
                .is_err()
            );
        }
    }
    let mut altered = header;
    altered.summary.bindings += 1;
    let forged = f.metadata.put(&encode(&altered).unwrap()).unwrap();
    f.metadata.finish().unwrap();
    assert!(
        inspect(
            &f.e,
            &forged,
            &mut f.metadata.finished_reader().unwrap(),
            &f.limits
        )
        .is_err()
    );
}
#[test]
fn private_roster_and_local_protection_limits_both_constrain_projection() {
    for private_roster in [false, true] {
        let mut f = Fixture::new(2, 1, false);
        let r = request(
            f.selections.clone(),
            Paging::default(),
            &mut f.metadata,
            &f.limits,
        )
        .unwrap();
        f.metadata.finish().unwrap();
        if private_roster {
            f.roster.limits.bindings = 1;
        } else {
            f.limits.selections = 2;
            f.limits.source.roots = 2;
            // The two selected roots fit. A third, unselected current root
            // makes only the local complete-projection root cap fail. This
            // must not pass merely because the roster's root cap is larger.
            let ns = &mut f.namespaces[0];
            let selection = ns.source(&key("main")).unwrap().unwrap().predecessors[0]
                .selection
                .clone();
            ns.apply(
                key("extra-enroll"),
                Command::Enroll {
                    source: key("extra"),
                    owner: key("other"),
                    class: Kind::Job,
                },
                &f.metadata,
                &f.limits.source,
            )
            .unwrap();
            ns.select(
                &Select {
                    prepare: key("extra-prepare"),
                    adopt: key("extra-adopt"),
                    source: key("extra"),
                    owner: key("other"),
                    expected_revision: 1,
                    selection,
                },
                &f.metadata,
                &f.limits.source,
            )
            .unwrap();
        }
        let before = f.namespaces[0].checkpoint();
        assert!(matches!(
            build(
                &f.e,
                &r,
                &f.roster,
                &mut f.namespaces,
                &mut f.metadata,
                &f.limits
            ),
            Err(Error::Budget)
        ));
        assert_eq!(before, f.namespaces[0].checkpoint());
    }
}

#[test]
fn rebuild_is_deterministic_and_an_unselected_predecessor_remains_a_hold() {
    let mut f = Fixture::new(17, 1, false);
    let before = f.namespaces[0].checkpoint();
    let (request, manifest) = f.compile(Paging::default());
    let metadata = f.metadata.checkpoint();
    let (again, rebuilt) = f.compile(Paging::default());
    assert_eq!(request, again);
    assert_eq!(manifest, rebuilt);
    assert_eq!(metadata, f.metadata.checkpoint());
    f.selections.pop();
    let (_, partial) = f.compile(Paging::default());
    assert_ne!(manifest, partial);
    let inventory = inspect(
        &f.e,
        &partial,
        &mut f.metadata.finished_reader().unwrap(),
        &f.limits,
    )
    .unwrap();
    assert_eq!(inventory.summary().roots, 16);
    assert_eq!(inventory.summary().held_objects, 1);
    assert_eq!(before, f.namespaces[0].checkpoint());
}

#[test]
fn selected_tree_cannot_substitute_objects_or_invent_paths_in_the_original() {
    for p in ["binding-00000", "binding-"] {
        let mut f = Fixture::new(1, 1, false);
        let before = f.namespaces[0].checkpoint();
        let entries = BTreeMap::from([(path(p).unwrap(), Object::from_bytes(b"different"))]);
        f.selections[0].bindings = Tree::build(&entries, &mut f.metadata, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        let request = request(
            f.selections.clone(),
            Paging::default(),
            &mut f.metadata,
            &f.limits,
        )
        .unwrap();
        f.metadata.finish().unwrap();
        assert!(matches!(
            build(
                &f.e,
                &request,
                &f.roster,
                &mut f.namespaces,
                &mut f.metadata,
                &f.limits
            ),
            Err(Error::Invalid)
        ));
        assert_eq!(before, f.namespaces[0].checkpoint());
    }
}

#[test]
fn exact_foreign_job_retention_and_nonhistorical_selections_are_refused() {
    for (class, owner, historical) in [
        (Kind::Branch, "foreign", true),
        (Kind::Job, "owner", true),
        (Kind::Retention, "owner", true),
        (Kind::Draft, "owner", false),
    ] {
        let mut f = Fixture::new(1, 1, false);
        let ns = &mut f.namespaces[0];
        let original = ns.source(&key("main")).unwrap().unwrap().predecessors[0]
            .selection
            .clone();
        ns.apply(
            key("enroll-other"),
            Command::Enroll {
                source: key("other"),
                owner: key(owner),
                class,
            },
            &f.metadata,
            &f.limits.source,
        )
        .unwrap();
        ns.select(
            &Select {
                prepare: key("prepare-other"),
                adopt: key("adopt-other"),
                source: key("other"),
                owner: key(owner),
                expected_revision: 1,
                selection: original,
            },
            &f.metadata,
            &f.limits.source,
        )
        .unwrap();
        if historical {
            ns.apply(
                key("withdraw-other"),
                Command::Withdraw {
                    source: key("other"),
                    owner: key(owner),
                    expected_revision: 3,
                },
                &f.metadata,
                &f.limits.source,
            )
            .unwrap();
        }
        f.selections[0].source = key("other");
        f.selections[0].predecessor_revision = 3;
        let before = ns.checkpoint();
        let r = request(
            f.selections.clone(),
            Paging::default(),
            &mut f.metadata,
            &f.limits,
        )
        .unwrap();
        f.metadata.finish().unwrap();
        assert!(
            build(
                &f.e,
                &r,
                &f.roster,
                &mut f.namespaces,
                &mut f.metadata,
                &f.limits
            )
            .is_err()
        );
        assert_eq!(before, f.namespaces[0].checkpoint());
    }
}

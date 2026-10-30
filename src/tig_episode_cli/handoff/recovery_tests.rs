//! Synthetic retained-job state, not media or creative acceptance.
use super::*;

struct Fixture {
    profile: Profile,
    journal: SourceJournal,
    report: PathBuf,
    report_bytes: Vec<u8>,
    // Drop journal handles before the owned temporary directory on Windows.
    _root: tempfile::TempDir,
}
fn tree(store: &mut impl NodeStore, name: &str, value: Value, extra: &[Object]) -> Selection {
    let raw = identity::canonical_json(&value).unwrap();
    let object = store.put(&raw).unwrap();
    let mut names = BTreeMap::from([(LogicalPath::parse(name).unwrap(), object.clone())]);
    for item in extra {
        names.insert(
            LogicalPath::parse(&format!("objects/{}", item.sha256.as_str())).unwrap(),
            item.clone(),
        );
    }
    Selection {
        root: Tree::build(&names, store, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone(),
        context: object.sha256,
    }
}
fn adopt(journal: &mut SourceJournal, source: &str, selection: Selection, revision: u64) {
    let prepared = journal
        .apply(
            key(&format!("{source}-prepare-{revision}")).unwrap(),
            Command::Prepare {
                source: key(source).unwrap(),
                owner: key("fixture").unwrap(),
                expected_revision: revision,
                selection,
            },
        )
        .unwrap();
    journal
        .apply(
            key(&format!("{source}-adopt-{revision}")).unwrap(),
            Command::Adopt {
                source: key(source).unwrap(),
                owner: key("fixture").unwrap(),
                expected_revision: prepared.source_revision,
                prepared: prepared.reference,
            },
        )
        .unwrap();
}
impl Fixture {
    fn new() -> Self {
        Self::case(true, [2, 2])
    }
    fn case(succeeded: bool, states: [u8; 2]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let cache = base.join("cache");
        fs::create_dir_all(cache.join("objects/sha256")).unwrap();
        let coordination = base.join("coordination");
        fs::create_dir(&coordination).unwrap();
        let metadata = base.join("metadata");
        let mut store = MetadataStore::create(
            &metadata,
            tig_store::metadata::Limits {
                nodes: 10000,
                bytes: 4 * 1024 * 1024,
                database_bytes: 8 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let context = Selection {
            root: Object::from_bytes(b"frozen-source"),
            context: Digest::blob(b"source-context"),
        };
        let header = json!({"source":context,"recipe":Digest::blob(b"recipe"),"checkpoint":"captured",
            "admission":{"scope":"episode"},"binding":"binding","consumption":"consumption","operation":"render"});
        let child = tree(
            &mut store,
            "job/identity",
            json!({"component":"scene-a"}),
            &[],
        );
        let input = tree(
            &mut store,
            "job/identity",
            header.clone(),
            std::slice::from_ref(&child.root),
        );
        let parent = tree(
            &mut store,
            "job/identity",
            header,
            &[child.root.clone(), input.root.clone()],
        );
        let temporary = ["scene/scene-a", "parent"].map(|name| {
            phase("episode", "run-v1", name)
                .unwrap()
                .as_str()
                .to_owned()
        });
        let report_bytes=identity::canonical_json(&json!({"schema":"reel.tig-episode-worker-result.experimental-1",
            "recipe":Digest::blob(b"recipe"),"decoded_master_matches_ordered_segments":true,"timestamps_verified":true,
            "temporary_sources":temporary,"resources":parent})).unwrap();
        let report_object = Object::from_bytes(&report_bytes);
        let report =
            tig_store::existing::Layout::ReelCas.object_path(&cache, &report_object.sha256);
        fs::create_dir_all(report.parent().unwrap()).unwrap();
        fs::write(&report, &report_bytes).unwrap();
        let raw =
            identity::canonical_json(&json!({"schema":"reel.tig-runner-retention.experimental-1",
            "source_id":"episode","owner":"fixture","revision":2,"succeeded":succeeded,"source":input}))
            .unwrap();
        let retention = store.put(&raw).unwrap();
        let mut names = BTreeMap::from([
            (
                LogicalPath::parse("retention/identity").unwrap(),
                retention.clone(),
            ),
            (
                LogicalPath::parse(&format!(
                    "outputs/{}",
                    Digest::blob(b"worker.json").as_str()
                ))
                .unwrap(),
                report_object,
            ),
        ]);
        for name in ["result/request", "result/evidence"] {
            names.insert(
                LogicalPath::parse(name).unwrap(),
                store
                    .put(
                        &identity::canonical_json(
                            &json!({"synthetic_diagnostic":name,"succeeded":succeeded}),
                        )
                        .unwrap(),
                    )
                    .unwrap(),
            );
        }
        if !succeeded {
            names.remove(
                &LogicalPath::parse(&format!(
                    "outputs/{}",
                    Digest::blob(b"worker.json").as_str()
                ))
                .unwrap(),
            );
            fs::remove_file(&report).unwrap();
        }
        let result = Selection {
            root: Tree::build(&names, &mut store, TreeLimits::default())
                .unwrap()
                .0
                .root()
                .clone(),
            context: retention.sha256,
        };
        let minimum = store.finish().unwrap();
        drop(store);
        let journal_root = base.join("owners");
        let mut journal = SourceJournal::create(
            &journal_root,
            key("project").unwrap(),
            key("epoch").unwrap(),
        )
        .unwrap();
        adopt(&mut journal, "episode", input, 0);
        adopt(&mut journal, "episode", result, 2);
        for ((source, selection), (status, label)) in temporary
            .iter()
            .zip([child, parent])
            .zip(states.into_iter().zip(["scene/scene-a", "parent"]))
        {
            match status {
                0 => {}
                1 => {
                    let identity = Identity {
                        source: key(source).unwrap(),
                        owner: key("fixture").unwrap(),
                    };
                    let operation =
                        phase("episode", "run-v1", &format!("protect/{label}")).unwrap();
                    journal
                        .apply(
                            jobs::phase(&identity, &operation, "prepare").unwrap(),
                            Command::Prepare {
                                source: identity.source,
                                owner: identity.owner,
                                expected_revision: 0,
                                selection,
                            },
                        )
                        .unwrap();
                }
                2 => adopt(&mut journal, source, selection, 0),
                _ => panic!("invalid synthetic source stage"),
            }
        }
        let fake_pin = json!({"sha256":Digest::blob(b"never-open-this-tool"),"bytes":20});
        let mut fake_file = fake_pin.clone();
        fake_file["path"] = json!(base.join("tool-does-not-exist"));
        let profile:Profile=serde_json::from_value(json!({"schema":SCHEMA,"inputs":{
            "source":{},"context":context,"request":{"schema":tig_episode::SCHEMA,"assembly_id":"assembly","assembly_definition":"assembly-type",
            "episode_definition":"episode-type","scene_definitions":["scene-type"],"component_definitions":[]},
            "runtime":{"host":fake_pin,"ffmpeg":fake_file,"ffprobe":fake_file,"dependencies":{}},
            "scenes":[{"id":"scene-a","output_id":"scene-output","output_definition":"output-type","request":{
                "schema":tig_scene::SCHEMA,"scene_id":"scene-a","scene_definition":"scene-type","primary_definitions":["image-type"],
                "language":"es","controls":{"catalog":"catalog","episode":"episode","policy":"policy","profile":"profile","template_receipt":null},
                "delivery_id":"delivery","delivery_title":"fixture"}}],"presentations":[]},
            "cache":{"path_policy":"configured-local-roots","cache_roots":[cache],"coordination_root":coordination},"preparation_cache":cache,
            "metadata_root":metadata,"metadata_minimum":minimum,"journal_root":journal_root,"journal_minimum":journal.checkpoint(),
            "source_id":"episode","owner":"fixture","expected_revision":0,"operation":"run-v1",
            "budget_bytes":1024*1024,"scratch_bytes":65536,"timeout_seconds":10,"reference_staging_bytes":65536,"reference_picture_processes":2,
            "max_objects":10000,"max_logical_bytes":1024*1024,"outputs":{"receipt.json":65536,"master.mkv":65536}
        })).unwrap();
        validate(&profile).unwrap();
        Self {
            _root: root,
            profile,
            journal,
            report,
            report_bytes,
        }
    }
    fn state(&mut self) -> source_claims::Source {
        self.journal
            .source(&key("episode").unwrap())
            .unwrap()
            .unwrap()
    }
    fn interrupt(&mut self) {
        let error = finish_after_prepare(&self.profile, &mut self.journal, || {
            bail!("synthetic interruption after durable prepare")
        })
        .unwrap_err();
        assert!(error.to_string().contains("synthetic interruption"));
        assert_eq!(self.state().revision, 5);
        assert!(self.state().pending.is_some());
        for source in expected(&self.profile).unwrap() {
            assert_eq!(
                self.journal
                    .source(&key(&source).unwrap())
                    .unwrap()
                    .unwrap()
                    .revision,
                2
            );
        }
    }
}
#[test]
fn prepared_episode_handoff_reopens_rebuilds_and_resumes_without_runtime_or_media() {
    let mut f = Fixture::new();
    f.interrupt();
    let checkpoint = f.journal.checkpoint();
    let pending = f.state().pending.unwrap();
    // Reopen both authorities, like restarting the command after a crash.
    f.journal = SourceJournal::open(&f.profile.journal_root, &checkpoint).unwrap();
    f.profile.max_objects = 1;
    assert!(finish(&f.profile, &mut f.journal).is_err());
    assert_eq!(f.journal.checkpoint(), checkpoint);
    f.profile.max_objects = 10000;
    fs::remove_file(&f.report).unwrap();
    assert!(finish(&f.profile, &mut f.journal).is_err());
    assert_eq!(f.journal.checkpoint(), checkpoint);
    fs::write(&f.report, &f.report_bytes).unwrap();
    let result = finish(&f.profile, &mut f.journal).unwrap();
    assert_eq!(result["temporary_owner_cleanup_required"], false);
    let state = f.state();
    assert_eq!(state.revision, 6);
    assert!(state.pending.is_none());
    assert_eq!(state.current.unwrap().selection, pending.selection);
    for source in expected(&f.profile).unwrap() {
        let state = f.journal.source(&key(&source).unwrap()).unwrap().unwrap();
        assert_eq!(state.revision, 3);
        assert!(state.current.is_none());
    }
    let checkpoint = f.journal.checkpoint();
    finish(&f.profile, &mut f.journal).unwrap();
    assert_eq!(f.journal.checkpoint(), checkpoint);
}
#[test]
fn foreign_prepare_or_different_pending_packet_keeps_every_obligation_live() {
    let mut original = Fixture::new();
    original.interrupt();
    let selection = original.state().pending.unwrap().selection;
    for same_packet in [false, true] {
        let mut f = Fixture::new();
        f.journal
            .apply(
                key("foreign-prepare").unwrap(),
                Command::Prepare {
                    source: key("episode").unwrap(),
                    owner: key("fixture").unwrap(),
                    expected_revision: 4,
                    selection: if same_packet {
                        selection.clone()
                    } else {
                        Selection {
                            root: Object::from_bytes(b"foreign"),
                            context: Digest::blob(b"foreign"),
                        }
                    },
                },
            )
            .unwrap();
        let checkpoint = f.journal.checkpoint();
        assert!(finish(&f.profile, &mut f.journal).is_err());
        assert_eq!(f.journal.checkpoint(), checkpoint);
        assert_eq!(f.state().revision, 5);
        for source in expected(&f.profile).unwrap() {
            assert_eq!(
                f.journal
                    .source(&key(&source).unwrap())
                    .unwrap()
                    .unwrap()
                    .revision,
                2
            );
        }
    }
}

#[test]
fn failed_episode_retains_diagnostics_and_every_present_or_prepared_component_before_release() {
    for stages in [[0, 0], [1, 0], [2, 0], [2, 1], [2, 2]] {
        let mut f = Fixture::case(false, stages);
        assert!(
            !f.report.exists(),
            "a failed worker need not emit a success report"
        );
        let before = f.state().current.unwrap().selection;
        let mut reader =
            MetadataReader::open(&f.profile.metadata_root, &f.profile.metadata_minimum).unwrap();
        let original = entries(&before, &mut reader).unwrap();
        let expected = expected(&f.profile).unwrap();
        let mut held = Vec::new();
        for source in &expected {
            if let Some(state) = f.journal.source(&key(source).unwrap()).unwrap() {
                held.push(
                    state
                        .current
                        .map(|state| state.selection)
                        .or_else(|| state.pending.map(|state| state.selection))
                        .unwrap(),
                );
            }
        }
        if stages == [2, 1] {
            assert!(
                finish_after_prepare(&f.profile, &mut f.journal, || bail!("stop failed handoff"))
                    .is_err()
            );
            assert_eq!(f.state().revision, 5);
            assert_eq!(
                f.journal
                    .source(&key(&expected[1]).unwrap())
                    .unwrap()
                    .unwrap()
                    .revision,
                1
            );
            f.journal =
                SourceJournal::open(&f.profile.journal_root, &f.journal.checkpoint()).unwrap();
        }
        let finished = finish(&f.profile, &mut f.journal).unwrap();
        assert_eq!(finished["succeeded"], false);
        assert_eq!(finished["temporary_owner_cleanup_required"], false);
        let state = f.state();
        assert_eq!(state.revision, 6);
        let kept = entries(&state.current.unwrap().selection, &mut reader).unwrap();
        for object in original.values().chain(std::iter::once(&before.root)) {
            assert!(kept.values().any(|kept| kept == object));
        }
        for selection in held {
            assert!(kept.values().any(|kept| kept == &selection.root));
            assert!(
                entries(&selection, &mut reader)
                    .unwrap()
                    .values()
                    .all(|object| kept.values().any(|kept| kept == object))
            );
        }
        for (source, stage) in expected.iter().zip(stages) {
            let state = f.journal.source(&key(source).unwrap()).unwrap();
            if stage == 0 {
                assert!(state.is_none());
            } else {
                let state = state.unwrap();
                assert_eq!(state.revision, u64::from(stage) + 1);
                assert!(state.current.is_none() && state.pending.is_none());
            }
        }
        let checkpoint = f.journal.checkpoint();
        finish(&f.profile, &mut f.journal).unwrap();
        assert_eq!(f.journal.checkpoint(), checkpoint);
    }
}

#[test]
fn failed_episode_refuses_a_foreign_component_prepare_without_changing_claims() {
    let mut original = Fixture::new();
    let source = expected(&original.profile).unwrap()[0].clone();
    let selection = original
        .journal
        .source(&key(&source).unwrap())
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    let mut f = Fixture::case(false, [0, 0]);
    f.journal
        .apply(
            key("foreign-component-prepare").unwrap(),
            Command::Prepare {
                source: key(&source).unwrap(),
                owner: key("fixture").unwrap(),
                expected_revision: 0,
                selection,
            },
        )
        .unwrap();
    let checkpoint = f.journal.checkpoint();
    assert!(finish(&f.profile, &mut f.journal).is_err());
    assert_eq!(f.journal.checkpoint(), checkpoint);
    assert_eq!(f.state().revision, 4);
    assert_eq!(
        f.journal
            .source(&key(&source).unwrap())
            .unwrap()
            .unwrap()
            .revision,
        1
    );
}

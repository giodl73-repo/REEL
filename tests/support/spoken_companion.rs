//! Original public signals and SVG; no voice recording or spoken-word claim.
use super::*;
use reel::{
    cue_relative::{CueMarker, MarkerKind},
    scene_delivery::{FileRef, InputFiles},
    spoken_programme::{checker, companion as api},
};
use std::{path::Path, time::Duration};

struct BrowserFixture {
    plan: reel::tig_browser::Prepared,
    paths: BTreeMap<PathBuf, (FileRef, PathBuf, fs::File)>,
    _controls: tempfile::TempDir,
}
impl InputFiles for BrowserFixture {
    fn verified_path(&self, file: &FileRef) -> Result<PathBuf> {
        let (expected, path, _) = self
            .paths
            .get(&file.path)
            .ok_or_else(|| anyhow::anyhow!("fixture provider refused original name"))?;
        if expected != file {
            bail!("fixture provider pin differs")
        };
        Ok(path.clone())
    }
}
impl BrowserFixture {
    fn new(f: &CompanionFixture, c: &api::Composition, forge: bool) -> Self {
        Self::from_source(&f.root, &f.candidate, &f.receipt, &f.request.svg, c, forge)
    }

    fn from_source(
        root: &Path,
        candidate: &Path,
        receipt: &[u8],
        svg_source: &FileRef,
        c: &api::Composition,
        forge: bool,
    ) -> Self {
        let controls = tempfile::tempdir().unwrap();
        let mut clock = c.clock_bytes().to_vec();
        if forge {
            let mut v: serde_json::Value = serde_json::from_slice(&clock).unwrap();
            v["anchors"][1]["sample"] = json!(v["anchors"][1]["sample"].as_u64().unwrap() + 2000);
            clock = serde_json::to_vec_pretty(&v).unwrap();
        }
        let receipt: producer::ProductionReceipt = serde_json::from_slice(receipt).unwrap();
        let original_wave = receipt
            .editions
            .iter()
            .find(|e| e.edition_id == c.edition_id())
            .unwrap()
            .wave
            .clone();
        let svg = FileRef {
            path: "admitted/svg".into(),
            ..svg_source.clone()
        };
        let wave = FileRef {
            path: format!("admitted/audio/{}", c.edition_id()).into(),
            ..original_wave.clone()
        };
        let mut paths = BTreeMap::new();
        let mut inputs = BTreeMap::new();
        for (alias, file, path) in [
            ("opening.svg", svg, root.join(&svg_source.path)),
            (
                "audio.wav",
                wave.clone(),
                candidate.join(&original_wave.path),
            ),
            (
                "clock.json",
                pin("bound-clock.json", &clock),
                controls.path().join("clock.json"),
            ),
            (
                "evidence.json",
                pin("bound-evidence.json", c.evidence_bytes()),
                controls.path().join("evidence.json"),
            ),
        ] {
            if alias == "clock.json" {
                fs::write(&path, &clock).unwrap();
            }
            if alias == "evidence.json" {
                fs::write(&path, c.evidence_bytes()).unwrap();
            }
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(1);
            }
            let held = options.open(&path).unwrap();
            inputs.insert(alias.to_owned(), file.clone());
            paths.insert(file.path.clone(), (file, path, held));
        }
        let mut manifest = c.manifest().clone();
        if forge {
            manifest.source_clock.as_mut().unwrap().sha256 = hash(&clock);
            manifest
                .assets
                .iter_mut()
                .find(|a| a.path == "clock.json")
                .unwrap()
                .sha256 = hash(&clock);
        }
        let bytes = inputs.values().map(|f| f.bytes).sum();
        Self {
            plan: reel::tig_browser::Prepared {
                // Public test-only worker context, following the existing native fixture.
                // No production checkpoint, admission or custody is asserted.
                source_checkpoint: tig_core::Digest::blob(b"synthetic companion fixture"),
                source_record: tig_assets::graph::Reference {
                    id: "fixture/companion".into(),
                    object: tig_core::Object::from_bytes(b"fixture declaration"),
                },
                manifest_bytes: serde_json::to_vec(&manifest).unwrap(),
                manifest,
                inputs,
                alias_bytes: bytes,
                unique_bytes: bytes,
                unique_objects: 4,
            },
            paths,
            _controls: controls,
        }
    }
}

struct CompanionFixture {
    root: PathBuf,
    candidate: PathBuf,
    f: Fixture,
    plan: Vec<u8>,
    receipt: Vec<u8>,
    runtime: compiler::CompilerIdentity,
    policy: ProcessingPolicy,
    access: PathBuf,
    access_manifest: Vec<u8>,
    request: api::Request,
}
fn pin(path: impl Into<PathBuf>, raw: &[u8]) -> FileRef {
    FileRef {
        path: path.into(),
        sha256: hash(raw),
        bytes: raw.len() as u64,
    }
}
fn composition_request(root: &Path, edition_kind: &str) -> api::Request {
    let svg=br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect id="r" width="8" height="8" fill="red"/><script>addEventListener('reelframe',e=>{const a=e.detail.sourceClock.anchorFrames,f=e.detail.frame;document.getElementById('r').setAttribute('fill',f&lt;a[1]?'red':f&lt;a[2]?'lime':'blue')})</script></svg>"##;
    fs::write(root.join("fixture-private-canary.svg"), svg).unwrap();
    api::Request {
        schema: api::SCHEMA.into(),
        edition_ids: ["en", "es"]
            .map(|language| format!("{language}-{edition_kind}"))
            .to_vec(),
        svg: pin("fixture-private-canary.svg", svg),
        width: 8,
        height: 8,
        events: [("follow-rhythm", "first"), ("begin-again", "second")]
            .into_iter()
            .map(|(id, marker)| api::Event {
                id: id.into(),
                bindings: ["en", "es"]
                    .into_iter()
                    .map(|language| api::Binding {
                        edition_id: format!("{language}-{edition_kind}"),
                        anchor: api::Anchor::EngineeringMarker {
                            performance_id: format!("{language}-opening"),
                            marker_id: marker.into(),
                        },
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(feature = "print")]
pub(super) fn check_shared_opening_companion(
    inputs: reel::spoken_programme::installation::Inputs<'_>,
    root: &Path,
) {
    let visuals = root.to_path_buf();
    let access: checker::AccessPackage = serde_json::from_slice(inputs.access_manifest).unwrap();
    let prepared = api::prepare(
        composition_request(root, "installation"),
        api::Inputs {
            spoken: inputs.spoken,
            sources: inputs.sources,
            candidate: inputs.candidate,
            visuals: &visuals,
            access: inputs.access,
            access_manifest: inputs.access_manifest,
        },
        Duration::from_secs(30),
    )
    .unwrap();
    for composition in prepared.compositions() {
        let edition = access
            .editions
            .iter()
            .find(|edition| edition.edition_id == composition.edition_id())
            .unwrap();
        let evidence: serde_json::Value =
            serde_json::from_slice(composition.evidence_bytes()).unwrap();
        assert_eq!(evidence["access"]["wave"]["sha256"], edition.wave.sha256);
        assert_eq!(
            evidence["access"]["transcript"]["sha256"],
            edition.transcript.sha256
        );
    }
    let output = root.join("shared-companion");
    api::produce(
        &prepared,
        &output,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
    .unwrap();
    api::check(&prepared, &output, Duration::from_secs(30)).unwrap();
}

impl CompanionFixture {
    fn new(v2: bool) -> Self {
        Self::with_trim(v2, 0)
    }
    fn with_trim(v2: bool, trim: u64) -> Self {
        let mut f = Fixture::new();
        f.document.modules.truncate(1);
        f.document.performances = vec![
            f.document.performances[0].clone(),
            f.document.performances[3].clone(),
        ];
        f.signals = vec![f.signals[0].clone(), f.signals[3].clone()];
        f.document.editions.retain(|e| e.id.ends_with("-technical"));
        for e in &mut f.document.editions {
            e.required_modules = vec!["opening".into()];
            e.selections.truncate(1);
            e.selections[0].takes[0].start_sample = trim;
            e.joins.clear();
        }
        for p in &mut f.document.performances {
            p.scope.edition_ids.retain(|id| id.ends_with("-technical"));
            let (a, b) = if p.language == "en" {
                (if v2 { 50000 } else { 48000 }, 96000)
            } else {
                (60000, 120000)
            };
            p.markers = vec![
                CueMarker {
                    id: "first".into(),
                    kind: MarkerKind::Phrase,
                    start_sample: a,
                    end_sample: a + 2000,
                },
                CueMarker {
                    id: "second".into(),
                    kind: MarkerKind::Phrase,
                    start_sample: b,
                    end_sample: b + 2000,
                },
            ];
        }
        if v2 {
            f.signals[0][9000] += 1;
        }
        f.sync();
        let policy = ProcessingPolicy::identity();
        let runtime = producer::current_runtime().unwrap();
        // Compile once and produce from those exact original plan bytes.
        // Avoid repeated whole-executable observations in this fixture.
        let plan = serde_json::to_vec_pretty(
            &compiler::compile_with_inputs(
                &f.document,
                f.temp.path(),
                &runtime,
                CompilationProfile::Engineering,
            )
            .unwrap(),
        )
        .unwrap();
        let produced = producer::produce(
            &f.document,
            &plan,
            &policy,
            f.temp.path(),
            &f.output(),
            || Ok(()),
        )
        .unwrap();
        let receipt = serde_json::to_vec_pretty(&produced).unwrap();
        let mut checked = checker::check(
            checker::CheckRequest {
                document: &f.document,
                plan_bytes: &plan,
                receipt_bytes: &receipt,
                processing: &policy,
                expected_runtime: &runtime,
            },
            f.temp.path(),
            &f.output(),
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || Ok(()),
            },
        )
        .unwrap();
        let access = f.temp.path().join("access");
        checker::write_access(&mut checked, &access, || Ok(())).unwrap();
        let access_manifest = fs::read(access.join("access.json")).unwrap();
        let request = composition_request(f.temp.path(), "technical");
        let root = f.temp.path().to_path_buf();
        let candidate = f.output();
        Self {
            root,
            candidate,
            f,
            plan,
            receipt,
            runtime,
            policy,
            access,
            access_manifest,
            request,
        }
    }
    fn inputs(&self) -> api::Inputs<'_> {
        api::Inputs {
            spoken: checker::CheckRequest {
                document: &self.f.document,
                plan_bytes: &self.plan,
                receipt_bytes: &self.receipt,
                processing: &self.policy,
                expected_runtime: &self.runtime,
            },
            sources: &self.root,
            candidate: &self.candidate,
            visuals: &self.root,
            access: &self.access,
            access_manifest: &self.access_manifest,
        }
    }
    fn retained(&self) -> Self {
        fn copy(source: &Path, target: &Path) {
            fs::create_dir_all(target).unwrap();
            for entry in fs::read_dir(source).unwrap() {
                let entry = entry.unwrap();
                let to = target.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &to)
                } else {
                    fs::copy(entry.path(), to).unwrap();
                }
            }
        }
        // Each new test-only retained owner has the same inherited outer limit.
        // No production store, migration or custody is established by this copy.
        let new_temp = tempfile::tempdir().unwrap();
        copy(self.f.temp.path(), new_temp.path());
        let root = new_temp.path().to_path_buf();
        let candidate = root.join("candidate");
        let access = new_temp.path().join("access");
        Self {
            root,
            candidate,
            f: Fixture {
                temp: new_temp,
                document: self.f.document.clone(),
                signals: self.f.signals.clone(),
                started: std::time::Instant::now(),
            },
            plan: self.plan.clone(),
            receipt: self.receipt.clone(),
            runtime: self.runtime.clone(),
            policy: ProcessingPolicy::identity(),
            access,
            access_manifest: self.access_manifest.clone(),
            request: self.request.clone(),
        }
    }
}

#[test]
#[ignore = "bounded companion qualification; explicitly invoked in hosted core shard 2"]
fn bilingual_markers_use_sealed_audio_and_keep_text_access_signal_basis() {
    let f = CompanionFixture::new(false);
    let p = api::prepare(f.request.clone(), f.inputs(), Duration::from_secs(30)).unwrap();
    use reel::spoken_programme::interactive::{Selection, companion_placements};

    let inputs = f.inputs();
    let mut checked = checker::check(
        inputs.spoken,
        inputs.sources,
        inputs.candidate,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
    .unwrap();
    checker::verify_access(&mut checked, inputs.access_manifest, inputs.access, || {
        Ok(())
    })
    .unwrap();
    for composition in p.compositions() {
        let edition = checked
            .access()
            .editions
            .iter()
            .find(|edition| edition.edition_id == composition.edition_id())
            .unwrap();
        let chapter = &edition.chapters[0];
        let span = Selection {
            edition: &edition.edition_id,
            language: &edition.language,
            module: &chapter.module_id,
            programme_sha256: &edition.wave.sha256,
            access_sha256: &edition.transcript.sha256,
            first_frame: chapter.start_sample,
            end_frame: chapter.end_sample,
        };
        let placements = companion_placements(&checked, span, composition).unwrap();
        let expected = if edition.language == "en" {
            [0, 48000, 96000]
        } else {
            [0, 60000, 120000]
        };
        assert_eq!(
            placements
                .iter()
                .map(|event| event.frame)
                .collect::<Vec<_>>(),
            expected
        );
        let other = p
            .compositions()
            .iter()
            .find(|other| other.edition_id() != span.edition)
            .unwrap();
        assert!(companion_placements(&checked, span, other).is_err());
    }
    for (c, expected) in p
        .compositions()
        .iter()
        .zip([[0, 48000, 96000, 144000], [0, 60000, 120000, 168000]])
    {
        let clock: serde_json::Value = serde_json::from_slice(c.clock_bytes()).unwrap();
        let samples: Vec<_> = clock["anchors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["sample"].as_u64().unwrap())
            .collect();
        assert_eq!(samples, expected);
        let evidence: serde_json::Value = serde_json::from_slice(c.evidence_bytes()).unwrap();
        assert_eq!(
            evidence["events"][0]["basis"],
            "engineering-marker-not-spoken-words"
        );
        assert!(evidence["events"][0]["phrase"].is_null());
        assert!(
            evidence["access"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["kind"] == "engineering-signal"
                    && e["text_basis"].is_null()
                    && e["text"] == "[Engineering signal; no spoken-word evidence]")
        );
        let language = evidence["access"]["language"].as_str().unwrap();
        let script = f.f.document.modules[0].cues[0]
            .scripts
            .iter()
            .find(|s| s.language == language)
            .unwrap();
        assert_eq!(evidence["recipe"]["cues"][0]["script"]["text"], script.text);
    }
    let output = f.f.temp.path().join("companion");
    let result = api::produce(
        &p,
        &output,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
    .unwrap();
    let summary = serde_json::to_string(&result).unwrap();
    assert!(!summary.contains("canary"));
    assert!(
        !result.acoustic_word_fidelity_verified
            && !result.authority_verified
            && !result.device_playback_checked
            && !result.release_authorized
    );
    api::check(&p, &output, Duration::from_secs(30)).unwrap();
    let original = fs::read(output.join("companion.json")).unwrap();
    assert!(
        api::produce(
            &p,
            &output,
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(fs::read(output.join("companion.json")).unwrap(), original);
    fs::write(output.join("edition-000.clock.json"), b"false clock").unwrap();
    assert!(api::check(&p, &output, Duration::from_secs(30)).is_err());
}

#[test]
#[ignore = "bounded companion qualification; explicitly invoked in hosted core shard 2"]
fn changing_english_take_moves_only_english_companion() {
    let a = CompanionFixture::new(false);
    let b = CompanionFixture::new(true);
    let pa = api::prepare(a.request.clone(), a.inputs(), Duration::from_secs(30)).unwrap();
    let pb = api::prepare(b.request.clone(), b.inputs(), Duration::from_secs(30)).unwrap();
    assert_ne!(
        pa.compositions()[0].clock_bytes(),
        pb.compositions()[0].clock_bytes()
    );
    assert_ne!(
        pa.compositions()[0].evidence_bytes(),
        pb.compositions()[0].evidence_bytes()
    );
    assert_eq!(
        pa.compositions()[1].clock_bytes(),
        pb.compositions()[1].clock_bytes()
    );
    assert_eq!(
        pa.compositions()[1].evidence_bytes(),
        pb.compositions()[1].evidence_bytes()
    );
    let ea: serde_json::Value =
        serde_json::from_slice(pa.compositions()[0].evidence_bytes()).unwrap();
    let eb: serde_json::Value =
        serde_json::from_slice(pb.compositions()[0].evidence_bytes()).unwrap();
    assert_eq!(ea["events"][0]["event_id"], eb["events"][0]["event_id"]);
}

#[test]
#[ignore = "bounded companion qualification; explicitly invoked in hosted core shard 2"]
fn invalid_bindings_stale_audio_text_and_interrupted_publication_refuse() {
    let f = CompanionFixture::new(false);
    let candidate = f.f.output();
    for (case, diagnostic) in [
        (0, "event"),
        (1, "closure"),
        (2, "performance"),
        (3, "marker"),
        (4, "engineering"),
        (5, "request"),
        (6, "edition"),
        (7, "distinct interior frames"),
    ] {
        let mut r = f.request.clone();
        match case {
            0 => r.events[1].id = r.events[0].id.clone(),
            1 => r.events[0].bindings.pop().map(|_| ()).unwrap(),
            2 => {
                r.events[0].bindings[0].anchor = api::Anchor::EngineeringMarker {
                    performance_id: "missing".into(),
                    marker_id: "first".into(),
                }
            }
            3 => {
                r.events[0].bindings[0].anchor = api::Anchor::EngineeringMarker {
                    performance_id: "en-opening".into(),
                    marker_id: "missing".into(),
                }
            }
            4 => {
                r.events[0].bindings[0].anchor = api::Anchor::Phrase {
                    performance_id: "en-opening".into(),
                    marker_id: "first".into(),
                    phrase: "false words".into(),
                    occurrence: None,
                }
            }
            5 => r.width = 641,
            6 => {
                // Preserve binding closure so this reaches edition lookup.
                r.edition_ids[0] = "unknown".into();
                for event in &mut r.events {
                    event.bindings[0].edition_id = "unknown".into();
                }
            }
            7 => {
                r.events[1].bindings[0].anchor = api::Anchor::EngineeringMarker {
                    performance_id: "en-opening".into(),
                    marker_id: "first".into(),
                }
            }
            _ => unreachable!(),
        };
        let error = api::prepare(r, f.inputs(), Duration::from_secs(30))
            .err()
            .unwrap();
        assert!(
            error.to_string().contains(diagnostic),
            "case {case}: {error:#}"
        );
    }
    assert!(api::prepare(f.request.clone(), f.inputs(), Duration::ZERO).is_err());
    let p = api::prepare(f.request.clone(), f.inputs(), Duration::from_secs(30)).unwrap();
    let output = f.f.temp.path().join("interrupted");
    assert!(
        api::produce(
            &p,
            &output,
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || {
                    if output.exists() {
                        bail!("test cancellation after reservation")
                    };
                    Ok(())
                }
            }
        )
        .is_err()
    );
    assert!(!output.exists());
    let transcript = f.access.join("edition-000.txt");
    let original = fs::read(&transcript).unwrap();
    fs::write(&transcript, b"changed authored words").unwrap();
    assert!(
        api::produce(
            &p,
            &output,
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || Ok(())
            }
        )
        .is_err()
    );
    fs::write(&transcript, original).unwrap();
    let wave = candidate.join("edition-000.wav");
    let mut bytes = fs::read(&wave).unwrap();
    bytes[50] ^= 1;
    fs::write(&wave, bytes).unwrap();
    assert!(
        api::produce(
            &p,
            &output,
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || Ok(())
            }
        )
        .is_err()
    );
    assert!(!output.exists());
}

#[test]
#[ignore = "bounded companion qualification; explicitly invoked in hosted core shard 2"]
fn earlier_companion_reopens_with_retained_inputs_after_replacement() {
    let a = CompanionFixture::new(false);
    let original = a.f.temp.path().to_path_buf();
    let retained = a.retained();
    drop(a);
    assert!(!original.exists());
    let p = api::prepare(
        retained.request.clone(),
        retained.inputs(),
        Duration::from_secs(30),
    )
    .unwrap();
    let output = retained.f.temp.path().join("retained-companion");
    api::produce(
        &p,
        &output,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
    .unwrap();
    let b = CompanionFixture::new(true);
    let pb = api::prepare(b.request.clone(), b.inputs(), Duration::from_secs(30)).unwrap();
    api::check(&p, &output, Duration::from_secs(30)).unwrap();
    assert!(api::check(&pb, &output, Duration::from_secs(30)).is_err());
    let source = retained
        .f
        .temp
        .path()
        .join(&retained.f.document.performances[0].source.path);
    fs::remove_file(source).unwrap();
    assert!(api::check(&p, &output, Duration::from_secs(30)).is_err());
    let newer = b.f.temp.path().join("new-companion");
    api::produce(
        &pb,
        &newer,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
    .unwrap();
}

#[test]
#[ignore = "bounded companion qualification; explicitly invoked in hosted core shard 2"]
fn altered_clock_and_browser_pins_cannot_replace_original_checked_controls() {
    let f = CompanionFixture::new(false);
    let p = api::prepare(f.request.clone(), f.inputs(), Duration::from_secs(30)).unwrap();
    let c = &p.compositions()[0];
    let mut browser = BrowserFixture::new(&f, c, false);
    api::validate_browser(
        &p,
        c.edition_id(),
        &browser.plan,
        &browser,
        Duration::from_secs(30),
    )
    .unwrap();
    let forged = BrowserFixture::new(&f, c, true);
    assert!(
        api::validate_browser(
            &p,
            c.edition_id(),
            &forged.plan,
            &forged,
            Duration::from_secs(30)
        )
        .err()
        .unwrap()
        .to_string()
        .contains("manifest differs")
    );
    // A self-consistent admitted object can still be the wrong programme.
    // Its provider/hash/length all validate; original caller controls must win.
    let mut substituted = BrowserFixture::new(&f, c, false);
    let old_audio = substituted.plan.inputs["audio.wav"].clone();
    let source_path = substituted.paths[&old_audio.path].1.clone();
    let mut raw = fs::read(source_path).unwrap();
    raw[50] ^= 1;
    let path = substituted._controls.path().join("different.wav");
    fs::write(&path, &raw).unwrap();
    let replacement = pin("admitted/false-audio", &raw);
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let held = options.open(&path).unwrap();
    substituted.paths.remove(&old_audio.path);
    substituted
        .paths
        .insert(replacement.path.clone(), (replacement.clone(), path, held));
    substituted
        .plan
        .inputs
        .insert("audio.wav".into(), replacement.clone());
    assert_eq!(
        hash(&substituted.control_bytes(&replacement).unwrap()),
        replacement.sha256
    );
    assert!(
        api::validate_browser(
            &p,
            c.edition_id(),
            &substituted.plan,
            &substituted,
            Duration::from_secs(30)
        )
        .err()
        .unwrap()
        .to_string()
        .contains("original object differs")
    );
    // The original SVG really exists and validates through the legacy local
    // provider, so any hidden fallback would turn this refusal into success.
    f.root.verified_path(&f.request.svg).unwrap();
    let admitted_svg = browser.plan.inputs["opening.svg"].path.clone();
    assert_ne!(admitted_svg, f.request.svg.path);
    browser.paths.remove(&admitted_svg);
    assert!(
        api::validate_browser(
            &p,
            c.edition_id(),
            &browser.plan,
            &browser,
            Duration::from_secs(30)
        )
        .err()
        .unwrap()
        .to_string()
        .contains("provider refused")
    );
}

#[test]
#[ignore = "bounded companion qualification; explicitly invoked in hosted core shard 2"]
fn nonintegral_duration_and_rehashed_false_candidate_receipt_refuse() {
    let f = CompanionFixture::with_trim(false, 1);
    let error = api::prepare(f.request.clone(), f.inputs(), Duration::from_secs(30))
        .err()
        .unwrap();
    assert!(error.to_string().contains("whole frames"), "{error:#}");
    let mut r = f.request.clone();
    r.svg.bytes = 65537;
    assert!(
        api::prepare(r, f.inputs(), Duration::from_secs(30))
            .err()
            .unwrap()
            .to_string()
            .contains("byte profile")
    );
    drop(f);
    let mut f = CompanionFixture::new(false);
    let path = f.candidate.join("edition-000.wav");
    let mut bytes = fs::read(&path).unwrap();
    bytes[50] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let mut receipt: serde_json::Value = serde_json::from_slice(&f.receipt).unwrap();
    receipt["editions"][0]["wave"]["sha256"] = json!(hash(&bytes));
    receipt["editions"][0]["pcm_sha256"] = json!(hash(&bytes[44..]));
    f.receipt = serde_json::to_vec_pretty(&receipt).unwrap();
    let error = api::prepare(f.request.clone(), f.inputs(), Duration::from_secs(30))
        .err()
        .unwrap();
    assert!(error.to_string().contains("independent PCM"), "{error:#}");
}

#[cfg(windows)]
pub(super) mod native {
    use super::*;
    use reel::media_runtime::{Bindings, BoundPrograms, BoundTool, ByteIdentity};
    fn runtime() -> &'static (BoundPrograms, BoundTool, PathBuf) {
        static RUNTIME: std::sync::OnceLock<(BoundPrograms, BoundTool, PathBuf)> =
            std::sync::OnceLock::new();
        RUNTIME.get_or_init(|| {
            fn program(name: &str) -> FileRef {
                let path = PathBuf::from(
                    std::env::var_os(name)
                        .expect("explicit native companion runtime required; no successful skip"),
                );
                let identity = ByteIdentity::observe_file(&path).unwrap();
                FileRef {
                    path,
                    sha256: identity.sha256,
                    bytes: identity.bytes,
                }
            }
            let ffmpeg = program("REEL_TEST_FFMPEG");
            let ffprobe = program("REEL_TEST_FFPROBE");
            let browser = program("REEL_TEST_BROWSER");
            let bindings = Bindings {
                host: ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap(),
                ffmpeg: ffmpeg.clone(),
                ffprobe,
                dependencies: BTreeMap::from([("browser/program".into(), browser.clone())]),
            };
            (
                BoundPrograms::bind(&bindings).unwrap(),
                BoundTool::bind(&browser).unwrap(),
                ffmpeg.path,
            )
        })
    }

    pub(crate) fn render_shared_openings(
        inputs: reel::spoken_programme::installation::Inputs<'_>,
        root: &Path,
        candidate: &Path,
    ) {
        let request = composition_request(root, "installation");
        let visuals = root.to_path_buf();
        let prepared = api::prepare(
            request.clone(),
            api::Inputs {
                spoken: inputs.spoken,
                sources: inputs.sources,
                candidate: inputs.candidate,
                visuals: &visuals,
                access: inputs.access,
                access_manifest: inputs.access_manifest,
            },
            Duration::from_secs(30),
        )
        .unwrap();
        let receipt: producer::ProductionReceipt =
            serde_json::from_slice(inputs.spoken.receipt_bytes).unwrap();
        let (programs, browser, _) = runtime();
        assert_eq!(prepared.compositions().len(), 2);
        for (composition, expected_frames) in prepared.compositions().iter().zip([72, 84]) {
            let files = BrowserFixture::from_source(
                root,
                candidate,
                inputs.spoken.receipt_bytes,
                &request.svg,
                composition,
                false,
            );
            let workspace = tempfile::tempdir().unwrap();
            let output = workspace.path().join("shared-opening.mkv");
            let rendered = api::render(
                &prepared,
                composition.edition_id(),
                &files.plan,
                &files,
                programs,
                browser,
                workspace.path(),
                &output,
                || Ok(()),
            )
            .unwrap();
            let edition = receipt
                .editions
                .iter()
                .find(|edition| edition.edition_id == composition.edition_id())
                .unwrap();
            assert_eq!(rendered.frames, expected_frames);
            assert_eq!(
                rendered.pcm_sha256.as_deref(),
                Some(edition.pcm_sha256.as_str())
            );
        }
    }
    #[test]
    #[ignore = "explicit bounded native browser qualification in Windows core shard 2"]
    fn real_native_bilingual_svg_transitions_keep_exact_pcm_and_follow_replacement() {
        let (programs, browser, ffmpeg) = runtime();
        for v2 in [false, true] {
            let f = CompanionFixture::new(v2);
            let p = api::prepare(f.request.clone(), f.inputs(), Duration::from_secs(30)).unwrap();
            for (i, c) in p
                .compositions()
                .iter()
                .enumerate()
                .take(if v2 { 1 } else { 2 })
            {
                let files = BrowserFixture::new(&f, c, false);
                let workspace = tempfile::tempdir().unwrap();
                let output = workspace.path().join("companion.mkv");
                let result = api::render(
                    &p,
                    c.edition_id(),
                    &files.plan,
                    &files,
                    programs,
                    browser,
                    workspace.path(),
                    &output,
                    || Ok(()),
                )
                .unwrap();
                let (frames, a, b) = if i == 0 {
                    (72, if v2 { 25 } else { 24 }, 48)
                } else {
                    (84, 30, 60)
                };
                assert_eq!(result.frames, frames as u64);
                let decoded = Command::new(ffmpeg)
                    .args(["-v", "error", "-nostdin", "-threads", "1", "-i"])
                    .arg(&output)
                    .args([
                        "-map", "0:v:0", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1",
                    ])
                    .output()
                    .unwrap();
                assert!(
                    decoded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&decoded.stderr)
                );
                assert_eq!(decoded.stdout.len(), frames * 8 * 8 * 3);
                for (frame, rgb) in decoded.stdout.chunks_exact(8 * 8 * 3).enumerate() {
                    let dominant = if frame < a {
                        0
                    } else if frame < b {
                        1
                    } else {
                        2
                    };
                    for pixel in rgb.chunks_exact(3) {
                        assert!(
                            pixel[dominant] > 200
                                && pixel[(dominant + 1) % 3] < 30
                                && pixel[(dominant + 2) % 3] < 30,
                            "unexpected state: v2={v2}, edition={i}, frame={frame}, pixel={pixel:?}"
                        );
                    }
                }
                let pcm = Command::new(ffmpeg)
                    .args(["-v", "error", "-nostdin", "-threads", "1", "-i"])
                    .arg(&output)
                    .args([
                        "-map",
                        "0:a:0",
                        "-f",
                        "s24le",
                        "-c:a",
                        "pcm_s24le",
                        "pipe:1",
                    ])
                    .output()
                    .unwrap();
                assert!(pcm.status.success());
                assert_eq!(pcm.stdout, &wave(&f.f.signals[i])[44..]);
                assert_eq!(
                    result.pcm_sha256.as_deref(),
                    Some(hash(&pcm.stdout).as_str())
                );
                assert!(
                    api::render(
                        &p,
                        c.edition_id(),
                        &files.plan,
                        &files,
                        programs,
                        browser,
                        workspace.path(),
                        &output,
                        || Ok(())
                    )
                    .is_err()
                );
                assert_eq!(
                    hash(&fs::read(&output).unwrap()),
                    result.output.sha256.as_str()
                );
            }
        }
    }
    #[test]
    #[ignore = "explicit bounded native browser qualification in Windows core shard 2"]
    fn native_companion_cancellation_prevents_final_output() {
        let (programs, browser, _) = runtime();
        let f = CompanionFixture::new(false);
        let p = api::prepare(f.request.clone(), f.inputs(), Duration::from_secs(30)).unwrap();
        let c = &p.compositions()[0];
        let files = BrowserFixture::new(&f, c, false);
        let workspace = tempfile::tempdir().unwrap();
        let output = workspace.path().join("canceled.mkv");
        let mut entered_worker = false;
        assert!(
            api::render(
                &p,
                c.edition_id(),
                &files.plan,
                &files,
                programs,
                browser,
                workspace.path(),
                &output,
                || {
                    if fs::read_dir(workspace.path())?.next().is_some() {
                        entered_worker = true;
                        bail!("cancel admitted worker")
                    };
                    Ok(())
                }
            )
            .is_err()
        );
        assert!(entered_worker, "did not exercise worker interruption");
        assert!(!output.exists());
        assert!(fs::read_dir(workspace.path()).unwrap().next().is_none());
    }
}

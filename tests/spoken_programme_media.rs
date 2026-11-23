//! Controlled signals only; these tests do not establish speech/listening approval.
#[path = "support/spoken_checking.rs"]
mod checking;
#[cfg(feature = "tig-publication")]
#[path = "support/spoken_companion.rs"]
mod companion;
#[path = "support/spoken_installation.rs"]
mod installation;
#[path = "support/spoken_layers.rs"]
mod layers;
#[cfg(feature = "print")]
#[path = "support/spoken_lesson.rs"]
mod lesson;
#[path = "support/spoken_multicue.rs"]
mod qualification;
#[cfg(feature = "print")]
#[path = "support/spoken_reading.rs"]
mod reading;
use anyhow::{Result, bail};
use reel::spoken_programme::{
    Document,
    compiler::{self, CompilationProfile},
    producer::{self, CueProcessing, ProcessingPolicy, RationalGain},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// Test-owned encoder, deliberately separate from the production container code.
fn wave(values: &[i32]) -> Vec<u8> {
    let data: Vec<u8> = values
        .iter()
        .flat_map(|n| n.to_le_bytes()[..3].to_vec())
        .collect();
    let mut out = vec![0u8; 44];
    out[0..4].copy_from_slice(b"RIFF");
    out[4..8].copy_from_slice(&(data.len() as u32 + 36).to_le_bytes());
    out[8..16].copy_from_slice(b"WAVEfmt ");
    out[16..20].copy_from_slice(&16u32.to_le_bytes());
    out[20..22].copy_from_slice(&1u16.to_le_bytes());
    out[22..24].copy_from_slice(&2u16.to_le_bytes());
    out[24..28].copy_from_slice(&48000u32.to_le_bytes());
    out[28..32].copy_from_slice(&288000u32.to_le_bytes());
    out[32..34].copy_from_slice(&6u16.to_le_bytes());
    out[34..36].copy_from_slice(&24u16.to_le_bytes());
    out[36..40].copy_from_slice(b"data");
    out[40..44].copy_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

fn decode(bytes: &[u8]) -> Vec<i32> {
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(
        u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize,
        bytes.len() - 44
    );
    bytes[44..]
        .chunks_exact(3)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], if b[2] & 128 != 0 { 255 } else { 0 }]))
        .collect()
}

struct Fixture {
    temp: tempfile::TempDir,
    document: Document,
    signals: Vec<Vec<i32>>,
    started: std::time::Instant,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert!(
                self.started.elapsed() <= std::time::Duration::from_secs(60),
                "spoken fixture exceeded its measured 60-second outer budget"
            );
        }
    }
}

impl Fixture {
    fn new() -> Self {
        Self::with_temp(tempfile::tempdir().unwrap())
    }
    fn with_temp(temp: tempfile::TempDir) -> Self {
        let started = std::time::Instant::now();
        let document =
            serde_json::from_str(include_str!("fixtures/spoken-programme/four-editions.json"))
                .unwrap();
        let mut f = Self {
            temp,
            document,
            signals: vec![],
            started,
        };
        for (k, p) in f.document.performances.iter().enumerate() {
            let mut values = Vec::new();
            for i in 0..p.duration_samples {
                let v = if i < 2400 || i >= p.duration_samples - 4800 {
                    0
                } else {
                    1000 * (k as i32 + 1) + (i % 97) as i32 - 48
                };
                values.extend([v, -v]);
            }
            f.signals.push(values);
        }
        f.sync();
        f
    }

    fn sync(&mut self) {
        for (index, p) in self.document.performances.iter_mut().enumerate() {
            let bytes = wave(&self.signals[index]);
            p.source.path = format!("take-{index}.wav").into();
            p.source.sha256 = hash(&bytes);
            p.source.bytes = bytes.len() as u64;
            p.observed_text_sha256 = hash(b"");
            fs::write(self.temp.path().join(&p.source.path), bytes).unwrap();
        }
        self.sync_metadata();
    }

    fn sync_metadata(&mut self) {
        for (index, p) in self.document.performances.iter_mut().enumerate() {
            let evidence = serde_json::to_vec(&json!({
                "schema": compiler::ALIGNMENT_SCHEMA, "performance_id":p.id,
                "cue_id":p.cue_id, "language":p.language, "source":p.source,
                "sample_rate":48000, "duration_samples":p.duration_samples,
                "script_id":p.script_id, "script_sha256":p.script_sha256,
                "observation":{"kind":"engineering-signal"}, "markers":p.markers, "word_bindings":[]
            }))
            .unwrap();
            p.alignment.path = format!("alignment-{index}.json").into();
            p.alignment.sha256 = hash(&evidence);
            p.alignment.bytes = evidence.len() as u64;
            fs::write(self.temp.path().join(&p.alignment.path), evidence).unwrap();
        }
    }

    fn plan(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(
            &compiler::compile_with_inputs(
                &self.document,
                self.temp.path(),
                &producer::current_runtime().unwrap(),
                CompilationProfile::Engineering,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn output(&self) -> PathBuf {
        self.temp.path().join("candidate")
    }

    fn produce(&self, policy: &ProcessingPolicy) -> Result<producer::ProductionReceipt> {
        producer::produce(
            &self.document,
            &self.plan(),
            policy,
            self.temp.path(),
            &self.output(),
            || Ok(()),
        )
    }
}

#[test]
fn neutral_pcm_matches_independent_oracle() {
    let f = Fixture::new();
    let receipt = f.produce(&ProcessingPolicy::identity()).unwrap();
    let spans = [
        ("en-executive", 300000usize, vec![(0, 0), (2, 156000)]),
        (
            "en-technical",
            508800,
            vec![(0, 0), (1, 158400), (2, 364800)],
        ),
        ("es-executive", 352800, vec![(3, 0), (5, 184800)]),
        (
            "es-technical",
            590400,
            vec![(3, 0), (4, 187200), (5, 422400)],
        ),
    ];
    for (id, count, placements) in spans {
        let e = receipt
            .editions
            .iter()
            .find(|e| e.edition_id == id)
            .unwrap();
        let mut expected = vec![0; count * 2];
        for (source, start) in placements {
            let values = &f.signals[source];
            expected[start * 2..start * 2 + values.len()].copy_from_slice(values);
        }
        let bytes = fs::read(f.output().join(&e.wave.path)).unwrap();
        assert_eq!(decode(&bytes), expected, "{id}");
        assert_eq!(e.wave.sha256, hash(&bytes));
        assert!(
            e.takes
                .iter()
                .all(|t| t.selected_pcm_sha256 == t.processed_pcm_sha256)
        );
    }
    assert!(receipt.source_bytes_verified);
    assert!(
        !receipt.independent_check_passed
            && !receipt.authority_verified
            && !receipt.native_admission_verified
    );
    assert!(f.output().join("receipt.json").is_file());
}

#[test]
fn declared_gap_overlap_and_envelope_match_pcm() {
    let mut f = Fixture::new();
    // Only the final 1000 samples of protected tail are removed from this
    // controlled fixture, explicitly, so a declared overlap can be tested.
    // No real speech or protected word is altered.
    let edition = f
        .document
        .editions
        .iter_mut()
        .find(|e| e.id == "en-executive")
        .unwrap();
    edition.joins[0].added_pause_samples = 0;
    edition.joins[0].overlap_samples = 1000;
    for p in &mut f.document.performances {
        p.protected
            .retain(|r| !matches!(r.kind, reel::spoken_programme::ProtectedKind::Tail));
    }
    // Nonzero controlled prefix/suffix makes fade/overlap arithmetic observable.
    f.signals[0].fill(101);
    f.signals[2].fill(-203);
    f.sync();
    let policy = ProcessingPolicy {
        schema: producer::PROCESSING_SCHEMA.into(),
        operations: vec![CueProcessing {
            edition_id: "en-executive".into(),
            performance_id: f.document.performances[0].id.clone(),
            gain: RationalGain {
                numerator: 1,
                denominator: 2,
            },
            fade_in_samples: 100,
            fade_out_samples: 1000,
        }],
    };
    let receipt = f.produce(&policy).unwrap();
    let e = receipt
        .editions
        .iter()
        .find(|e| e.edition_id == "en-executive")
        .unwrap();
    let mut expected = vec![0; 287000 * 2];
    for i in 0..144000 {
        let gain = if i < 100 {
            50 * i as i32 / 100
        } else if i >= 143000 {
            50 * (143999 - i) as i32 / 1000
        } else {
            50
        };
        expected[i * 2] += gain;
        expected[i * 2 + 1] += gain;
        expected[(143000 + i) * 2] -= 203;
        expected[(143000 + i) * 2 + 1] -= 203;
    }
    assert_eq!(
        decode(&fs::read(f.output().join(&e.wave.path)).unwrap()),
        expected
    );
    assert_ne!(
        e.takes[0].selected_pcm_sha256,
        e.takes[0].processed_pcm_sha256
    );
}

#[test]
fn failure_never_publishes_candidate() {
    let f = Fixture::new();
    let plan = f.plan();
    let result = producer::produce(
        &f.document,
        &plan,
        &ProcessingPolicy::identity(),
        f.temp.path(),
        &f.output(),
        || {
            if f.output().join("edition-000.wav").exists() {
                bail!("injected interruption");
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!f.output().exists());
    fs::create_dir(f.output()).unwrap();
    fs::write(f.output().join("foreign"), b"preserve").unwrap();
    assert!(f.produce(&ProcessingPolicy::identity()).is_err());
    assert_eq!(fs::read(f.output().join("foreign")).unwrap(), b"preserve");
    assert!(!f.output().join("receipt.json").exists());
}

#[test]
fn changed_source_and_rehashed_unsupported_format_refuse() {
    let mut f = Fixture::new();
    let path = f.temp.path().join(&f.document.performances[0].source.path);
    let mut bytes = fs::read(&path).unwrap();
    bytes[90000] ^= 1;
    fs::write(&path, &bytes).unwrap();
    assert!(f.produce(&ProcessingPolicy::identity()).is_err());
    assert!(!f.output().exists());
    // Update the hash so this exercises actual format validation, not pin rejection.
    bytes[24..28].copy_from_slice(&44100u32.to_le_bytes());
    f.document.performances[0].source.sha256 = hash(&bytes);
    fs::write(&path, &bytes).unwrap();
    let p = &mut f.document.performances[0];
    let meta_path = f.temp.path().join(&p.alignment.path);
    let mut evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
    evidence["source"] = json!(p.source);
    let meta = serde_json::to_vec(&evidence).unwrap();
    p.alignment.sha256 = hash(&meta);
    p.alignment.bytes = meta.len() as u64;
    fs::write(meta_path, meta).unwrap();
    assert!(
        format!(
            "{:#}",
            f.produce(&ProcessingPolicy::identity()).unwrap_err()
        )
        .contains("PCM format invalid")
    );
    assert!(!f.output().exists());
}

#[test]
fn modified_plan_and_protected_fade_refuse() {
    let f = Fixture::new();
    let mut value: serde_json::Value = serde_json::from_slice(&f.plan()).unwrap();
    value["editions"][0]["cues"][0]["start_sample"] = json!(1);
    let altered = serde_json::to_vec_pretty(&value).unwrap();
    assert!(
        producer::produce(
            &f.document,
            &altered,
            &ProcessingPolicy::identity(),
            f.temp.path(),
            &f.output(),
            || Ok(())
        )
        .is_err()
    );
    let policy = ProcessingPolicy {
        schema: producer::PROCESSING_SCHEMA.into(),
        operations: vec![CueProcessing {
            edition_id: "en-executive".into(),
            performance_id: f.document.performances[0].id.clone(),
            gain: RationalGain {
                numerator: 1,
                denominator: 1,
            },
            fade_in_samples: 2401,
            fade_out_samples: 0,
        }],
    };
    assert!(f.produce(&policy).is_err());
    assert!(!f.output().exists());
}

#[test]
fn peak_limit_is_inclusive_and_one_sample_above_refuses() {
    let mut f = Fixture::new();
    f.signals[0][6000] = 4_194_303;
    f.sync();
    f.produce(&ProcessingPolicy::identity()).unwrap();
    // A new output location is used; no completed candidate is overwritten.
    f.signals[0][6000] = 4_194_304;
    f.sync();
    let second = f.temp.path().join("over-peak");
    let error = producer::produce(
        &f.document,
        &f.plan(),
        &ProcessingPolicy::identity(),
        f.temp.path(),
        &second,
        || Ok(()),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("exceeds peak profile"));
    assert!(!second.exists());
    assert!(f.output().join("receipt.json").exists());
}

#[test]
fn spoken_peak_rounds_down_to_the_at_most_minus_one_dbfs_boundary() {
    let mut f = Fixture::new();
    f.signals[0][6000] = 7_476_354;
    f.sync();
    let runtime = producer::current_runtime().unwrap();
    let make_plan = |f: &Fixture| {
        serde_json::to_vec_pretty(
            &compiler::compile_with_inputs(
                &f.document,
                f.temp.path(),
                &runtime,
                CompilationProfile::Spoken,
            )
            .unwrap(),
        )
        .unwrap()
    };
    producer::produce(
        &f.document,
        &make_plan(&f),
        &ProcessingPolicy::identity(),
        f.temp.path(),
        &f.output(),
        || Ok(()),
    )
    .unwrap();
    f.signals[0][6000] = 7_476_355;
    f.sync();
    let refused = f.temp.path().join("over-spoken-peak");
    let error = producer::produce(
        &f.document,
        &make_plan(&f),
        &ProcessingPolicy::identity(),
        f.temp.path(),
        &refused,
        || Ok(()),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("exceeds peak profile"));
    assert!(!refused.exists());
    assert!(f.output().join("receipt.json").exists());
}

#[test]
fn interruption_preserves_unrelated_files_in_reserved_directory() {
    let f = Fixture::new();
    let result = producer::produce(
        &f.document,
        &f.plan(),
        &ProcessingPolicy::identity(),
        f.temp.path(),
        &f.output(),
        || {
            if f.output().join("edition-000.wav").exists() {
                fs::write(f.output().join("foreign"), b"preserve")?;
                bail!("injected interruption");
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(f.output().join("foreign")).unwrap(), b"preserve");
    assert!(!f.output().join("edition-000.wav").exists());
    assert!(!f.output().join("receipt.json").exists());
}

#[test]
fn aggregate_budget_refuses_before_source_access() {
    let mut f = Fixture::new();
    // Six individually legal pins exceed the engineering aggregate cap.
    for p in &mut f.document.performances {
        p.source.bytes = 4 * 1024 * 1024;
        fs::remove_file(f.temp.path().join(&p.source.path)).unwrap();
    }
    f.sync_metadata();
    let error = f.produce(&ProcessingPolicy::identity()).unwrap_err();
    assert!(format!("{error:#}").contains("input budget exceeded"));
    assert!(!f.output().exists());
}

#[test]
fn narrowed_deadline_refuses_without_publishing() {
    let f = Fixture::new();
    let result = producer::produce_controlled(
        &f.document,
        &f.plan(),
        &ProcessingPolicy::identity(),
        f.temp.path(),
        &f.output(),
        producer::ProductionControl {
            timeout: std::time::Duration::ZERO,
            cancellation: || Ok(()),
        },
    );
    assert!(format!("{:#}", result.unwrap_err()).contains("deadline elapsed"));
    assert!(!f.output().exists());
}

#[test]
fn processing_targets_gains_and_envelopes_are_strict() {
    let f = Fixture::new();
    let valid = CueProcessing {
        edition_id: "en-executive".into(),
        performance_id: f.document.performances[0].id.clone(),
        gain: RationalGain {
            numerator: 1,
            denominator: 1,
        },
        fade_in_samples: 0,
        fade_out_samples: 0,
    };
    let mut variants = Vec::new();
    let mut bad = valid.clone();
    bad.gain.denominator = 0;
    variants.push(vec![bad]);
    let mut bad = valid.clone();
    bad.gain.numerator = 9;
    variants.push(vec![bad]);
    let mut bad = valid.clone();
    bad.performance_id = "unknown".into();
    variants.push(vec![bad]);
    let mut bad = valid.clone();
    bad.fade_out_samples = 144001;
    variants.push(vec![bad]);
    variants.push(vec![valid.clone(), valid]);
    let plan = f.plan();
    for operations in variants {
        let policy = ProcessingPolicy {
            schema: producer::PROCESSING_SCHEMA.into(),
            operations,
        };
        assert!(
            producer::produce(
                &f.document,
                &plan,
                &policy,
                f.temp.path(),
                &f.output(),
                || Ok(())
            )
            .is_err()
        );
        assert!(!f.output().exists());
    }
    assert!(
        producer::parse_processing(
            br#"{"schema":"reel.spoken-processing.v1","operations":[],"unknown":true}"#
        )
        .is_err()
    );
}

#[test]
fn cli_spoken_impact_rebuilds_only_editions_using_the_changed_english_take() {
    let before = Fixture::new();
    let mut after = Fixture::new();
    let number = after
        .document
        .performances
        .iter()
        .position(|take| take.id == "en-opening")
        .unwrap();
    after.signals[number][5000] += 1;
    after.sync();
    let write_declaration = |fixture: &Fixture| {
        let path = fixture.temp.path().join("declaration.json");
        fs::write(&path, serde_json::to_vec(&fixture.document).unwrap()).unwrap();
        path
    };
    let result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("spoken-edition-impact")
        .arg(write_declaration(&before))
        .arg(write_declaration(&after))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["schema"], "reel.spoken-edition-impact.v1");
    assert_eq!(report["reuse_requires_checked_retained_output"], true);
    let editions = report["editions"].as_array().unwrap();
    assert_eq!(editions.len(), 4);
    for (edition, action) in [
        ("en-executive", "rebuild"),
        ("en-technical", "rebuild"),
        ("es-executive", "reuse"),
        ("es-technical", "reuse"),
    ] {
        let row = editions
            .iter()
            .find(|row| row["edition_id"] == edition)
            .unwrap();
        assert_eq!(row["action"], action);
        assert_eq!(
            row["previous_recipe_sha256"] == row["current_recipe_sha256"],
            action == "reuse"
        );
    }
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(!stdout.contains("take-0.wav"));
    assert!(!stdout.contains("alignment-0.json"));
}

#[test]
fn cli_produces_and_checks_only_selected_spoken_editions() {
    let fixture = Fixture::new();
    let root = fixture.temp.path();
    let declaration = root.join("declaration.json");
    fs::write(&declaration, serde_json::to_vec(&fixture.document).unwrap()).unwrap();
    for take in &fixture.document.performances {
        if take.language == "es" {
            fs::remove_file(root.join(&take.source.path)).unwrap();
        }
    }
    let plan = root.join("scoped-plan.json");
    let candidate = fixture.output();
    let access = root.join("access");
    let invoke = |args: &[&str]| {
        let result = Command::new(env!("CARGO_BIN_EXE_reel"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    let declaration = declaration.to_str().unwrap();
    let plan = plan.to_str().unwrap();
    let candidate_path = candidate.to_str().unwrap();
    let root = root.to_str().unwrap();
    invoke(&[
        "spoken-edition-compile",
        declaration,
        "--output",
        plan,
        "--select-edition",
        "en-executive",
        "--select-edition",
        "en-technical",
    ]);
    invoke(&[
        "spoken-edition-produce",
        plan,
        "--declaration",
        declaration,
        "--asset-root",
        root,
        "--output",
        candidate_path,
    ]);
    invoke(&[
        "spoken-edition-check",
        declaration,
        plan,
        "--asset-root",
        root,
        "--candidate",
        candidate_path,
        "--write-access",
        access.to_str().unwrap(),
    ]);
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(candidate.join("receipt.json")).unwrap()).unwrap();
    let ids: Vec<_> = receipt["editions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edition| edition["edition_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["en-executive", "en-technical"]);
    assert!(!candidate.join("edition-002.wav").exists());
    assert!(access.is_dir());
}

#[test]
fn cli_compiles_produces_and_refuses_overwrite() {
    let mut f = Fixture::new();
    f.document.id = "PRIVATE-PROGRAMME-CANARY".into();
    let source = &mut f.document.performances[0].source;
    let private_name: PathBuf = "PRIVATE-SOURCE-CANARY.wav".into();
    fs::rename(
        f.temp.path().join(&source.path),
        f.temp.path().join(&private_name),
    )
    .unwrap();
    source.path = private_name;
    f.sync_metadata();
    let declaration = f.temp.path().join("declaration.json");
    fs::write(&declaration, serde_json::to_vec(&f.document).unwrap()).unwrap();
    let plan = f.temp.path().join("plan.json");
    let compile = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("spoken-edition-compile")
        .arg(&declaration)
        .arg("--output")
        .arg(&plan)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("spoken-edition-produce")
            .arg(&plan)
            .arg("--declaration")
            .arg(&declaration)
            .arg("--asset-root")
            .arg(f.temp.path())
            .arg("--output")
            .arg(f.output())
            .output()
            .unwrap()
    };
    let first = invoke();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let safe: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(safe["editions"], 4);
    assert!(!String::from_utf8_lossy(&first.stdout).contains(f.temp.path().to_str().unwrap()));
    let originals: BTreeMap<_, _> = fs::read_dir(f.output())
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            (path.clone(), fs::read(path).unwrap())
        })
        .collect();
    let second = invoke();
    assert!(!second.status.success());
    for output in [&first, &second] {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for canary in [
            "PRIVATE-PROGRAMME-CANARY",
            "PRIVATE-SOURCE-CANARY",
            "en-opening",
            "en-executive",
        ] {
            assert!(!combined.contains(canary));
        }
    }
    for (path, bytes) in originals {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

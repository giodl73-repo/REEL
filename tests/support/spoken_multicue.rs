//! Public signal qualification only: no recorded speech or listening approval.
use super::*;
use reel::spoken_programme::{
    self, Join, TakeUse,
    checker::{self, CheckRequest, CheckedPackage, EntryKind},
};

fn fixture() -> Fixture {
    let mut f = Fixture::new();
    let mut question = f.document.modules[1].cues[0].clone();
    question.id = "detail-question".into();
    question.speaker_id = "host".into();
    for script in &mut question.scripts {
        script.id = format!("{}-question", script.language);
        script.text = if script.language == "en" {
            "How does an edition choose its material?"
        } else {
            "¿Cómo elige una edición su contenido?"
        }
        .into();
        script.sha256 = hash(script.text.as_bytes());
    }
    for (index, duration) in [(0, 72000), (3, 96000)] {
        let mut performance = f.document.performances[index].clone();
        performance.id = format!("{}-question", performance.language);
        performance.module_id = "detail".into();
        performance.cue_id = question.id.clone();
        let script = question
            .scripts
            .iter()
            .find(|s| s.language == performance.language)
            .unwrap();
        performance.script_id = script.id.clone();
        performance.script_sha256 = script.sha256.clone();
        performance.duration_samples = duration;
        performance.protected[0].end_sample = duration - 4800;
        performance.protected[1].start_sample = duration - 4800;
        performance.protected[1].end_sample = duration;
        performance.scope.edition_ids = vec![format!("{}-technical", performance.language)];
        let code = f.signals.len() as i32 + 1;
        let signal = (0..duration)
            .flat_map(|i| {
                let left = if i < 2400 || i >= duration - 4800 {
                    0
                } else {
                    1000 * code + (i % 97) as i32 - 48
                };
                [left, -left]
            })
            .collect();
        f.signals.push(signal);
        f.document.performances.push(performance);
    }
    f.document.modules[1].cues.insert(0, question);
    f.document.modules[1].requires = vec!["opening".into()];
    for edition in f
        .document
        .editions
        .iter_mut()
        .filter(|e| e.audience == "technical")
    {
        let en = edition.language == "en";
        let question = format!("{}-question", edition.language);
        edition.selections[1].takes.insert(
            0,
            TakeUse {
                performance_id: question.clone(),
                start_sample: 0,
                end_sample: if en { 72000 } else { 96000 },
            },
        );
        edition.joins[0].to = question.clone();
        edition.joins.insert(
            1,
            Join {
                from: question,
                to: format!("{}-detail", edition.language),
                added_pause_samples: if en { 4800 } else { 7200 },
                overlap_samples: 0,
            },
        );
    }
    f.sync();
    f
}

fn check(f: &Fixture, plan: &[u8]) -> Result<CheckedPackage> {
    checker::check(
        CheckRequest {
            document: &f.document,
            plan_bytes: plan,
            receipt_bytes: &fs::read(f.output().join("receipt.json"))?,
            processing: &ProcessingPolicy::identity(),
            expected_runtime: &producer::current_runtime()?,
        },
        f.temp.path(),
        &f.output(),
        producer::ProductionControl {
            timeout: std::time::Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
}

fn assert_pack(f: &Fixture, reordered: bool) {
    let plan = f.plan();
    fs::write(
        f.temp.path().join("declaration.json"),
        serde_json::to_vec_pretty(&f.document).unwrap(),
    )
    .unwrap();
    fs::write(f.temp.path().join("plan.json"), &plan).unwrap();
    let reopened =
        spoken_programme::parse(&fs::read(f.temp.path().join("declaration.json")).unwrap())
            .unwrap();
    assert_eq!(
        serde_json::to_value(reopened).unwrap(),
        serde_json::to_value(&f.document).unwrap()
    );
    let receipt = f.produce(&ProcessingPolicy::identity()).unwrap();
    let mut checked = check(f, &fs::read(f.temp.path().join("plan.json")).unwrap()).unwrap();
    let access_root = f.temp.path().join("access");
    checker::write_access(&mut checked, &access_root, || Ok(())).unwrap();
    checker::verify_access(
        &mut checked,
        &fs::read(access_root.join("access.json")).unwrap(),
        &access_root,
        || Ok(()),
    )
    .unwrap();
    for edition in &checked.access().editions {
        let (duration, placements, speakers): (usize, Vec<(usize, usize)>, Vec<u16>) =
            match (edition.edition_id.as_str(), reordered) {
                ("en-executive", _) => (300000, vec![(0, 0), (2, 156000)], vec![1, 1]),
                ("es-executive", _) => (352800, vec![(3, 0), (5, 184800)], vec![1, 1]),
                ("en-technical", false) => (
                    585600,
                    vec![(0, 0), (6, 158400), (1, 235200), (2, 441600)],
                    vec![1, 1, 2, 1],
                ),
                ("es-technical", false) => (
                    693600,
                    vec![(3, 0), (7, 187200), (4, 290400), (5, 525600)],
                    vec![1, 1, 2, 1],
                ),
                ("en-technical", true) => (
                    573600,
                    vec![(0, 0), (2, 153600), (6, 304800), (1, 381600)],
                    vec![1, 1, 1, 2],
                ),
                ("es-technical", true) => (
                    676800,
                    vec![(3, 0), (5, 180000), (7, 357600), (4, 460800)],
                    vec![1, 1, 1, 2],
                ),
                _ => panic!("unknown fixture edition"),
            };
        assert_eq!(edition.duration_samples, duration as u64);
        let mut expected = vec![0; duration * 2];
        for &(source, start) in &placements {
            let signal = &f.signals[source];
            expected[start * 2..start * 2 + signal.len()].copy_from_slice(signal);
        }
        let media = receipt
            .editions
            .iter()
            .find(|e| e.edition_id == edition.edition_id)
            .unwrap();
        assert_eq!(
            decode(&fs::read(f.output().join(&media.wave.path)).unwrap()),
            expected
        );
        let entries = edition
            .entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::EngineeringSignal))
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), placements.len());
        assert!(edition.word_evidence.is_empty());
        assert_eq!(
            edition
                .entries
                .iter()
                .filter(|e| matches!(e.kind, EntryKind::AddedPause))
                .count(),
            placements.len() - 1
        );
        for ((entry, &(source, start)), &speaker) in entries.iter().zip(&placements).zip(&speakers)
        {
            let performance = &f.document.performances[source];
            assert_eq!(
                (entry.start_sample, entry.end_sample, entry.speaker),
                (
                    start as u64,
                    start as u64 + performance.duration_samples,
                    Some(speaker)
                )
            );
            assert_eq!(entry.text, "[Engineering signal; no spoken-word evidence]");
            assert!(entry.text_basis.is_none());
        }
        let chapters = edition
            .chapters
            .iter()
            .map(|c| (c.module_id.as_str(), c.start_sample, c.end_sample))
            .collect::<Vec<_>>();
        let expected_chapters = match (edition.edition_id.as_str(), reordered) {
            ("en-executive", _) => vec![("opening", 0, 144000), ("closing", 156000, 300000)],
            ("es-executive", _) => vec![("opening", 0, 168000), ("closing", 184800, 352800)],
            ("en-technical", false) => vec![
                ("opening", 0, 144000),
                ("detail", 158400, 427200),
                ("closing", 441600, 585600),
            ],
            ("es-technical", false) => vec![
                ("opening", 0, 168000),
                ("detail", 187200, 506400),
                ("closing", 525600, 693600),
            ],
            ("en-technical", true) => vec![
                ("opening", 0, 144000),
                ("closing", 153600, 297600),
                ("detail", 304800, 573600),
            ],
            ("es-technical", true) => vec![
                ("opening", 0, 168000),
                ("closing", 180000, 348000),
                ("detail", 357600, 676800),
            ],
            _ => unreachable!(),
        };
        assert_eq!(chapters, expected_chapters);
        let transcript = fs::read_to_string(access_root.join(&edition.transcript.path)).unwrap();
        assert!(transcript.contains("Speaker 1:"));
        assert!(transcript.contains("Engineering signal; no spoken-word evidence"));
    }
    assert!(checked.summary().access_checked && checked.summary().independent_pcm_check_passed);
    assert!(
        !checked.summary().authority_verified && !checked.summary().acoustic_word_fidelity_verified
    );
}

#[test]
fn module_with_two_speakers_matches_sample_and_access_goldens() {
    let baseline = Fixture::new();
    let baseline_plan: serde_json::Value = serde_json::from_slice(&baseline.plan()).unwrap();
    let f = fixture();
    let variant: serde_json::Value = serde_json::from_slice(&f.plan()).unwrap();
    for original in baseline_plan["editions"].as_array().unwrap() {
        let after = variant["editions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["recipe"]["edition_id"] == original["recipe"]["edition_id"])
            .unwrap();
        let technical = original["recipe"]["edition_id"]
            .as_str()
            .unwrap()
            .ends_with("technical");
        assert_eq!(
            original["recipe_sha256"] != after["recipe_sha256"],
            technical
        );
    }
    assert_pack(&f, false);
}

#[test]
fn multicue_adjacency_scope_and_protected_regions_refuse_before_output() {
    let f = fixture();
    for mutation in 0..5 {
        let mut doc = f.document.clone();
        let technical = doc
            .editions
            .iter()
            .position(|e| e.id == "en-technical")
            .unwrap();
        let executive = doc
            .editions
            .iter()
            .position(|e| e.id == "en-executive")
            .unwrap();
        match mutation {
            0 => doc.editions[technical].selections[1].takes.reverse(),
            1 => {
                doc.editions[technical].selections[1].takes.remove(0);
            }
            2 => doc.editions[executive].joins[0].from = "en-detail".into(),
            3 => {
                let join = &mut doc.editions[technical].joins[1];
                join.added_pause_samples = 0;
                join.overlap_samples = 4800;
            }
            _ => doc.modules[2].requires.push("detail".into()),
        }
        assert!(spoken_programme::validate(&doc).is_err());
        assert!(
            compiler::compile_with_inputs(
                &doc,
                f.temp.path(),
                &producer::current_runtime().unwrap(),
                CompilationProfile::Engineering
            )
            .is_err()
        );
        assert!(!f.output().exists());
    }
}

#[test]
fn reordered_modules_need_fresh_controls_and_checked_adjacency() {
    let mut f = fixture();
    let original = f.document.clone();
    let original_plan = f.plan();
    for edition in f
        .document
        .editions
        .iter_mut()
        .filter(|e| e.audience == "technical")
    {
        edition.selections.swap(1, 2);
        let lang = &edition.language;
        let en = lang == "en";
        edition.joins = [
            ("opening", "closing", if en { 9600 } else { 12000 }),
            ("closing", "question", if en { 7200 } else { 9600 }),
            ("question", "detail", if en { 4800 } else { 7200 }),
        ]
        .into_iter()
        .map(|(from, to, pause)| Join {
            from: format!("{lang}-{from}"),
            to: format!("{lang}-{to}"),
            added_pause_samples: pause,
            overlap_samples: 0,
        })
        .collect();
    }
    assert_pack(&f, true);
    assert!(
        checker::check(
            CheckRequest {
                document: &original,
                plan_bytes: &original_plan,
                receipt_bytes: &fs::read(f.output().join("receipt.json")).unwrap(),
                processing: &ProcessingPolicy::identity(),
                expected_runtime: &producer::current_runtime().unwrap(),
            },
            f.temp.path(),
            &f.output(),
            producer::ProductionControl {
                timeout: std::time::Duration::from_secs(30),
                cancellation: || Ok(()),
            }
        )
        .is_err()
    );
}

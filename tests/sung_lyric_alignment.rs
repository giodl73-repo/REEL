use reel::sung_lyric_alignment::*;

fn request(indices: &[u32]) -> AlignmentRequest {
    let canonical = (1..=6)
        .map(|index| CanonicalSyllable {
            index,
            line_id: "line-1".into(),
            word: format!("w{index}"),
            printed: format!("s{index}"),
            normalized: format!("s{index}"),
            phones: vec![format!("p{index}")],
            vowel_nucleus: "a".into(),
        })
        .collect();
    let score_events = (1..=6)
        .map(|index| ScoreEvent {
            id: format!("note-{index}"),
            order: index,
            measure: "1".into(),
            voice: "1".into(),
            kind: "note".into(),
            start_ms: None,
            end_ms: None,
            canonical_indices: vec![index],
        })
        .collect();
    let evidence = indices
        .iter()
        .enumerate()
        .map(|(n, index)| PerformedEvidence {
            id: format!("ev-{n}"),
            start_ms: n as u64 * 100,
            end_ms: n as u64 * 100 + 90,
            class: "lyric".into(),
            normalized: Some(format!("s{index}")),
            phones: vec![format!("p{index}")],
            vowel_nucleus: Some("a".into()),
            candidates: vec![
                EvidenceCandidate {
                    canonical_index: *index,
                    confidence_micros: 900_000,
                },
                EvidenceCandidate {
                    canonical_index: if *index == 6 { 5 } else { *index + 1 },
                    confidence_micros: 100_000,
                },
            ],
            sources: vec![EvidenceSource {
                adapter: "synthetic-ctc".into(),
                observation_id: format!("obs-{n}"),
                confidence_micros: 900_000,
            }],
        })
        .collect();
    AlignmentRequest {
        schema: "reel.sung-lyric-alignment.request.v1".into(),
        recording_sha256: "a".repeat(64),
        score_sha256: "b".repeat(64),
        clock: ClockTransform {
            offset_ms: 0,
            rate_num: 1,
            rate_den: 1,
        },
        canonical,
        score_events,
        evidence,
        evidence_streams: vec![],
        anchors: vec![],
        resolve: None,
    }
}

#[test]
fn repeat_timing_segments_accumulate_across_two_completed_loops() {
    let mut input = request(&[1, 2, 3, 1, 2, 3, 4, 5, 4, 5, 6]);
    for (position, score) in input.score_events.iter_mut().enumerate() {
        score.start_ms = Some(position as u64 * 100);
        score.end_ms = Some(position as u64 * 100 + 90);
    }
    let document = align(&input).unwrap();
    let offsets: Vec<_> = document
        .timing_segments
        .iter()
        .map(|segment| {
            (
                segment.opened_by_repeat,
                segment.cumulative_repeat_offset_ms,
            )
        })
        .collect();
    assert_eq!(
        offsets,
        vec![
            (false, 0),
            (true, 0),
            (false, 300),
            (true, 300),
            (false, 500)
        ]
    );
    assert_eq!(
        document
            .timing_segments
            .iter()
            .map(|segment| segment.comparison_offset_ms)
            .collect::<Vec<_>>(),
        vec![0, 300, 300, 500, 500]
    );
    assert_eq!(document.recommended[6].recording_score_offset_ms, Some(300));
    assert_eq!(
        document.recommended[10].recording_score_offset_ms,
        Some(500)
    );
    assert_eq!(
        document.ranked_recommendations[0].timing_segments,
        document.timing_segments
    );
    assert!(document.validation.timing_within_500ms);
}

#[test]
fn bounded_resolve_recomputes_repeat_offsets_without_moving_locked_exterior() {
    let mut input = request(&[1, 2, 3, 1, 2, 3, 4, 5, 4, 5, 6]);
    for (position, score) in input.score_events.iter_mut().enumerate() {
        score.start_ms = Some(position as u64 * 100);
        score.end_ms = Some(position as u64 * 100 + 90);
    }
    input.anchors.push(HumanAnchor {
        correction_id: "locked-left".into(),
        evidence_id: "ev-0".into(),
        canonical_index: 1,
        reviewer: "owner".into(),
        created_at: "2026-09-06T00:00:00Z".into(),
    });
    input.resolve = Some(ResolveScope {
        first_evidence_id: "ev-1".into(),
        last_evidence_id: "ev-10".into(),
    });
    let document = align(&input).unwrap();
    assert!(document.recommended[0].locked_human);
    assert_eq!(document.recommended[0].recording_score_offset_ms, Some(0));
    assert_eq!(
        document.recommended[10].recording_score_offset_ms,
        Some(500)
    );
    assert_eq!(
        document
            .timing_segments
            .last()
            .unwrap()
            .cumulative_repeat_offset_ms,
        500
    );
}

#[test]
fn preserves_forced_phone_evidence_without_claiming_independent_identity() {
    let mut input = request(&[1, 2, 3]);
    input.evidence[1].phones.clear();
    input.evidence[1].vowel_nucleus = None;
    input.evidence_streams.push(EvidenceStream {
        adapter: "synthetic-mfa-phones".into(),
        kind: Some("forced_alignment".into()),
        stream_sha256: "c".repeat(64),
        clock: ClockTransform {
            offset_ms: 0,
            rate_num: 1,
            rate_den: 1,
        },
        observations: vec![AdapterObservation {
            observation_id: "phone-2".into(),
            evidence_id: Some("ev-1".into()),
            start_ms: 100,
            end_ms: 180,
            confidence_micros: 930_000,
            normalized: Some("s2".into()),
            phones: vec!["p2".into()],
            vowel_nucleus: Some("a".into()),
            candidates: vec![EvidenceCandidate {
                canonical_index: 2,
                confidence_micros: 930_000,
            }],
        }],
    });
    let document = align(&input).unwrap();
    assert_eq!(
        document.ranked_recommendations[0].canonical_path,
        vec![Some(1), Some(2), Some(3)]
    );
    assert!(document.ranked_recommendations.len() >= 2);
    assert!(
        !document.recommended[1]
            .evidence
            .iter()
            .any(|e| e.adapter == "synthetic-mfa-phones")
    );
    assert_eq!(document.evidence_streams[0].kind, "forced_alignment");
    assert_eq!(
        document.evidence_streams[0].observations[0].phones,
        vec!["p2"]
    );
    assert_eq!(document.recommended[1].phones, vec!["p2"]);
}

fn word_stream(kind: &str, adapter: &str, words: &[(&str, usize)]) -> EvidenceStream {
    EvidenceStream {
        adapter: adapter.into(),
        kind: Some(kind.into()),
        stream_sha256: "d".repeat(64),
        clock: ClockTransform {
            offset_ms: 0,
            rate_num: 1,
            rate_den: 1,
        },
        observations: words
            .iter()
            .enumerate()
            .map(|(n, (word, evidence))| AdapterObservation {
                observation_id: format!("{adapter}-{n}"),
                evidence_id: Some(format!("ev-{evidence}")),
                start_ms: *evidence as u64 * 100,
                end_ms: *evidence as u64 * 100 + 90,
                confidence_micros: 950_000,
                normalized: Some((*word).into()),
                phones: vec![],
                vowel_nucleus: None,
                candidates: vec![],
            })
            .collect(),
    }
}

#[test]
fn independent_asr_seeds_first_last_and_repeated_anchor_islands() {
    let mut input = request(&[1, 2, 3, 4, 5]);
    let repeated = input.evidence.clone();
    for (n, mut event) in repeated.into_iter().enumerate() {
        event.id = format!("ev-{}", n + 5);
        event.start_ms += 500;
        event.end_ms += 500;
        input.evidence.push(event);
    }
    for (n, syllable) in input.canonical.iter_mut().enumerate() {
        syllable.word = ["sol", "sobre", "mar", "canta", "hoy", "luz"][n].into();
    }
    input.evidence_streams.push(word_stream(
        "independent_asr",
        "local-asr",
        &[
            ("sol", 0),
            ("sobre", 1),
            ("mar", 2),
            ("canta", 3),
            ("hoy", 4),
            ("sol", 5),
            ("sobre", 6),
            ("mar", 7),
            ("canta", 8),
            ("hoy", 9),
        ],
    ));
    let document = align(&input).unwrap();
    assert!(
        document
            .anchor_islands
            .iter()
            .any(|i| i.first_evidence_id == "ev-0")
    );
    assert!(
        document
            .anchor_islands
            .iter()
            .any(|i| i.last_evidence_id == "ev-9")
    );
    let repeated: Vec<_> = document
        .anchor_islands
        .iter()
        .filter(|i| i.canonical_first == 1 && i.canonical_last == 3)
        .map(|i| i.occurrence)
        .collect();
    assert_eq!(repeated, vec![1, 2]);
    assert_eq!(document.evidence_streams[0].kind, "independent_asr");
}

#[test]
fn forced_transcript_phones_and_activity_cannot_create_anchor_over_accompaniment() {
    let mut input = request(&[1, 2, 3]);
    input.evidence_streams.push(word_stream(
        "forced_alignment",
        "mfa-transcript-forced",
        &[("w1", 0), ("w2", 1), ("w3", 2)],
    ));
    input.evidence_streams.push(word_stream(
        "acoustic_activity",
        "vocal-energy",
        &[("w1", 0), ("w2", 1), ("w3", 2)],
    ));
    let document = align(&input).unwrap();
    assert!(document.anchor_islands.is_empty());
    assert_eq!(document.evidence_streams.len(), 2);
}

#[test]
fn non_unique_asr_phrase_is_surfaced_as_ambiguous_not_anchored() {
    let mut input = request(&[1, 2, 3, 4]);
    input.canonical[0].word = "de".into();
    input.canonical[1].word = "mar".into();
    input.canonical[2].word = "de".into();
    input.canonical[3].word = "mar".into();
    input.evidence_streams.push(word_stream(
        "independent_asr",
        "local-asr",
        &[("de", 0), ("mar", 1)],
    ));
    let document = align(&input).unwrap();
    assert!(document.anchor_islands.is_empty());
    assert_eq!(document.ambiguous_ngrams[0].canonical_match_count, 2);
}

#[test]
fn unassociated_independent_asr_island_repairs_multi_second_stale_timing() {
    let mut input = request(&[1, 2, 3]);
    input.canonical[0].word = "canta".into();
    input.canonical[1].word = "sobre".into();
    input.canonical[2].word = "mar".into();
    let mut stream = word_stream(
        "independent_asr",
        "fresh-asr",
        &[("canta", 0), ("sobre", 1), ("mar", 2)],
    );
    for (n, observation) in stream.observations.iter_mut().enumerate() {
        observation.evidence_id = None;
        observation.start_ms = 4_000 + n as u64 * 200;
        observation.end_ms = observation.start_ms + 150;
    }
    input.evidence_streams.push(stream);
    let document = align(&input).unwrap();
    assert_eq!(document.recommended[0].start_ms, 4_000);
    assert_eq!(document.recommended[2].start_ms, 4_400);
    assert_eq!(document.anchor_islands.len(), 1);
    assert_eq!(document.anchor_islands[0].canonical_first, 1);
    assert_eq!(document.anchor_islands[0].canonical_last, 3);
}

#[test]
fn asr_words_bind_full_unequal_syllable_spans_and_distribute_word_time() {
    let mut input = request(&[1, 2, 3, 4, 5, 6, 6]);
    input.canonical = [
        (1, "Cantaré", "can"),
        (2, "Cantaré", "ta"),
        (3, "Cantaré", "ré"),
        (4, "a", "a"),
        (5, "la", "la"),
        (6, "playa", "pla"),
        (7, "playa", "ya"),
    ]
    .into_iter()
    .map(|(index, word, syllable)| CanonicalSyllable {
        index,
        line_id: "line-1".into(),
        word: word.into(),
        printed: syllable.into(),
        normalized: syllable.to_ascii_lowercase(),
        phones: vec![format!("p{index}")],
        vowel_nucleus: "a".into(),
    })
    .collect();
    input.score_events = (1..=7)
        .map(|index| ScoreEvent {
            id: format!("note-{index}"),
            order: index,
            measure: "1".into(),
            voice: "1".into(),
            kind: "note".into(),
            start_ms: None,
            end_ms: None,
            canonical_indices: vec![index],
        })
        .collect();
    input.evidence = (1..=7)
        .map(|index| PerformedEvidence {
            id: format!("span-{index}"),
            start_ms: index as u64 * 100,
            end_ms: index as u64 * 100 + 90,
            class: "lyric".into(),
            normalized: None,
            phones: vec![],
            vowel_nucleus: None,
            candidates: vec![EvidenceCandidate {
                canonical_index: index,
                confidence_micros: 900_000,
            }],
            sources: vec![EvidenceSource {
                adapter: "candidate".into(),
                observation_id: format!("candidate-{index}"),
                confidence_micros: 900_000,
            }],
        })
        .collect();
    let mut stream = word_stream(
        "independent_asr",
        "word-span-asr",
        &[("cantaré", 0), ("a", 0), ("la", 0)],
    );
    for observation in &mut stream.observations {
        observation.evidence_id = None;
    }
    stream.observations[0].start_ms = 4_000;
    stream.observations[0].end_ms = 4_900;
    stream.observations[1].start_ms = 4_900;
    stream.observations[1].end_ms = 5_000;
    stream.observations[2].start_ms = 5_000;
    stream.observations[2].end_ms = 5_100;
    input.evidence_streams.push(stream);
    let mut closing_stream = word_stream(
        "independent_asr",
        "closing-word-span-asr",
        &[("a", 0), ("la", 0), ("playa", 0)],
    );
    for observation in &mut closing_stream.observations {
        observation.evidence_id = None;
    }
    closing_stream.observations[0].start_ms = 4_900;
    closing_stream.observations[0].end_ms = 5_000;
    closing_stream.observations[1].start_ms = 5_000;
    closing_stream.observations[1].end_ms = 5_100;
    closing_stream.observations[2].start_ms = 5_100;
    closing_stream.observations[2].end_ms = 5_700;
    input.evidence_streams.push(closing_stream);

    let first = align(&input).unwrap();
    let second = align(&input).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.anchor_islands[0].canonical_first, 1);
    assert_eq!(first.anchor_islands[0].canonical_last, 5);
    assert_eq!(
        first.recommended[..5]
            .iter()
            .map(|event| (event.canonical_index, event.start_ms, event.end_ms))
            .collect::<Vec<_>>(),
        vec![
            (Some(1), 4_000, 4_300),
            (Some(2), 4_300, 4_600),
            (Some(3), 4_600, 4_900),
            (Some(4), 4_900, 5_000),
            (Some(5), 5_000, 5_100),
        ]
    );
    assert_eq!(first.anchor_islands[1].canonical_first, 4);
    assert_eq!(first.anchor_islands[1].canonical_last, 7);
    assert_eq!(
        first.recommended[5..7]
            .iter()
            .map(|event| (event.canonical_index, event.start_ms, event.end_ms))
            .collect::<Vec<_>>(),
        vec![(Some(6), 5_100, 5_400), (Some(7), 5_400, 5_700)]
    );
    assert!(first.recommended[..5].iter().all(|event| {
        event
            .evidence
            .iter()
            .any(|source| source.adapter == "word-span-asr")
    }));
}

#[test]
fn low_confidence_or_misspelled_asr_words_do_not_lock_syllables() {
    let mut input = request(&[1, 2, 3]);
    input.canonical[0].word = "canta".into();
    input.canonical[1].word = "sobre".into();
    input.canonical[2].word = "mar".into();
    let mut stream = word_stream(
        "independent_asr",
        "uncertain-asr",
        &[("kanta", 0), ("sobre", 1), ("mar", 2)],
    );
    stream.observations[1].confidence_micros = 849_999;
    input.evidence_streams.push(stream);
    let document = align(&input).unwrap();
    assert!(document.anchor_islands.is_empty());
    assert_eq!(document.recommended[0].start_ms, 0);
}

#[test]
fn bounded_resolve_requires_locked_neighbors_and_preserves_them() {
    let mut input = request(&[1, 2, 3, 4]);
    input.anchors = vec![
        HumanAnchor {
            correction_id: "left".into(),
            evidence_id: "ev-0".into(),
            canonical_index: 1,
            reviewer: "owner".into(),
            created_at: "2026-09-06T00:00:00Z".into(),
        },
        HumanAnchor {
            correction_id: "right".into(),
            evidence_id: "ev-3".into(),
            canonical_index: 4,
            reviewer: "owner".into(),
            created_at: "2026-09-06T00:00:00Z".into(),
        },
    ];
    input.resolve = Some(ResolveScope {
        first_evidence_id: "ev-1".into(),
        last_evidence_id: "ev-2".into(),
    });
    let document = align(&input).unwrap();
    let scope = document.resolved_scope.unwrap();
    assert_eq!((scope.first_position, scope.last_position), (1, 2));
    assert!(document.recommended[0].locked_human);
    assert!(document.recommended[3].locked_human);
}

#[test]
fn repeat_loop_points_to_canonical_identity_and_resynchronizes() {
    let document = align(&request(&[1, 2, 3, 1, 2, 3, 4, 5, 6])).unwrap();
    let mapped: Vec<_> = document
        .recommended
        .iter()
        .map(|e| (e.canonical_index.unwrap(), e.occurrence.unwrap()))
        .collect();
    assert_eq!(
        mapped,
        vec![
            (1, 1),
            (2, 1),
            (3, 1),
            (1, 2),
            (2, 2),
            (3, 2),
            (4, 1),
            (5, 1),
            (6, 1)
        ]
    );
    assert!(
        document.recommended[3]
            .review_reasons
            .contains(&"performed-repeat".to_string())
    );
    assert!(document.coverage.omitted_indices.is_empty());
}

#[test]
fn human_anchor_is_immutable_and_output_is_deterministic() {
    let mut input = request(&[1, 2]);
    input.anchors.push(HumanAnchor {
        correction_id: "correction-1".into(),
        evidence_id: "ev-1".into(),
        canonical_index: 3,
        reviewer: "owner".into(),
        created_at: "2026-09-05T00:00:00Z".into(),
    });
    let first = align(&input).unwrap();
    let second = align(&input).unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_eq!(first.recommended[1].canonical_index, Some(3));
    assert!(first.recommended[1].locked_human);
}

#[test]
fn rejects_lyric_candidates_on_confirmed_silence() {
    let mut input = request(&[1]);
    input.evidence[0].class = "silence".into();
    assert!(
        align(&input)
            .unwrap_err()
            .to_string()
            .contains("cannot carry lyric candidates")
    );
}

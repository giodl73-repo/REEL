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
fn imports_phone_evidence_and_ranks_complete_song_paths() {
    let mut input = request(&[1, 2, 3]);
    input.evidence[1].phones.clear();
    input.evidence[1].vowel_nucleus = None;
    input.evidence_streams.push(EvidenceStream {
        adapter: "synthetic-mfa-phones".into(),
        stream_sha256: "c".repeat(64),
        clock: ClockTransform {
            offset_ms: 0,
            rate_num: 1,
            rate_den: 1,
        },
        observations: vec![AdapterObservation {
            observation_id: "phone-2".into(),
            evidence_id: "ev-1".into(),
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
        document.recommended[1]
            .evidence
            .iter()
            .any(|e| e.adapter == "synthetic-mfa-phones")
    );
    assert_eq!(document.recommended[1].phones, vec!["p2"]);
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

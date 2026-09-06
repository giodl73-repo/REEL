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
        anchors: vec![],
    }
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

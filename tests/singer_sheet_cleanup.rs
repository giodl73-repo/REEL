use reel::singer_sheet_cleanup::*;

fn request(key_fifths: i8, prefix: &str) -> CleanupRequest {
    CleanupRequest {
        schema: "reel.singer-sheet-cleanup.request.v1".into(),
        draft_musicxml_sha256: "a".repeat(64),
        isolated_vocal_sha256: "b".repeat(64),
        key_fifths,
        tempo_bpm: 120,
        divisions_per_quarter: 480,
        canonical: vec![
            CanonicalSyllable {
                index: 1,
                word_id: format!("{prefix}-word"),
                printed: "lu".into(),
                word_position: "begin".into(),
                vowel: "u".into(),
            },
            CanonicalSyllable {
                index: 2,
                word_id: format!("{prefix}-word"),
                printed: "na".into(),
                word_position: "end".into(),
                vowel: "a".into(),
            },
        ],
        occurrences: vec![
            PerformedOccurrence {
                occurrence_id: format!("{prefix}-1"),
                canonical_index: 1,
                start_ms: 100,
                end_ms: 1_100,
            },
            PerformedOccurrence {
                occurrence_id: format!("{prefix}-2"),
                canonical_index: 2,
                start_ms: 1_250,
                end_ms: 1_750,
            },
        ],
        vocal_observations: vec![
            VocalObservation {
                occurrence_id: format!("{prefix}-1"),
                start_ms: 100,
                end_ms: 1_100,
                median_midi_micros: 61_000_000,
                energy_micros: 800_000,
            },
            VocalObservation {
                occurrence_id: format!("{prefix}-2"),
                start_ms: 1_250,
                end_ms: 1_750,
                median_midi_micros: 63_000_000,
                energy_micros: 700_000,
            },
        ],
        draft_notes: vec![
            DraftNote {
                note_id: format!("{prefix}-draft-1"),
                occurrence_id: Some(format!("{prefix}-1")),
                midi: Some(60),
                duration_divisions: 37,
            },
            DraftNote {
                note_id: format!("{prefix}-draft-2"),
                occurrence_id: Some(format!("{prefix}-1")),
                midi: Some(61),
                duration_divisions: 80,
            },
            DraftNote {
                note_id: format!("{prefix}-draft-3"),
                occurrence_id: Some(format!("{prefix}-2")),
                midi: Some(63),
                duration_divisions: 480,
            },
        ],
    }
}

#[test]
fn recommends_readable_underlay_melisma_pauses_and_exact_clock() {
    let document = cleanup(&request(-2, "first-song")).unwrap();
    assert_eq!(document.exact_performance_duration_ms, 1_750);
    assert_eq!(document.metrics.exact_clock_coverage_micros, 1_000_000);
    assert_eq!(document.metrics.tiny_durations_before, 2);
    assert_eq!(document.metrics.tiny_durations_after, 0);
    assert_eq!(document.notation[1].pitch_spelling.as_deref(), Some("Db4"));
    assert_eq!(document.notation[1].syllabic.as_deref(), Some("begin"));
    assert_eq!(document.notation[1].held_vowel.as_deref(), Some("u"));
    assert!(document.notation[1].melisma);
    assert_eq!(
        document.notation[2].pause_kind.as_deref(),
        Some("breath-mark")
    );
    assert!(
        document.ledger[0]
            .changes
            .contains(&"duration-normalized".into())
    );
}

#[test]
fn cold_second_song_is_deterministic_and_uses_key_aware_sharps() {
    let input = request(3, "second-song");
    let first = cleanup(&input).unwrap();
    let second = cleanup(&input).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.notation[1].pitch_spelling.as_deref(), Some("C#4"));
    assert_eq!(
        first.review_state,
        "machine-recommendation-musician-review-required"
    );
}

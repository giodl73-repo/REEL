//! Tests for exact branch admission from verified records.
use reel::spoken_programme::{
    checker::CheckedPackage,
    interactive::{Selection, admit_checked_selection, admit_selection as admit},
};

pub(super) fn assert_verified_bilingual_selections(package: &CheckedPackage) {
    assert!(package.summary().access_checked);
    let checked = ["en-executive", "es-executive"].map(|id| {
        let edition = package
            .access()
            .editions
            .iter()
            .find(|edition| edition.edition_id == id)
            .unwrap();
        let chapter = &edition.chapters[0];
        Selection {
            edition: &edition.edition_id,
            language: &edition.language,
            module: &chapter.module_id,
            programme_sha256: &edition.wave.sha256,
            access_sha256: &edition.transcript.sha256,
            first_frame: chapter.start_sample,
            end_frame: chapter.end_sample,
        }
    });
    let [en, es] = checked;
    assert_eq!(
        (en.language, en.first_frame, en.end_frame),
        ("en", 0, 144_000)
    );
    assert_eq!(
        (es.language, es.first_frame, es.end_frame),
        ("es", 0, 168_000)
    );
    assert_eq!(admit(es, &checked).unwrap(), es);
    admit_checked_selection(package, en).unwrap();
    admit_checked_selection(package, es).unwrap();
    assert!(
        admit_checked_selection(
            package,
            Selection {
                end_frame: en.end_frame,
                ..es
            }
        )
        .is_err()
    );
    assert!(
        admit(
            Selection {
                end_frame: en.end_frame,
                ..es
            },
            &checked
        )
        .is_err()
    );
    assert!(
        admit(
            Selection {
                access_sha256: en.access_sha256,
                ..es
            },
            &checked
        )
        .is_err()
    );
}

const EN: Selection<'static> = Selection {
    edition: "example-en",
    language: "en",
    module: "example",
    programme_sha256: "1111111111111111111111111111111111111111111111111111111111111111",
    access_sha256: "2222222222222222222222222222222222222222222222222222222222222222",
    first_frame: 12,
    end_frame: 36,
};
const ES: Selection<'static> = Selection {
    edition: "example-es",
    language: "es",
    module: "example",
    programme_sha256: "3333333333333333333333333333333333333333333333333333333333333333",
    access_sha256: "4444444444444444444444444444444444444444444444444444444444444444",
    first_frame: 15,
    end_frame: 49,
};

#[test]
fn branch_uses_the_exact_edition_and_preserves_the_language_specific_span() {
    let checked = [EN, ES];
    let en = admit(EN, &checked).unwrap();
    let es = admit(ES, &checked).unwrap();
    assert_eq!((en.first_frame, en.end_frame), (12, 36));
    assert_eq!((es.first_frame, es.end_frame), (15, 49));
    assert_ne!(en.access_sha256, es.access_sha256);
}

#[test]
fn branch_refuses_language_fallback_and_ambiguous_checked_selection() {
    assert!(
        admit(ES, &[EN])
            .unwrap_err()
            .to_string()
            .contains("missing")
    );
    assert!(
        admit(EN, &[EN, EN, ES])
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
}

#[test]
fn branch_refuses_reused_identity_with_wrong_media_text_or_bounds() {
    let checked = [EN, ES];
    for (request, failure) in [
        (
            Selection {
                programme_sha256: EN.programme_sha256,
                ..ES
            },
            "programme identity",
        ),
        (
            Selection {
                access_sha256: EN.access_sha256,
                ..ES
            },
            "access identity",
        ),
        (
            Selection {
                end_frame: EN.end_frame,
                ..ES
            },
            "media span",
        ),
    ] {
        assert!(
            admit(request, &checked)
                .unwrap_err()
                .to_string()
                .contains(failure)
        );
    }
}

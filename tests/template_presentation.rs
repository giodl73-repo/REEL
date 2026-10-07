use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use reel_assembly::scene_authoring::{NATIVE_ALIGNMENT_SCHEMA, NativeAlignment};
use reel_assembly::template_presentation::{
    BylineStyle, ChapterLineStyle, ChapterStyle, DisplaySourceUnit, EditableTextInvocation,
    EditableTextTemplate, INVOCATION_SCHEMA, Panel, PoemLine, PresentationSourceText,
    SCENE_SOURCE_TEXT_SCHEMA_V2, SOURCE_TEXT_SCHEMA, SourceLine, TEMPLATE_SCHEMA, compile_layer,
    verify_native_line_ownership, verify_source_text,
};

fn chapter_line(font_size: u32, margin_left: u32, margin_vertical: u32) -> ChapterLineStyle {
    ChapterLineStyle {
        font_name: "Georgia".into(),
        font_size,
        primary_rgb: [255, 244, 228],
        outline_rgb: [11, 16, 22],
        outline_alpha: 204,
        shadow_rgb: [0, 0, 0],
        shadow_alpha: 104,
        outline_width: if font_size == 54 { 2.6 } else { 2.4 },
        shadow_depth: if font_size == 54 { 1.3 } else { 1.2 },
        margin_left,
        margin_vertical,
    }
}

fn template(kind: &str) -> EditableTextTemplate {
    EditableTextTemplate {
        schema: TEMPLATE_SCHEMA.into(),
        template_id: "test-master".into(),
        kind: kind.into(),
        soundtrack_requirement: None,
        canvas_width: 1280,
        canvas_height: 720,
        font_name: "Monotype Corsiva".into(),
        title_size: 42,
        body_size: 31,
        title_x: 880,
        title_y: 32,
        body_x: 880,
        body_y: 94,
        line_spacing: 42,
        future_rgb: [120, 127, 133],
        active_rgb: [216, 227, 232],
        completed_rgb: [255, 255, 255],
        panel: matches!(kind, "opening-poem" | "internal-poem").then_some(Panel {
            x: 853,
            width: 427,
            background_rgb: [20, 18, 16],
            divider_rgb: [211, 178, 107],
            divider_alpha: 185,
        }),
        byline: None,
        fixed_duration_seconds: (kind == "chapter-title").then_some(4),
        post_poem_title_duration_ms: None,
        chapter: (kind == "chapter-title").then_some(ChapterStyle {
            number: chapter_line(54, 110, 88),
            title: chapter_line(36, 190, 102),
            fade_in_ms: 750,
            fade_out_ms: 750,
        }),
    }
}

fn poem() -> EditableTextInvocation {
    EditableTextInvocation {
        schema: INVOCATION_SCHEMA.into(),
        template_id: "test-master".into(),
        language: "es".into(),
        title: "Recuerdos".into(),
        byline: None,
        lines: vec![
            PoemLine {
                text: "Primera línea".into(),
                cue_id: "a".into(),
                audio_cue_id: None,
                semantic_trigger_id: "first".into(),
                stanza_break_before: false,
            },
            PoemLine {
                text: "Segunda línea".into(),
                cue_id: "a".into(),
                audio_cue_id: None,
                semantic_trigger_id: "second".into(),
                stanza_break_before: false,
            },
            PoemLine {
                text: "Tercera línea".into(),
                cue_id: "b".into(),
                audio_cue_id: None,
                semantic_trigger_id: "first".into(),
                stanza_break_before: true,
            },
        ],
        chapter_number: None,
    }
}

#[test]
fn title_prelude_is_separate_from_native_reading_and_rejects_verse_clocks() {
    let mut title = template("opening-poem");
    title.kind = "poem-title".into();
    title.fixed_duration_seconds = Some(2);
    title.byline = Some(BylineStyle {
        x: 880,
        y: 660,
        font_size: 21,
        rgb: [255, 255, 255],
    });
    let mut text = poem();
    text.lines.clear();
    text.byline = Some("por Andrés Alarcón García".into());
    let output = compile_layer(&title, &text, &[], &BTreeMap::new()).unwrap();
    assert_eq!((output.duration_samples, output.sample_rate), (200, 100));
    assert!(output.ass.contains("Recuerdos"));
    assert!(output.ass.contains("por Andrés Alarcón García"));
    assert!(!output.ass.contains("Primera línea"));
    assert!(compile_layer(&title, &text, &["a".into()], &BTreeMap::new()).is_err());
    text.lines = poem().lines;
    assert!(compile_layer(&title, &text, &[], &BTreeMap::new()).is_err());
}

#[test]
fn title_prelude_rejects_missing_duration_bad_panel_and_mismatched_credit() {
    let mut title = template("opening-poem");
    title.kind = "poem-title".into();
    let mut text = poem();
    text.lines.clear();
    assert!(compile_layer(&title, &text, &[], &BTreeMap::new()).is_err());
    title.fixed_duration_seconds = Some(31);
    assert!(compile_layer(&title, &text, &[], &BTreeMap::new()).is_err());
    title.fixed_duration_seconds = Some(2);
    text.byline = Some("poet".into());
    assert!(compile_layer(&title, &text, &[], &BTreeMap::new()).is_err());
    text.byline = None;
    title.panel.as_mut().unwrap().width = 1;
    assert!(compile_layer(&title, &text, &[], &BTreeMap::new()).is_err());
}

#[test]
fn title_prelude_source_rejects_changed_title_credit_and_missing_credit() {
    let mut invocation = poem();
    invocation.lines.clear();
    invocation.byline = Some("por Andrés Alarcón García".into());
    let source = PresentationSourceText {
        schema: SOURCE_TEXT_SCHEMA.into(),
        source_authority_id: "manuscript".into(),
        source_document_sha256: "a".repeat(64),
        source_scope_ids: vec!["poem-title".into(), "poet-credit".into()],
        language: "es".into(),
        text_state: "canonical-original".into(),
        title: invocation.title.clone(),
        byline: invocation.byline.clone(),
        chapter_number: None,
        lines: vec![],
        display_units: vec![],
    };
    let verify = |text: &EditableTextInvocation| {
        verify_source_text(text, &source, "manuscript", &source.source_scope_ids, &[])
    };
    assert!(verify(&invocation).is_ok());
    let mut changed = invocation.clone();
    changed.title = "Other poem".into();
    assert!(verify(&changed).is_err());
    changed = invocation.clone();
    changed.byline = Some("Other poet".into());
    assert!(verify(&changed).is_err());
    changed.byline = None;
    assert!(verify(&changed).is_err());
}

fn clocks() -> BTreeMap<String, NativeAlignment> {
    BTreeMap::from([
        (
            "a".into(),
            NativeAlignment {
                schema: NATIVE_ALIGNMENT_SCHEMA.into(),
                language: "es".into(),
                cue_id: "a".into(),
                selected_take_sha256: "a".repeat(64),
                sample_rate: 24_000,
                cue_end_sample: 24_000,
                semantic_markers: BTreeMap::from([("first".into(), 0), ("second".into(), 12_000)]),
            },
        ),
        (
            "b".into(),
            NativeAlignment {
                schema: NATIVE_ALIGNMENT_SCHEMA.into(),
                language: "es".into(),
                cue_id: "b".into(),
                selected_take_sha256: "b".repeat(64),
                sample_rate: 24_000,
                cue_end_sample: 48_000,
                semantic_markers: BTreeMap::from([("first".into(), 0)]),
            },
        ),
    ])
}

fn label() -> (EditableTextTemplate, EditableTextInvocation) {
    let mut definition = template("semantic-label");
    definition.title_x = 640;
    definition.title_y = 680;
    definition.fixed_duration_seconds = Some(4);
    let mut invocation = poem();
    invocation.title = "1937".into();
    invocation.lines = vec![PoemLine {
        text: "1937".into(),
        cue_id: "b".into(),
        audio_cue_id: None,
        semantic_trigger_id: "first".into(),
        stanza_break_before: false,
    }];
    (definition, invocation)
}

#[test]
fn semantic_label_uses_language_local_marker_and_preserves_scene_clock() {
    let (definition, mut invocation) = label();
    for (language, first_duration, marker, expected_start) in [
        ("es", 24_000, 12_000, "0:00:01.50"),
        ("en", 48_000, 6_000, "0:00:02.25"),
    ] {
        let mut native = clocks();
        invocation.language = language.into();
        for alignment in native.values_mut() {
            alignment.language = language.into();
        }
        native.get_mut("a").unwrap().cue_end_sample = first_duration;
        native
            .get_mut("b")
            .unwrap()
            .semantic_markers
            .insert("first".into(), marker);
        let compiled =
            compile_layer(&definition, &invocation, &["a".into(), "b".into()], &native).unwrap();
        assert_eq!(compiled.duration_samples, first_duration + 48_000);
        assert!(
            compiled
                .ass
                .contains(&format!("Dialogue: 0,{expected_start},"))
        );
        assert!(compiled.ass.contains("\\an2\\pos(640,680)\\fs42"));
        assert_eq!(compiled.ass.matches("Dialogue:").count(), 1);
    }
}

#[test]
fn semantic_label_rejects_wrong_scope_missing_markers_and_mixed_clocks() {
    let (definition, invocation) = label();
    let mut native = clocks();
    assert!(compile_layer(&definition, &invocation, &["a".into()], &native).is_err());
    native.get_mut("b").unwrap().semantic_markers.clear();
    assert!(compile_layer(&definition, &invocation, &["a".into(), "b".into()], &native).is_err());
    native = clocks();
    native.get_mut("b").unwrap().sample_rate = 44_100;
    assert!(compile_layer(&definition, &invocation, &["a".into(), "b".into()], &native).is_err());
    native = clocks();
    native
        .get_mut("b")
        .unwrap()
        .semantic_markers
        .insert("first".into(), 48_000);
    assert!(compile_layer(&definition, &invocation, &["a".into(), "b".into()], &native).is_err());
}

#[test]
fn semantic_label_rejects_poem_layout_and_unbound_text() {
    let (mut definition, mut invocation) = label();
    invocation.lines[0].text = "1939".into();
    assert!(
        compile_layer(
            &definition,
            &invocation,
            &["a".into(), "b".into()],
            &clocks()
        )
        .is_err()
    );
    invocation.lines[0].text = invocation.title.clone();
    definition.fixed_duration_seconds = Some(0);
    assert!(
        compile_layer(
            &definition,
            &invocation,
            &["a".into(), "b".into()],
            &clocks()
        )
        .is_err()
    );
    definition.fixed_duration_seconds = Some(4);
    invocation.lines[0].stanza_break_before = true;
    assert!(
        compile_layer(
            &definition,
            &invocation,
            &["a".into(), "b".into()],
            &clocks()
        )
        .is_err()
    );
}

#[test]
fn semantic_label_cannot_borrow_an_unrelated_source_performance() {
    let (_, mut invocation) = label();
    let cue: reel_assembly::scene_authoring::Cue = serde_json::from_value(serde_json::json!({
        "cue_id":"b", "source_id":"b", "source_cue_ids":["source-b"],
        "exact_text_sha256":"b".repeat(64), "narration_slot_id":"b.voice"
    }))
    .unwrap();
    invocation.lines[0].audio_cue_id = Some("b".into());
    invocation.lines[0].cue_id = "source-a".into();
    assert!(verify_native_line_ownership(&invocation, std::slice::from_ref(&cue)).is_err());
    invocation.lines[0].cue_id = "source-b".into();
    verify_native_line_ownership(&invocation, std::slice::from_ref(&cue)).unwrap();
    invocation.lines[0].semantic_trigger_id.clear();
    assert!(verify_native_line_ownership(&invocation, &[cue]).is_err());
}

#[test]
fn distinct_source_lines_use_one_continuous_native_performance() {
    let mut invocation = poem();
    for (index, line) in invocation.lines.iter_mut().enumerate() {
        line.cue_id = format!("source-{index}");
        line.audio_cue_id = Some("whole".into());
        line.semantic_trigger_id = format!("line-{index}");
    }
    for (language, markers, end) in [
        ("es", [0, 24_000, 48_000], 72_000),
        ("en", [0, 12_000, 36_000], 60_000),
    ] {
        invocation.language = language.into();
        let mut native = BTreeMap::from([(
            "whole".into(),
            NativeAlignment {
                schema: NATIVE_ALIGNMENT_SCHEMA.into(),
                language: language.into(),
                cue_id: "whole".into(),
                selected_take_sha256: "a".repeat(64),
                sample_rate: 24_000,
                cue_end_sample: end,
                semantic_markers: markers
                    .into_iter()
                    .enumerate()
                    .map(|(i, n)| (format!("line-{i}"), n))
                    .collect(),
            },
        )]);
        let layer = compile_layer(
            &template("opening-poem"),
            &invocation,
            &["whole".into()],
            &native,
        )
        .unwrap();
        assert_eq!(layer.duration_samples, end);
        native
            .get_mut("whole")
            .unwrap()
            .semantic_markers
            .remove("line-1");
        assert!(
            compile_layer(
                &template("opening-poem"),
                &invocation,
                &["whole".into()],
                &native
            )
            .is_err()
        );
        native
            .get_mut("whole")
            .unwrap()
            .semantic_markers
            .insert("line-1".into(), markers[2]);
        assert!(
            compile_layer(
                &template("opening-poem"),
                &invocation,
                &["whole".into()],
                &native
            )
            .is_err()
        );
    }
}

#[test]
fn complete_poem_is_visible_from_zero_and_highlights_native_markers() {
    let layer = compile_layer(
        &template("opening-poem"),
        &poem(),
        &["a".into(), "b".into()],
        &clocks(),
    )
    .unwrap();
    assert_eq!(layer.duration_samples, 72_000);
    assert_eq!(layer.sample_rate, 24_000);
    let zero = layer
        .ass
        .lines()
        .filter(|line| line.starts_with("Dialogue: 0,0:00:00.00,0:00:00.50,Text"))
        .collect::<Vec<_>>();
    assert_eq!(zero.len(), 3);
    assert!(zero.iter().any(|line| line.contains("Primera línea")));
    assert!(zero.iter().any(|line| line.contains("Segunda línea")));
    assert!(zero.iter().any(|line| line.contains("Tercera línea")));
    assert!(layer.ass.contains("&HB96BB2D3"));
    assert!(layer.ass.contains("Dialogue: 0,0:00:00.50,0:00:01.00,Text"));
    assert!(layer.ass.contains("Dialogue: 0,0:00:01.00,0:00:03.00,Text"));
}

#[test]
fn internal_poem_uses_the_same_source_ordered_native_panel() {
    let mut invocation = poem();
    invocation.title = "¿Por qué vuelan?".into();
    let layer = compile_layer(
        &template("internal-poem"),
        &invocation,
        &["a".into(), "b".into()],
        &clocks(),
    )
    .unwrap();
    assert_eq!(layer.duration_samples, 72_000);
    assert!(layer.ass.contains("¿Por qué vuelan?"));
    assert!(layer.ass.contains("Tercera línea"));
}

#[test]
fn long_poem_lines_wrap_inside_the_panel_without_changing_source_text() {
    let mut invocation = poem();
    invocation.lines[1].text =
        "¿Por qué el hombre sus pies los tiene clavados en la tierra?".into();
    let layer = compile_layer(
        &template("internal-poem"),
        &invocation,
        &["a".into(), "b".into()],
        &clocks(),
    )
    .unwrap();
    assert!(layer.ass.contains("\\N"));
    assert!(layer.ass.contains("clavados"));
    assert!(layer.ass.contains("tierra?"));
}

#[test]
fn selected_poet_byline_is_source_checked_and_editable_from_first_frame() {
    let mut invocation = poem();
    invocation.byline = Some("por Andrés Alarcón García".into());
    let mut master = template("opening-poem");
    master.byline = Some(BylineStyle {
        x: 880,
        y: 660,
        font_size: 21,
        rgb: [255, 255, 255],
    });
    let layer = compile_layer(&master, &invocation, &["a".into(), "b".into()], &clocks()).unwrap();
    assert!(layer.ass.contains("por Andrés Alarcón García"));
    assert!(layer.ass.contains("\\pos(880,660)\\fs21"));
    let source = PresentationSourceText {
        schema: SOURCE_TEXT_SCHEMA.into(),
        source_authority_id: "manuscript".into(),
        source_document_sha256: "a".repeat(64),
        source_scope_ids: vec!["source-block-1".into()],
        language: "es".into(),
        text_state: "canonical-original".into(),
        title: invocation.title.clone(),
        byline: invocation.byline.clone(),
        chapter_number: None,
        lines: invocation
            .lines
            .iter()
            .map(|line| SourceLine {
                text: line.text.clone(),
                cue_id: line.cue_id.clone(),
                audio_cue_id: line.audio_cue_id.clone(),
                stanza_break_before: line.stanza_break_before,
            })
            .collect(),
        display_units: vec![],
    };
    verify_source_text(
        &invocation,
        &source,
        "manuscript",
        &["source-block-1".into()],
        &[],
    )
    .unwrap();
    let mut changed = source;
    changed.byline = Some("otro poeta".into());
    assert!(
        verify_source_text(
            &invocation,
            &changed,
            "manuscript",
            &["source-block-1".into()],
            &[],
        )
        .is_err()
    );
}

#[test]
fn poem_title_and_credit_can_follow_the_last_native_line() {
    let mut invocation = poem();
    invocation.title = "de “Recuerdos”".into();
    invocation.byline = Some("por Andrés Alarcón García".into());
    let mut master = template("opening-poem");
    master.post_poem_title_duration_ms = Some(2_000);
    master.byline = Some(BylineStyle {
        x: 880,
        y: 660,
        font_size: 21,
        rgb: [255, 255, 255],
    });
    let layer = compile_layer(&master, &invocation, &["a".into(), "b".into()], &clocks()).unwrap();
    assert_eq!(layer.duration_samples, 120_000);
    let title = layer
        .ass
        .lines()
        .filter(|line| line.contains("de “Recuerdos”"))
        .collect::<Vec<_>>();
    let credit = layer
        .ass
        .lines()
        .filter(|line| line.contains("por Andrés Alarcón García"))
        .collect::<Vec<_>>();
    assert_eq!(title.len(), 1);
    assert_eq!(credit.len(), 1);
    assert!(title[0].starts_with("Dialogue: 0,0:00:03.00,0:00:05.00,Text"));
    assert!(credit[0].starts_with("Dialogue: 0,0:00:03.00,0:00:05.00,Text"));
    assert!(
        !layer
            .ass
            .lines()
            .any(|line| line.contains("Recuerdos") && line.contains("0:00:00.00"))
    );
    assert!(
        layer
            .ass
            .contains("Dialogue: 0,0:00:00.00,0:00:03.00,Panel")
    );
    assert!(layer.ass.contains("Dialogue: 0,0:00:01.00,0:00:03.00,Text"));
}

#[test]
fn post_poem_title_duration_is_validated_and_not_a_chapter_card_option() {
    let mut master = template("opening-poem");
    master.post_poem_title_duration_ms = Some(0);
    assert!(compile_layer(&master, &poem(), &["a".into(), "b".into()], &clocks()).is_err());
    master.post_poem_title_duration_ms = Some(15);
    assert!(compile_layer(&master, &poem(), &["a".into(), "b".into()], &clocks()).is_err());
    master.post_poem_title_duration_ms = Some(2_000);
    assert!(compile_layer(&master, &poem(), &["a".into(), "b".into()], &clocks()).is_err());
    let mut chapter = template("chapter-title");
    chapter.post_poem_title_duration_ms = Some(2_000);
    assert!(compile_layer(&chapter, &poem(), &[], &BTreeMap::new()).is_err());
}

#[test]
fn v2_source_binds_each_post_poem_title_and_credit_id_to_editable_text() {
    let hash = |text: &str| {
        Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let mut invocation = poem();
    invocation.title = "de “Recuerdos”".into();
    invocation.byline = Some("por Andrés Alarcón García".into());
    let unit = |id: &str, role: &str, raw: &str, editable: &str| DisplaySourceUnit {
        source_scope_id: id.into(),
        role: role.into(),
        source_text: raw.into(),
        source_text_sha256: hash(raw),
        editable_text: editable.into(),
        editable_text_sha256: hash(editable),
    };
    let mut source = PresentationSourceText {
        schema: SCENE_SOURCE_TEXT_SCHEMA_V2.into(),
        source_authority_id: "manuscript".into(),
        source_document_sha256: "a".repeat(64),
        source_scope_ids: vec!["spoken".into(), "title".into(), "credit".into()],
        language: "es".into(),
        text_state: "canonical-original".into(),
        title: invocation.title.clone(),
        byline: invocation.byline.clone(),
        chapter_number: None,
        lines: invocation
            .lines
            .iter()
            .map(|line| SourceLine {
                text: line.text.clone(),
                cue_id: line.cue_id.clone(),
                audio_cue_id: line.audio_cue_id.clone(),
                stanza_break_before: line.stanza_break_before,
            })
            .collect(),
        display_units: vec![
            unit("title", "poem-title", "nde “Recuerdos”", "de “Recuerdos”"),
            unit(
                "credit",
                "poet-credit",
                "Andrés Alarcón García",
                "por Andrés Alarcón García",
            ),
        ],
    };
    let scopes = ["spoken".into(), "title".into(), "credit".into()];
    let displays = ["title".into(), "credit".into()];
    assert!(verify_source_text(&invocation, &source, "manuscript", &scopes, &displays).is_ok());
    source.display_units.swap(0, 1);
    assert!(verify_source_text(&invocation, &source, "manuscript", &scopes, &displays).is_err());
    source.display_units.swap(0, 1);
    source.display_units[0].editable_text = "from “Memories”".into();
    assert!(verify_source_text(&invocation, &source, "manuscript", &scopes, &displays).is_err());
    source.display_units[0].editable_text = invocation.title.clone();
    source.language = "en".into();
    assert!(verify_source_text(&invocation, &source, "manuscript", &scopes, &displays).is_err());
}

#[test]
fn chapter_duration_comes_from_template() {
    let invocation = EditableTextInvocation {
        schema: INVOCATION_SCHEMA.into(),
        template_id: "test-master".into(),
        language: "es".into(),
        title: "Lo que el viento nos dejó".into(),
        byline: None,
        lines: vec![],
        chapter_number: Some("7".into()),
    };
    let layer = compile_layer(
        &template("chapter-title"),
        &invocation,
        &[],
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(layer.duration_samples, 400);
    assert!(layer.ass.contains("Style: Number,Georgia,54"));
    assert!(layer.ass.contains("Style: Chapter,Georgia,36"));
    assert!(layer.ass.contains(
        "Dialogue: 0,0:00:00.00,0:00:04.00,Number,chapter_number,110,0,88,,{\\fad(750,750)}7"
    ));
    assert!(layer.ass.contains("Dialogue: 1,0:00:00.00,0:00:04.00,Chapter,chapter_title,190,0,102,,{\\fad(750,750)}Lo que el viento nos dejó"));
}

#[test]
fn rejects_nonmonotonic_or_missing_native_line_anchors() {
    let mut invocation = poem();
    invocation.lines.swap(0, 1);
    assert!(
        compile_layer(
            &template("opening-poem"),
            &invocation,
            &["a".into(), "b".into()],
            &clocks()
        )
        .is_err()
    );
    invocation = poem();
    invocation.lines[1].semantic_trigger_id = "unmeasured".into();
    assert!(
        compile_layer(
            &template("opening-poem"),
            &invocation,
            &["a".into(), "b".into()],
            &clocks()
        )
        .is_err()
    );
}

#[test]
fn rejects_native_markers_that_round_to_one_ass_instant() {
    let mut alignments = clocks();
    alignments
        .get_mut("a")
        .unwrap()
        .semantic_markers
        .insert("second".into(), 1);
    assert!(
        compile_layer(
            &template("opening-poem"),
            &poem(),
            &["a".into(), "b".into()],
            &alignments
        )
        .is_err()
    );
}

#[test]
fn selected_source_must_match_every_poem_line_and_stanza() {
    let invocation = poem();
    let mut source = PresentationSourceText {
        schema: SOURCE_TEXT_SCHEMA.into(),
        source_authority_id: "manuscript".into(),
        source_document_sha256: "a".repeat(64),
        source_scope_ids: vec!["source-block-1".into()],
        language: "es".into(),
        text_state: "canonical-original".into(),
        title: invocation.title.clone(),
        byline: None,
        chapter_number: None,
        lines: invocation
            .lines
            .iter()
            .map(|line| SourceLine {
                text: line.text.clone(),
                cue_id: line.cue_id.clone(),
                audio_cue_id: line.audio_cue_id.clone(),
                stanza_break_before: line.stanza_break_before,
            })
            .collect(),
        display_units: vec![],
    };
    assert!(
        verify_source_text(
            &invocation,
            &source,
            "manuscript",
            &["source-block-1".into()],
            &[],
        )
        .is_ok()
    );
    source.lines[1].text = "Changed line".into();
    assert!(
        verify_source_text(
            &invocation,
            &source,
            "manuscript",
            &["source-block-1".into()],
            &[],
        )
        .is_err()
    );
    source.lines[1].text = invocation.lines[1].text.clone();
    source.lines[2].stanza_break_before = false;
    assert!(
        verify_source_text(
            &invocation,
            &source,
            "manuscript",
            &["source-block-1".into()],
            &[],
        )
        .is_err()
    );
}

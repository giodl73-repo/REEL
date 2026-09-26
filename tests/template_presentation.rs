use std::collections::BTreeMap;

use reel_assembly::scene_authoring::{NATIVE_ALIGNMENT_SCHEMA, NativeAlignment};
use reel_assembly::template_presentation::{
    BylineStyle, ChapterLineStyle, ChapterStyle, EditableTextInvocation, EditableTextTemplate,
    INVOCATION_SCHEMA, Panel, PoemLine, PresentationSourceText, SOURCE_TEXT_SCHEMA, SourceLine,
    TEMPLATE_SCHEMA, compile_layer, verify_source_text,
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
        panel: (kind == "opening-poem").then_some(Panel {
            x: 853,
            width: 427,
            background_rgb: [20, 18, 16],
            divider_rgb: [211, 178, 107],
            divider_alpha: 185,
        }),
        byline: None,
        fixed_duration_seconds: (kind == "chapter-title").then_some(4),
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
                semantic_trigger_id: "first".into(),
                stanza_break_before: false,
            },
            PoemLine {
                text: "Segunda línea".into(),
                cue_id: "a".into(),
                semantic_trigger_id: "second".into(),
                stanza_break_before: false,
            },
            PoemLine {
                text: "Tercera línea".into(),
                cue_id: "b".into(),
                semantic_trigger_id: "first".into(),
                stanza_break_before: true,
            },
        ],
        chapter_number: None,
    }
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
                stanza_break_before: line.stanza_break_before,
            })
            .collect(),
    };
    verify_source_text(
        &invocation,
        &source,
        "manuscript",
        &["source-block-1".into()],
    )
    .unwrap();
    let mut changed = source;
    changed.byline = Some("otro poeta".into());
    assert!(
        verify_source_text(
            &invocation,
            &changed,
            "manuscript",
            &["source-block-1".into()]
        )
        .is_err()
    );
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
                stanza_break_before: line.stanza_break_before,
            })
            .collect(),
    };
    assert!(
        verify_source_text(
            &invocation,
            &source,
            "manuscript",
            &["source-block-1".into()]
        )
        .is_ok()
    );
    source.lines[1].text = "Changed line".into();
    assert!(
        verify_source_text(
            &invocation,
            &source,
            "manuscript",
            &["source-block-1".into()]
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
            &["source-block-1".into()]
        )
        .is_err()
    );
}

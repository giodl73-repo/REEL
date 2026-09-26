//! Compile editable presentation layers. Geometry and style live in the
//! template; an invocation supplies only content and native semantic anchors.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::scene_authoring::{NATIVE_ALIGNMENT_SCHEMA, NativeAlignment};

pub const TEMPLATE_SCHEMA: &str = "reel.editable-text-template.v1";
pub const INVOCATION_SCHEMA: &str = "reel.editable-text-invocation.v1";
pub const SOURCE_TEXT_SCHEMA: &str = "reel.presentation-source-text.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationSourceText {
    pub schema: String,
    pub source_authority_id: String,
    pub source_document_sha256: String,
    pub source_scope_ids: Vec<String>,
    pub language: String,
    pub text_state: String,
    pub title: String,
    pub chapter_number: Option<String>,
    pub lines: Vec<SourceLine>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLine {
    pub text: String,
    pub cue_id: String,
    pub stanza_break_before: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditableTextTemplate {
    pub schema: String,
    pub template_id: String,
    pub kind: String,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub font_name: String,
    pub title_size: u32,
    pub body_size: u32,
    pub title_x: u32,
    pub title_y: u32,
    pub body_x: u32,
    pub body_y: u32,
    pub line_spacing: u32,
    pub future_rgb: [u8; 3],
    pub active_rgb: [u8; 3],
    pub completed_rgb: [u8; 3],
    pub panel: Option<Panel>,
    pub fixed_duration_seconds: Option<u32>,
    #[serde(default)]
    pub chapter: Option<ChapterStyle>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub x: u32,
    pub width: u32,
    pub background_rgb: [u8; 3],
    pub divider_rgb: [u8; 3],
    pub divider_alpha: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChapterStyle {
    pub number: ChapterLineStyle,
    pub title: ChapterLineStyle,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChapterLineStyle {
    pub font_name: String,
    pub font_size: u32,
    pub primary_rgb: [u8; 3],
    pub outline_rgb: [u8; 3],
    pub outline_alpha: u8,
    pub shadow_rgb: [u8; 3],
    pub shadow_alpha: u8,
    pub outline_width: f32,
    pub shadow_depth: f32,
    pub margin_left: u32,
    pub margin_vertical: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditableTextInvocation {
    pub schema: String,
    pub template_id: String,
    pub language: String,
    pub title: String,
    #[serde(default)]
    pub lines: Vec<PoemLine>,
    #[serde(default)]
    pub chapter_number: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoemLine {
    pub text: String,
    pub cue_id: String,
    pub semantic_trigger_id: String,
    #[serde(default)]
    pub stanza_break_before: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompiledLayer {
    pub schema: String,
    pub template_id: String,
    pub language: String,
    pub duration_samples: u64,
    pub sample_rate: u32,
    pub ass: String,
}

/// A selected source-text file is the wording authority for an invocation.
/// This checks exact text and stanza structure; it does not grant human review.
pub fn verify_source_text(
    invocation: &EditableTextInvocation,
    source: &PresentationSourceText,
    source_authority_id: &str,
    source_scope_ids: &[String],
) -> Result<()> {
    if source.schema != SOURCE_TEXT_SCHEMA
        || source.source_authority_id != source_authority_id
        || source.language != invocation.language
        || source.source_document_sha256.len() != 64
        || !source
            .source_document_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !matches!(
            source.text_state.as_str(),
            "canonical-original" | "approved-translation" | "project-draft-review-held"
        )
        || source.source_scope_ids != source_scope_ids
        || source.title != invocation.title
        || source.chapter_number != invocation.chapter_number
        || source.lines.len() != invocation.lines.len()
    {
        bail!("presentation content differs from selected source-text authority");
    }
    for (selected, authored) in source.lines.iter().zip(&invocation.lines) {
        if selected.text != authored.text
            || selected.cue_id != authored.cue_id
            || selected.stanza_break_before != authored.stanza_break_before
        {
            bail!("poem wording, cue scope, or stanza breaks differ from selected source");
        }
    }
    Ok(())
}

fn color(rgb: [u8; 3]) -> String {
    format!("&H00{:02X}{:02X}{:02X}", rgb[2], rgb[1], rgb[0])
}

fn alpha_color(rgb: [u8; 3], alpha: u8) -> String {
    format!("&H{alpha:02X}{:02X}{:02X}{:02X}", rgb[2], rgb[1], rgb[0])
}

fn chapter_style(name: &str, style: &ChapterLineStyle) -> Result<String> {
    if style.font_name.trim().is_empty()
        || style.font_name.contains([',', '\r', '\n'])
        || style.font_size == 0
        || !style.outline_width.is_finite()
        || !style.shadow_depth.is_finite()
        || style.outline_width < 0.0
        || style.shadow_depth < 0.0
    {
        bail!("invalid chapter typography");
    }
    Ok(format!(
        "Style: {name},{},{},{},&H000000FF,{},{},-1,0,0,0,100,100,0,0,1,{},{},1,100,100,75,1\n",
        style.font_name,
        style.font_size,
        color(style.primary_rgb),
        alpha_color(style.outline_rgb, style.outline_alpha),
        alpha_color(style.shadow_rgb, style.shadow_alpha),
        style.outline_width,
        style.shadow_depth,
    ))
}

fn escape(value: &str) -> Result<String> {
    if value.trim().is_empty() || value.contains(['\r', '\n']) {
        bail!("editable text must be nonempty and single-line");
    }
    Ok(value
        .replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}"))
}

fn centiseconds(sample: u64, rate: u32) -> u64 {
    ((sample as u128 * 100 + (rate as u128 / 2)) / rate as u128) as u64
}

fn time(sample: u64, rate: u32) -> String {
    let cs = centiseconds(sample, rate);
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360_000,
        cs / 6_000 % 60,
        cs / 100 % 60,
        cs % 100
    )
}

fn event(start: u64, end: u64, rate: u32, style: &str, text: &str) -> String {
    format!(
        "Dialogue: 0,{},{},{style},,0,0,0,,{text}\n",
        time(start, rate),
        time(end, rate)
    )
}

/// Create a portable, editable ASS source. Every poem line is present at the
/// first frame; only its read state changes. Each entrance is measured on its
/// selected language-local take, never supplied as an authored second.
pub fn compile_layer(
    template: &EditableTextTemplate,
    invocation: &EditableTextInvocation,
    ordered_cues: &[String],
    alignments: &BTreeMap<String, NativeAlignment>,
) -> Result<CompiledLayer> {
    if template.schema != TEMPLATE_SCHEMA
        || invocation.schema != INVOCATION_SCHEMA
        || template.template_id != invocation.template_id
        || !matches!(template.kind.as_str(), "opening-poem" | "chapter-title")
        || template.canvas_width == 0
        || template.canvas_height == 0
        || template.font_name.trim().is_empty()
        || template.font_name.contains([',', '\r', '\n'])
        || template.title_size == 0
        || template.body_size == 0
        || template.line_spacing == 0
    {
        bail!("invalid editable presentation template or invocation");
    }
    let mut ass = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {}\nPlayResY: {}\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Text,{},{},{},&H00000000,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\nStyle: Panel,Arial,1,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        template.canvas_width,
        template.canvas_height,
        template.font_name,
        template.body_size,
        color(template.completed_rgb)
    );
    if template.kind == "chapter-title" {
        if !invocation.lines.is_empty() || !ordered_cues.is_empty() || !alignments.is_empty() {
            bail!("chapter card cannot carry poem lines or native cue clocks");
        }
        let duration = template
            .fixed_duration_seconds
            .ok_or_else(|| anyhow::anyhow!("chapter template needs fixed duration"))?;
        if duration == 0 || template.panel.is_some() {
            bail!("invalid chapter card template");
        }
        let chapter = template
            .chapter
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("chapter typography missing"))?;
        if chapter.fade_in_ms.saturating_add(chapter.fade_out_ms) >= duration * 1000 {
            bail!("chapter fades consume its entire duration");
        }
        let styles = format!(
            "{}{}",
            chapter_style("Number", &chapter.number)?,
            chapter_style("Chapter", &chapter.title)?
        );
        ass = ass.replace("\n[Events]\n", &format!("\n{styles}[Events]\n"));
        let number = escape(
            invocation
                .chapter_number
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("chapter number missing"))?,
        )?;
        let title = escape(&invocation.title)?;
        let end = duration as u64 * 100;
        ass.push_str(&format!(
            "Dialogue: 0,0:00:00.00,{},Number,chapter_number,{},0,{},,{{\\fad({},{})}}{}\n",
            time(end, 100),
            chapter.number.margin_left,
            chapter.number.margin_vertical,
            chapter.fade_in_ms,
            chapter.fade_out_ms,
            number
        ));
        ass.push_str(&format!(
            "Dialogue: 1,0:00:00.00,{},Chapter,chapter_title,{},0,{},,{{\\fad({},{})}}{}\n",
            time(end, 100),
            chapter.title.margin_left,
            chapter.title.margin_vertical,
            chapter.fade_in_ms,
            chapter.fade_out_ms,
            title
        ));
        return Ok(CompiledLayer {
            schema: "reel.compiled-editable-layer.v1".into(),
            template_id: template.template_id.clone(),
            language: invocation.language.clone(),
            duration_samples: end,
            sample_rate: 100,
            ass,
        });
    }
    if template.fixed_duration_seconds.is_some()
        || template.chapter.is_some()
        || invocation.chapter_number.is_some()
        || invocation.lines.is_empty()
        || ordered_cues.is_empty()
    {
        bail!("poem requires native cues and source lines, with no fixed duration");
    }
    let panel = template
        .panel
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("poem panel missing"))?;
    if panel.x == 0
        || panel.width == 0
        || panel.x + panel.width != template.canvas_width
        || template.title_x < panel.x
        || template.body_x < panel.x
        || template.body_x >= template.canvas_width
        || template.body_y >= template.canvas_height
    {
        bail!("poem panel geometry does not fit canvas");
    }
    let mut starts = BTreeMap::new();
    let mut cursor = 0u64;
    let mut rate = None;
    for cue_id in ordered_cues {
        let alignment = alignments
            .get(cue_id)
            .ok_or_else(|| anyhow::anyhow!("missing native alignment for {cue_id}"))?;
        if alignment.schema != NATIVE_ALIGNMENT_SCHEMA
            || alignment.language != invocation.language
            || alignment.cue_id != *cue_id
            || alignment.sample_rate == 0
            || alignment.cue_end_sample == 0
            || rate.is_some_and(|old| old != alignment.sample_rate)
            || starts.insert(cue_id.as_str(), cursor).is_some()
        {
            bail!("invalid or duplicate native cue alignment");
        }
        rate = Some(alignment.sample_rate);
        cursor = cursor
            .checked_add(alignment.cue_end_sample)
            .ok_or_else(|| anyhow::anyhow!("native clock overflow"))?;
    }
    let rate = rate.unwrap();
    let mut entrances = Vec::new();
    let mut used = BTreeSet::new();
    for line in &invocation.lines {
        escape(&line.text)?;
        let base = starts
            .get(line.cue_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("line references cue outside poem"))?;
        let alignment = &alignments[&line.cue_id];
        let marker = *alignment
            .semantic_markers
            .get(&line.semantic_trigger_id)
            .ok_or_else(|| anyhow::anyhow!("line semantic marker missing"))?;
        if marker >= alignment.cue_end_sample
            || !used.insert((&line.cue_id, &line.semantic_trigger_id))
        {
            bail!("line marker invalid or reused");
        }
        entrances.push(base + marker);
    }
    if entrances[0] != 0 || entrances.windows(2).any(|pair| pair[0] >= pair[1]) {
        bail!("poem lines must enter in native source order from sample zero");
    }
    let displayed = entrances
        .iter()
        .copied()
        .chain(std::iter::once(cursor))
        .map(|sample| centiseconds(sample, rate))
        .collect::<Vec<_>>();
    if displayed.windows(2).any(|pair| pair[0] >= pair[1]) {
        bail!("native line entrance is too close to another entrance or poem end for ASS clock");
    }
    let mut y = template.body_y;
    let mut positions = Vec::new();
    for line in &invocation.lines {
        if line.stanza_break_before {
            y += template.line_spacing / 2;
        }
        if y + template.body_size >= template.canvas_height {
            bail!("poem text exceeds template canvas");
        }
        positions.push(y);
        y += template.line_spacing;
    }
    let draw = format!(
        "{{\\an7\\pos(0,0)\\p1\\1c{}}}m {} 0 l {} 0 {} {} {} {}{{\\p0}}",
        color(panel.background_rgb),
        panel.x,
        template.canvas_width,
        template.canvas_width,
        template.canvas_height,
        panel.x,
        template.canvas_height
    );
    ass.push_str(&event(0, cursor, rate, "Panel", &draw));
    let divider = format!(
        "{{\\an7\\pos(0,0)\\p1\\1c{}}}m {} 0 l {} 0 {} {} {} {}{{\\p0}}",
        alpha_color(panel.divider_rgb, panel.divider_alpha),
        panel.x,
        panel.x + 2,
        panel.x + 2,
        template.canvas_height,
        panel.x,
        template.canvas_height
    );
    ass.push_str(&event(0, cursor, rate, "Panel", &divider));
    ass.push_str(&event(
        0,
        cursor,
        rate,
        "Text",
        &format!(
            "{{\\pos({},{})\\fs{}\\1c{}}}{}",
            template.title_x,
            template.title_y,
            template.title_size,
            color(template.completed_rgb),
            escape(&invocation.title)?
        ),
    ));
    for boundary in 0..invocation.lines.len() {
        let start = entrances[boundary];
        let end = entrances.get(boundary + 1).copied().unwrap_or(cursor);
        for (index, line) in invocation.lines.iter().enumerate() {
            let state = if index < boundary {
                template.completed_rgb
            } else if index == boundary {
                template.active_rgb
            } else {
                template.future_rgb
            };
            let bold = if index == boundary { "\\b1" } else { "\\b0" };
            let visible_from = if boundary == 0 { 0 } else { start };
            ass.push_str(&event(
                visible_from,
                end,
                rate,
                "Text",
                &format!(
                    "{{\\pos({},{})\\1c{}{} }}{}",
                    template.body_x,
                    positions[index],
                    color(state),
                    bold,
                    escape(&line.text)?
                ),
            ));
        }
    }
    Ok(CompiledLayer {
        schema: "reel.compiled-editable-layer.v1".into(),
        template_id: template.template_id.clone(),
        language: invocation.language.clone(),
        duration_samples: cursor,
        sample_rate: rate,
        ass,
    })
}

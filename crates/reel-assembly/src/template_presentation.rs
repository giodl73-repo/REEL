//! Compile editable presentation layers. Geometry and style live in the
//! template; an invocation supplies only content and native semantic anchors.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::scene_authoring::{NATIVE_ALIGNMENT_SCHEMA, NativeAlignment};

pub const TEMPLATE_SCHEMA: &str = "reel.editable-text-template.v1";
pub const INVOCATION_SCHEMA: &str = "reel.editable-text-invocation.v1";
pub const SOURCE_TEXT_SCHEMA: &str = "reel.presentation-source-text.v1";
pub const SCENE_SOURCE_TEXT_SCHEMA_V2: &str = "reel.scene-presentation-source-text.v2";

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
    #[serde(default)]
    pub byline: Option<String>,
    pub chapter_number: Option<String>,
    pub lines: Vec<SourceLine>,
    /// V2 exact source/display join for a post-poem title and poet credit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub display_units: Vec<DisplaySourceUnit>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DisplaySourceUnit {
    pub source_scope_id: String,
    pub role: String,
    pub source_text: String,
    pub source_text_sha256: String,
    pub editable_text: String,
    pub editable_text_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLine {
    pub text: String,
    pub cue_id: String,
    /// Native performance owning this canonical source line; defaults to cue_id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_cue_id: Option<String>,
    pub stanza_break_before: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditableTextTemplate {
    pub schema: String,
    pub template_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soundtrack_requirement: Option<String>,
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
    #[serde(default)]
    pub byline: Option<BylineStyle>,
    pub fixed_duration_seconds: Option<u32>,
    /// Optional title/byline card after the last native poem cue, before prose.
    #[serde(default)]
    pub post_poem_title_duration_ms: Option<u32>,
    #[serde(default)]
    pub chapter: Option<ChapterStyle>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BylineStyle {
    pub x: u32,
    pub y: u32,
    pub font_size: u32,
    pub rgb: [u8; 3],
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
    pub byline: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_cue_id: Option<String>,
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
    display_scope_ids: &[String],
) -> Result<()> {
    if !matches!(
        source.schema.as_str(),
        SOURCE_TEXT_SCHEMA | SCENE_SOURCE_TEXT_SCHEMA_V2
    ) || source.source_authority_id != source_authority_id
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
        || source.byline != invocation.byline
        || source.chapter_number != invocation.chapter_number
        || source.lines.len() != invocation.lines.len()
    {
        bail!("presentation content differs from selected source-text authority");
    }
    for (selected, authored) in source.lines.iter().zip(&invocation.lines) {
        if selected.text != authored.text
            || selected.cue_id != authored.cue_id
            || selected.audio_cue_id.as_deref().unwrap_or(&selected.cue_id)
                != authored.audio_cue_id.as_deref().unwrap_or(&authored.cue_id)
            || selected.stanza_break_before != authored.stanza_break_before
        {
            bail!("poem wording, cue scope, or stanza breaks differ from selected source");
        }
    }
    if source.schema == SOURCE_TEXT_SCHEMA {
        if !source.display_units.is_empty() {
            bail!("legacy source text cannot claim V2 display units");
        }
    } else {
        if display_scope_ids.len() != 2
            || source.display_units.len() != 2
            || source.display_units[0].role != "poem-title"
            || source.display_units[1].role != "poet-credit"
            || invocation.byline.is_none()
        {
            bail!("post-poem display requires exact title and credit units");
        }
        for (index, unit) in source.display_units.iter().enumerate() {
            if unit.source_scope_id != display_scope_ids[index]
                || unit.source_text.trim().is_empty()
                || unit.editable_text.trim().is_empty()
                || unit.source_text_sha256 != hex_sha(unit.source_text.as_bytes())
                || unit.editable_text_sha256 != hex_sha(unit.editable_text.as_bytes())
            {
                bail!("post-poem source and editable display text differ");
            }
        }
        if source.display_units[0].editable_text != invocation.title
            || Some(source.display_units[1].editable_text.as_str()) != invocation.byline.as_deref()
        {
            bail!("post-poem title or credit differs from selected display units");
        }
    }
    Ok(())
}

fn hex_sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
        || !matches!(
            template.kind.as_str(),
            "opening-poem" | "internal-poem" | "chapter-title"
        )
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
        if invocation.byline.is_some() || template.byline.is_some() {
            bail!("chapter card cannot carry a poem byline");
        }
        if !invocation.lines.is_empty() || !ordered_cues.is_empty() || !alignments.is_empty() {
            bail!("chapter card cannot carry poem lines or native cue clocks");
        }
        let duration = template
            .fixed_duration_seconds
            .ok_or_else(|| anyhow::anyhow!("chapter template needs fixed duration"))?;
        if duration == 0
            || template.panel.is_some()
            || template.post_poem_title_duration_ms.is_some()
        {
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
    if invocation.byline.is_some() != template.byline.is_some() {
        bail!("poem byline content and template style must agree");
    }
    if template.post_poem_title_duration_ms.is_some() && invocation.byline.is_none() {
        bail!("post-poem title route requires a poet credit");
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
    let end = if let Some(duration_ms) = template.post_poem_title_duration_ms {
        if !(10..=30_000).contains(&duration_ms) || duration_ms % 10 != 0 {
            bail!("post-poem title duration must be 10 ms to 30 s in whole centiseconds");
        }
        let tail_samples = u64::from(duration_ms)
            .checked_mul(u64::from(rate))
            .context("post-poem title duration overflows native sample clock")?
            / 1000;
        let end = cursor
            .checked_add(tail_samples)
            .context("post-poem title end overflows native sample clock")?;
        if centiseconds(end, rate) <= centiseconds(cursor, rate) {
            bail!("post-poem title has no visible duration on ASS clock");
        }
        end
    } else {
        cursor
    };
    let mut entrances = Vec::new();
    let mut used = BTreeSet::new();
    for line in &invocation.lines {
        escape(&line.text)?;
        let audio_cue_id = line.audio_cue_id.as_deref().unwrap_or(&line.cue_id);
        let base = starts
            .get(audio_cue_id)
            .ok_or_else(|| anyhow::anyhow!("line references cue outside poem"))?;
        let alignment = &alignments[audio_cue_id];
        let marker = *alignment
            .semantic_markers
            .get(&line.semantic_trigger_id)
            .ok_or_else(|| anyhow::anyhow!("line semantic marker missing"))?;
        if marker >= alignment.cue_end_sample
            || !used.insert((audio_cue_id, &line.semantic_trigger_id))
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
    let mut displayed_lines = Vec::new();
    let available_width = template.canvas_width - template.body_x - 20;
    let characters_per_row =
        (u64::from(available_width) * 100 / (u64::from(template.body_size) * 47)) as usize;
    if characters_per_row == 0 {
        bail!("poem panel has no usable text width");
    }
    for line in &invocation.lines {
        let mut rows = Vec::new();
        let mut row = String::new();
        for word in line.text.split_whitespace() {
            if word.chars().count() > characters_per_row {
                bail!("poem word exceeds template panel width");
            }
            if !row.is_empty()
                && row.chars().count() + 1 + word.chars().count() > characters_per_row
            {
                rows.push(row);
                row = String::new();
            }
            if !row.is_empty() {
                row.push(' ');
            }
            row.push_str(word);
        }
        if !row.is_empty() {
            rows.push(row);
        }
        if line.stanza_break_before {
            y += template.line_spacing / 2;
        }
        if y + template.body_size + (rows.len().saturating_sub(1) as u32 * template.line_spacing)
            >= template.canvas_height
        {
            bail!("poem text exceeds template canvas");
        }
        positions.push(y);
        displayed_lines.push(
            rows.iter()
                .map(|row| escape(row))
                .collect::<Result<Vec<_>>>()?
                .join("\\N"),
        );
        y += template.line_spacing * rows.len() as u32;
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
    let title_start = if end == cursor { 0 } else { cursor };
    ass.push_str(&event(
        title_start,
        end,
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
    if let (Some(byline), Some(style)) = (&invocation.byline, &template.byline) {
        if style.font_size == 0
            || style.x < panel.x
            || style.x >= template.canvas_width
            || style.y >= template.canvas_height
        {
            bail!("poem byline style does not fit canvas");
        }
        ass.push_str(&event(
            title_start,
            end,
            rate,
            "Text",
            &format!(
                "{{\\pos({},{})\\fs{}\\1c{}}}{}",
                style.x,
                style.y,
                style.font_size,
                color(style.rgb),
                escape(byline)?
            ),
        ));
    }
    for boundary in 0..invocation.lines.len() {
        let start = entrances[boundary];
        let end = entrances.get(boundary + 1).copied().unwrap_or(cursor);
        for index in 0..invocation.lines.len() {
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
                    displayed_lines[index]
                ),
            ));
        }
    }
    Ok(CompiledLayer {
        schema: "reel.compiled-editable-layer.v1".into(),
        template_id: template.template_id.clone(),
        language: invocation.language.clone(),
        duration_samples: end,
        sample_rate: rate,
        ass,
    })
}

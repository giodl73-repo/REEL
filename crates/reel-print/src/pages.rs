use crate::Page;
use anyhow::{Context, Result, ensure};
use std::sync::Arc;
use svg2pdf::usvg::{self, fontdb};

pub(crate) enum Content {
    Svg(String),
    Pdf(Arc<lopdf::Document>),
}
pub(crate) struct Sheet {
    pub description: Page,
    pub content: Content,
}
pub(crate) struct Fonts {
    options: usvg::Options<'static>,
    body: fontdb::ID,
}
impl Fonts {
    pub fn new(family: &str, sources: &[Vec<u8>]) -> Result<Self> {
        ensure!(
            !family.is_empty() && family.len() <= 128,
            "explicit body font required"
        );
        let mut options = usvg::Options {
            font_family: family.into(),
            ..Default::default()
        };
        for raw in sources {
            options.fontdb_mut().load_font_data(raw.clone());
        }
        options.fontdb_mut().set_serif_family(family);
        let body = options
            .fontdb
            .query(&fontdb::Query {
                families: &[fontdb::Family::Name(family)],
                ..Default::default()
            })
            .context("body font is not present in captured font files")?;
        for face in options.fontdb.faces() {
            let permitted = options
                .fontdb
                .with_face_data(face.id, |raw, index| {
                    let font = ttf_parser::Face::parse(raw, index).ok()?;
                    Some(font.is_subsetting_allowed() && font.is_outline_embedding_allowed())
                })
                .flatten()
                .unwrap_or(false);
            ensure!(permitted, "font embedding/subsetting permission is absent");
        }
        Ok(Self { options, body })
    }
    fn width(&self, value: &str, size: f32) -> Result<f32> {
        self.options
            .fontdb
            .with_face_data(self.body, |raw, index| -> Result<f32> {
                let face = ttf_parser::Face::parse(raw, index)
                    .map_err(|_| anyhow::anyhow!("invalid body font"))?;
                let mut units = 0u64;
                for c in value.chars() {
                    let glyph = face
                        .glyph_index(c)
                        .context("body text glyph missing from selected font")?;
                    units += face.glyph_hor_advance(glyph).unwrap_or(0) as u64;
                }
                Ok(units as f32 * size / face.units_per_em() as f32)
            })
            .context("body font unavailable")?
    }
    fn glyphs(&self, svg: &str) -> Result<()> {
        let doc = roxmltree::Document::parse(svg)?;
        for node in doc.descendants().filter(|n| {
            n.is_text()
                && n.parent()
                    .is_some_and(|p| matches!(p.tag_name().name(), "text" | "tspan"))
        }) {
            for c in node
                .text()
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_whitespace())
            {
                ensure!(
                    self.options.fontdb.faces().any(|f| self
                        .options
                        .fontdb
                        .with_face_data(f.id, |raw, index| ttf_parser::Face::parse(raw, index)
                            .ok()
                            .is_some_and(|f| f.glyph_index(c).is_some()))
                        .unwrap_or(false)),
                    "captured fonts have no glyph for U+{:04X}",
                    c as u32
                );
            }
        }
        Ok(())
    }
    pub fn render(&mut self, svg: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        self.glyphs(svg)?;
        let tree = usvg::Tree::from_str(svg, &self.options)?;
        let pdf = svg2pdf::to_pdf(
            &tree,
            svg2pdf::ConversionOptions::default(),
            svg2pdf::PageOptions::default(),
        )
        .map_err(|e| anyhow::anyhow!("SVG to PDF conversion failed: {e:?}"))?;
        let size = tree.size().to_int_size();
        ensure!(
            size.width() <= 2048 && size.height() <= 2048,
            "render page dimensions exceed bound"
        );
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
            .context("page raster allocation failed")?;
        pixmap.fill(resvg::tiny_skia::Color::WHITE);
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        Ok((pdf, pixmap.encode_png()?))
    }
}
pub(crate) fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn label(value: &str) -> Result<()> {
    ensure!(
        value.len() <= 1024 && !value.chars().any(|c| c.is_control()),
        "invalid print label"
    );
    Ok(())
}
fn row(text: &str, x: f32, y: f32, size: f32) -> String {
    format!(
        "<text xml:space=\"preserve\" x=\"{x}\" y=\"{y}\" font-size=\"{size}\">{}</text>",
        escape(text)
    )
}
fn wrap(fonts: &Fonts, value: &str, width: f32, size: f32) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = value;
    while !rest.is_empty() {
        let mut end = 0;
        let mut space = None;
        let mut used = 0.0;
        for (i, c) in rest.char_indices() {
            let next = i + c.len_utf8();
            used += fonts.width(&c.to_string(), size)?;
            if used > width {
                break;
            }
            end = next;
            if c.is_whitespace() {
                space = Some(next);
            }
        }
        let end = if end == rest.len() {
            end
        } else {
            space.unwrap_or(end)
        };
        ensure!(end > 0, "a glyph exceeds the column width");
        out.push(rest[..end].into());
        rest = &rest[end..];
    }
    if out.is_empty() {
        out.push(String::new());
    }
    Ok(out)
}
fn page_svg(width: f32, height: f32, body: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><g fill=\"#23352f\">{body}</g></svg>"
    )
}
pub fn text(
    fonts: &Fonts,
    id: &str,
    title: &str,
    columns: &[String],
    parallel: bool,
    labels: Option<[&String; 2]>,
) -> Result<Vec<Sheet>> {
    label(title)?;
    ensure!(
        columns.len() == if parallel { 2 } else { 1 },
        "invalid text columns"
    );
    for text in columns {
        ensure!(
            text.len() <= 65_536
                && !text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
            "text exceeds print bound"
        );
    }
    let mut lines = Vec::<[String; 2]>::new();
    if let Some(labels) = labels {
        label(labels[0])?;
        label(labels[1])?;
        ensure!(
            fonts.width(labels[0], 12.0)? <= 246.0 && fonts.width(labels[1], 12.0)? <= 246.0,
            "parallel labels exceed column width"
        );
    }
    let source: Vec<_> = columns
        .iter()
        .map(|c| c.lines().collect::<Vec<_>>())
        .collect();
    let count = source.iter().map(Vec::len).max().unwrap_or(0);
    let width = if parallel { 246.0 } else { 516.0 };
    for index in 0..count {
        let left = wrap(
            fonts,
            source[0].get(index).copied().unwrap_or(""),
            width,
            12.0,
        )?;
        let right = if parallel {
            wrap(
                fonts,
                source[1].get(index).copied().unwrap_or(""),
                width,
                12.0,
            )?
        } else {
            vec![]
        };
        for n in 0..left.len().max(right.len()) {
            lines.push([
                left.get(n).cloned().unwrap_or_default(),
                right.get(n).cloned().unwrap_or_default(),
            ]);
        }
    }
    let headings = wrap(fonts, title, 516.0, 20.0)?;
    ensure!(headings.len() <= 4, "title exceeds page heading bound");
    let label_y = 60.0 + headings.len() as f32 * 25.0 + 20.0;
    let start = label_y + if labels.is_some() { 22.0 } else { 0.0 };
    let rows = ((730.0 - start) / 18.0).floor() as usize;
    ensure!(
        lines.len().div_ceil(rows) <= 256,
        "text page count exceeds bound"
    );
    let mut out = Vec::new();
    if lines.is_empty() {
        lines.push([String::new(), String::new()]);
    }
    for (index, chunk) in lines.chunks(rows).enumerate() {
        let mut body = String::new();
        for (n, heading) in headings.iter().enumerate() {
            body += &row(heading, 48.0, 60.0 + n as f32 * 25.0, 20.0);
        }
        if let Some(labels) = labels {
            body += &row(labels[0], 48.0, label_y, 12.0);
            body += &row(labels[1], 318.0, label_y, 12.0);
        }
        for (n, line) in chunk.iter().enumerate() {
            body += &row(&line[0], 48.0, start + n as f32 * 18.0, 12.0);
            if parallel {
                body += &row(&line[1], 318.0, start + n as f32 * 18.0, 12.0);
            }
        }
        body += &row(
            &format!("Private working edition | {}", index + 1),
            48.0,
            766.0,
            8.0,
        );
        out.push(Sheet {
            description: Page {
                entry: id.into(),
                kind: if parallel { "parallel-text" } else { "text" }.into(),
                width: 612.0,
                height: 792.0,
                source_band: None,
            },
            content: Content::Svg(page_svg(612.0, 792.0, &body)),
        });
    }
    Ok(out)
}

pub fn notation(
    fonts: &Fonts,
    id: &str,
    title: &str,
    raw: &[u8],
    per_page: usize,
) -> Result<Vec<Sheet>> {
    label(title)?;
    ensure!(
        fonts.width(title, 18.0)? <= 720.0,
        "notation title exceeds page width"
    );
    ensure!(
        raw.len() <= 4 * 1024 * 1024 && (1..=4).contains(&per_page),
        "native score input/layout exceeds bound"
    );
    let svg = std::str::from_utf8(raw)?;
    let doc = roxmltree::Document::parse(svg)?;
    ensure!(
        doc.descendants().count() <= 100_000,
        "native SVG node budget exceeded"
    );
    let root = doc.root_element();
    ensure!(
        root.tag_name().name() == "svg" && root.attribute("width") == Some("1100"),
        "unsupported native score viewport"
    );
    let height: f32 = root
        .attribute("height")
        .context("native viewport height absent")?
        .parse()?;
    ensure!(
        height.is_finite() && (120.0..=65536.0).contains(&height),
        "invalid native viewport height"
    );
    if let Some(viewbox) = root.attribute("viewBox") {
        let bounds = viewbox
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|v| !v.is_empty())
            .map(str::parse::<f32>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            bounds == [0.0, 0.0, 1100.0, height],
            "unsupported native coordinate transform"
        );
    }
    for n in doc.descendants().filter(|n| n.is_element()) {
        ensure!(
            !matches!(
                n.tag_name().name(),
                "image" | "use" | "script" | "foreignObject" | "filter" | "a"
            ),
            "external or active SVG content refused"
        );
        ensure!(
            !n.attributes().any(|a| a.name() == "href"
                || a.name().starts_with("on")
                || a.value().contains("url(")
                || a.value().contains("://")),
            "external SVG reference refused"
        );
        if n.tag_name().name() == "style" {
            let style = n.text().unwrap_or("");
            ensure!(
                !style.contains("url(") && !style.contains('@') && !style.contains("://"),
                "external CSS refused"
            );
        }
    }
    let mut starts = Vec::<f32>::new();
    for line in doc.descendants().filter(|n| {
        n.has_tag_name("line")
            && n.attribute("class") == Some("staff")
            && n.attribute("x1") == Some("80")
            && n.attribute("x2") == Some("1060")
    }) {
        ensure!(
            !line.ancestors().any(|n| n.attribute("transform").is_some()),
            "transformed staff grid is not supported"
        );
        let y: f32 = line.attribute("y1").context("staff y missing")?.parse()?;
        ensure!(
            y.is_finite() && line.attribute("y1") == line.attribute("y2"),
            "invalid staff geometry"
        );
        starts.push(y);
    }
    starts.sort_by(f32::total_cmp);
    ensure!(
        !starts.is_empty() && starts.len() % 5 == 0 && starts.len() <= 1280,
        "incomplete or oversized staff grid"
    );
    for (index, y) in starts.iter().enumerate() {
        ensure!(
            (*y - (80.0 + (index / 5) as f32 * 150.0 + (index % 5) as f32 * 10.0)).abs() < 0.01,
            "unsupported staff grid spacing"
        );
    }
    let systems = starts.len() / 5;
    ensure!(
        height >= 40.0 + 150.0 * systems as f32,
        "native viewport cuts its staff grid"
    );
    let inner_start =
        svg[root.range()].find('>').context("invalid SVG root")? + root.range().start + 1;
    let inner_end = svg.rfind("</svg>").context("invalid SVG close")?;
    let inner = &svg[inner_start..inner_end];
    let attributes = root
        .attributes()
        .filter(|a| {
            !matches!(
                a.name(),
                "width" | "height" | "viewBox" | "x" | "y" | "overflow"
            )
        })
        .map(|a| format!(" {}=\"{}\"", a.name(), escape(a.value())))
        .collect::<String>();
    let mut out = Vec::new();
    for first in (0..systems).step_by(per_page) {
        let start = 40.0 + 150.0 * first as f32;
        let end = 40.0 + 150.0 * (first + per_page).min(systems) as f32;
        let scale = 720.0 / 1100.0;
        let body = format!(
            "{}{}<svg{attributes} x=\"36\" y=\"78\" width=\"720\" height=\"{}\" viewBox=\"0 {start} 1100 {}\" overflow=\"hidden\">{inner}</svg>{}",
            row(title, 36.0, 34.0, 18.0),
            row(
                "Native notation study | musical review pending",
                36.0,
                57.0,
                9.0
            ),
            (end - start) * scale,
            end - start,
            row(
                "Private working edition | source notation retained",
                36.0,
                586.0,
                8.0
            )
        );
        out.push(Sheet {
            description: Page {
                entry: id.into(),
                kind: "native-score".into(),
                width: 792.0,
                height: 612.0,
                source_band: Some([start, end]),
            },
            content: Content::Svg(page_svg(792.0, 612.0, &body)),
        });
    }
    Ok(out)
}

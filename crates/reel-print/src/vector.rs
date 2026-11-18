//! Shared, in-memory rendering of captured, self-contained vector pages.
//! No files/providers are opened and no title page is inserted.
use crate::{pages::Fonts, pdf};
use anyhow::{Result, ensure};
use std::sync::Arc;

pub struct Renderer {
    fonts: Fonts,
}
/// Assemble captured passive reading PDFs in supplied order, without adding pages.
/// Callers retain responsibility for source hashes, edition identity and rights.
pub fn merge_reading_pdfs(parts: &[&[u8]], max_pages: usize, max_bytes: u64) -> Result<Vec<u8>> {
    ensure!(
        (1..=128).contains(&parts.len())
            && (1..=512).contains(&max_pages)
            && (1..=128 * 1024 * 1024).contains(&max_bytes),
        "reading assembly limits refused"
    );
    let mut used = 0u64;
    let mut pages = 0usize;
    let mut documents = Vec::new();
    for raw in parts {
        used = used
            .checked_add(raw.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("reading input overflow"))?;
        ensure!(used <= max_bytes, "reading input bound exceeded");
        let sheets = pdf::retained("reading-part", raw)?;
        ensure!(!sheets.is_empty(), "empty reading part refused");
        pages += sheets.len();
        ensure!(pages <= max_pages, "reading page bound exceeded");
        let crate::pages::Content::Pdf(document) = &sheets[0].content else {
            anyhow::bail!("reading PDF required");
        };
        documents.push(document.clone());
    }
    let output = pdf::merge(documents)?;
    ensure!(
        output.len() as u64 <= max_bytes,
        "reading output bound exceeded"
    );
    Ok(output)
}
pub struct RenderedPage {
    pub png: Vec<u8>,
}
pub struct RenderedBook {
    pub pdf: Vec<u8>,
    pub pages: Vec<RenderedPage>,
}
pub struct RasterLayer {
    pub png: Vec<u8>,
    pub rect: [u32; 4],
}
pub struct RasterPage {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<RasterLayer>,
    pub frames: String,
    pub lettering: String,
}
pub struct RenderedRasterPage {
    pub clean_png: Vec<u8>,
    pub reading_png: Vec<u8>,
}
pub struct RenderedRasterBook {
    pub pdf: Vec<u8>,
    pub pages: Vec<RenderedRasterPage>,
}

impl Renderer {
    pub fn new(family: &str, fonts: &[Vec<u8>]) -> Result<Self> {
        ensure!(
            (1..=8).contains(&fonts.len())
                && fonts
                    .iter()
                    .all(|f| !f.is_empty() && f.len() <= 16 * 1024 * 1024),
            "captured font budget exceeded"
        );
        Ok(Self {
            fonts: Fonts::new(family, fonts)?,
        })
    }
    /// Conservative unshaped advance check; actual reading review remains required.
    pub fn text_width(&self, text: &str, size: f32) -> Result<f32> {
        ensure!(
            text.len() <= 8192 && size.is_finite() && (1.0..=256.0).contains(&size),
            "text measurement bound exceeded"
        );
        self.fonts.width(text, size)
    }
    pub fn render_raster_book(
        &self,
        pages: &[RasterPage],
        max_output_bytes: u64,
    ) -> Result<RenderedRasterBook> {
        use resvg::tiny_skia::{Color, Pixmap, PixmapPaint, Transform};
        ensure!(
            (1..=32).contains(&pages.len()) && (1..=128 * 1024 * 1024).contains(&max_output_bytes),
            "raster book budget exceeded"
        );
        let mut documents = Vec::new();
        let mut rendered = Vec::new();
        let mut used = 0u64;
        for page in pages {
            ensure!(
                (72..=2048).contains(&page.width)
                    && (72..=2048).contains(&page.height)
                    && page.layers.len() <= 128,
                "raster page dimensions/count refused"
            );
            let mut pixels = Pixmap::new(page.width, page.height)
                .ok_or_else(|| anyhow::anyhow!("page allocation failed"))?;
            pixels.fill(Color::from_rgba8(255, 253, 246, 255));
            let mut layer_pixels = 0u64;
            for layer in &page.layers {
                let raw = &layer.png;
                ensure!(
                    raw.len() >= 24
                        && raw.len() <= 16 * 1024 * 1024
                        && raw.starts_with(b"\x89PNG\r\n\x1a\n")
                        && &raw[12..16] == b"IHDR",
                    "PNG layer refused"
                );
                let width = u32::from_be_bytes(raw[16..20].try_into()?);
                let height = u32::from_be_bytes(raw[20..24].try_into()?);
                layer_pixels += width as u64 * height as u64;
                ensure!(
                    (1..=2048).contains(&width)
                        && (1..=2048).contains(&height)
                        && layer_pixels <= 16 * 1024 * 1024,
                    "decoded layer pixel bound exceeded"
                );
                let [x, y, w, h] = layer.rect;
                ensure!(
                    w > 0
                        && h > 0
                        && x.checked_add(w).is_some_and(|n| n <= page.width)
                        && y.checked_add(h).is_some_and(|n| n <= page.height),
                    "raster placement refused"
                );
                let layer = Pixmap::decode_png(raw)?;
                let transform = Transform::from_row(
                    w as f32 / width as f32,
                    0.0,
                    0.0,
                    h as f32 / height as f32,
                    x as f32,
                    y as f32,
                );
                pixels.draw_pixmap(
                    0,
                    0,
                    layer.as_ref(),
                    &PixmapPaint::default(),
                    transform,
                    None,
                );
            }
            for source in [&page.frames, &page.lettering] {
                check_svg(source)?;
            }
            let frames = self.fonts.transparent(&page.frames)?;
            ensure!(
                frames.width() == page.width && frames.height() == page.height,
                "frame canvas differs"
            );
            pixels.draw_pixmap(
                0,
                0,
                frames.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            let clean_png = pixels.encode_png()?;
            let letters = self.fonts.transparent(&page.lettering)?;
            ensure!(
                letters.width() == page.width && letters.height() == page.height,
                "lettering canvas differs"
            );
            pixels.draw_pixmap(
                0,
                0,
                letters.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            let reading_png = pixels.encode_png()?;
            used = used
                .checked_add((clean_png.len() + reading_png.len()) as u64)
                .ok_or_else(|| anyhow::anyhow!("raster byte overflow"))?;
            ensure!(
                used <= max_output_bytes,
                "raster output byte bound exceeded"
            );
            documents.push(Arc::new(pdf::raster_page(&pixels)?));
            rendered.push(RenderedRasterPage {
                clean_png,
                reading_png,
            });
        }
        let pdf = pdf::merge(documents)?;
        ensure!(
            used.checked_add(pdf.len() as u64)
                .is_some_and(|n| n <= max_output_bytes),
            "raster book output byte bound exceeded"
        );
        Ok(RenderedRasterBook {
            pdf,
            pages: rendered,
        })
    }
    pub fn render_pages(
        &mut self,
        pages: &[String],
        max_output_bytes: u64,
    ) -> Result<RenderedBook> {
        ensure!(
            (1..=64).contains(&pages.len()) && (1..=256 * 1024 * 1024).contains(&max_output_bytes),
            "vector book budget exceeded"
        );
        let mut documents = Vec::new();
        let mut rendered = Vec::new();
        let mut used = 0u64;
        for svg in pages {
            check_svg(svg)?;
            let (pdf, png) = self.fonts.render(svg)?;
            used = used
                .checked_add((pdf.len() + png.len()) as u64)
                .ok_or_else(|| anyhow::anyhow!("vector byte overflow"))?;
            ensure!(used <= max_output_bytes, "vector output budget exceeded");
            documents.push(Arc::new(lopdf::Document::load_mem(&pdf)?));
            rendered.push(RenderedPage { png });
        }
        let pdf = pdf::merge(documents)?;
        ensure!(
            used.checked_add(pdf.len() as u64)
                .is_some_and(|n| n <= max_output_bytes),
            "merged vector output budget exceeded"
        );
        Ok(RenderedBook {
            pdf,
            pages: rendered,
        })
    }
}

/// A small passive SVG subset for the new captured-page entry. Existing score
/// admission remains unchanged. Raster assets use the separate explicit PNG path.
fn check_svg(svg: &str) -> Result<()> {
    ensure!(
        svg.len() <= 32 * 1024 * 1024,
        "vector source budget exceeded"
    );
    let doc = roxmltree::Document::parse(svg)?;
    ensure!(
        doc.root_element().has_tag_name("svg") && doc.descendants().count() <= 100_000,
        "vector structure refused"
    );
    for dimension in ["width", "height"] {
        let size: u32 = doc
            .root_element()
            .attribute(dimension)
            .ok_or_else(|| anyhow::anyhow!("numeric vector extent required"))?
            .parse()?;
        ensure!(
            (72..=2048).contains(&size),
            "vector extent outside qualified bound"
        );
    }
    for node in doc.descendants().filter(|n| n.is_element()) {
        ensure!(
            [
                "svg", "g", "defs", "clipPath", "mask", "rect", "circle", "ellipse", "path",
                "line", "polyline", "polygon", "text", "tspan", "title", "desc"
            ]
            .contains(&node.tag_name().name()),
            "unsupported vector element"
        );
        for attr in node.attributes() {
            let value = attr.value();
            ensure!(
                !attr.name().starts_with("on") && attr.name() != "base" && attr.name() != "style",
                "active vector attribute refused"
            );
            ensure!(attr.name() != "href", "external image/reference refused");
            ensure!(
                !value.contains("://")
                    && !value.contains("data:")
                    && (!value.contains("url(")
                        || (value.starts_with("url(#") && value.ends_with(')'))),
                "external vector reference refused"
            );
        }
    }
    Ok(())
}

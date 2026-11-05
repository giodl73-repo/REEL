//! Fixed phone-readable profiles, authored wording and explicitly bound clocks.
use super::*;
pub(super) struct Compiled {
    pub ass: String,
    pub ledger: Value,
    pub width: u32,
    pub height: u32,
    pub paper: String,
}
pub(super) fn color(v: &str) -> Result<String> {
    let v = v.strip_prefix('#').unwrap_or(v);
    if v.len() != 6 || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid album palette");
    }
    Ok(v.to_uppercase())
}
fn bgr(v: &str) -> Result<String> {
    let c = color(v)?;
    Ok(format!("{}{}{}", &c[4..6], &c[2..4], &c[..2]))
}
fn safe(v: &str) -> Result<()> {
    if v.trim().is_empty()
        || v.len() > 4096
        || v.chars()
            .any(|c| c.is_control() || matches!(c, '{' | '}' | '\\'))
    {
        bail!("empty reading or layout-control text");
    }
    Ok(())
}
fn wrap(v: &str, width: usize) -> Result<Vec<String>> {
    safe(v)?;
    let mut out = Vec::new();
    let mut line = String::new();
    for word in v.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push(line);
    Ok(out)
}
fn labels(v: &str, width: usize) -> Result<Vec<String>> {
    wrap(
        &v.replace('\\', "/").replace('{', "(").replace('}', ")"),
        width,
    )
}
pub(super) fn lines(pair: &Value, key: &str) -> Result<Vec<String>> {
    let original = text(pair, key)?;
    let fallback = wrap(original, 24)?;
    if let Some(display) = pair.get("display_lines") {
        let values = display[key]
            .as_array()
            .context("explicit source wrapping missing")?;
        if values.is_empty() || values.len() > 7 {
            bail!("explicit reading rows exceed template");
        }
        let result: Vec<String> = values
            .iter()
            .map(|v| {
                v.as_str()
                    .context("reading row must be text")
                    .map(str::to_owned)
            })
            .collect::<Result<_>>()?;
        for v in &result {
            safe(v)?;
        }
        if result.join(" ").split_whitespace().collect::<Vec<_>>()
            != original.split_whitespace().collect::<Vec<_>>()
        {
            bail!("explicit wrapping changed source wording");
        }
        return Ok(result);
    }
    Ok(fallback)
}
pub(super) fn clock(frame: u64) -> String {
    let n = frame * 100;
    let ticks =
        n / RATE + u64::from(n % RATE * 2 > RATE || (n % RATE * 2 == RATE && n / RATE % 2 == 1));
    format!(
        "{}:{:02}:{:02}.{:02}",
        ticks / 360000,
        ticks / 6000 % 60,
        ticks / 100 % 60,
        ticks % 100
    )
}
struct Faces {
    data: Vec<(Vec<u8>, String)>,
}
impl Faces {
    fn new(q: &Request) -> Result<Self> {
        if q.fonts.iter().map(|f| f.file.bytes).sum::<u64>() > 32 * 1024 * 1024 {
            bail!("font byte budget");
        }
        let mut data = Vec::new();
        for font in &q.fonts {
            if font.file.bytes > 16 * 1024 * 1024
                || !label(&font.family)
                || font.family.contains([',', '{', '}', '\\'])
            {
                bail!("font declaration invalid");
            }
            let raw = fs::read(&font.file.path)?;
            let face = ttf_parser::Face::parse(&raw, 0).map_err(|_| {
                anyhow::anyhow!("font is not a supported first-face OpenType input")
            })?;
            let matched = face
                .names()
                .into_iter()
                .filter(|n| matches!(n.name_id, 1 | 16))
                .filter_map(|n| n.to_string())
                .any(|n| n.eq_ignore_ascii_case(&font.family));
            if !matched || face.units_per_em() == 0 {
                bail!("declared font family differs from pinned file");
            }
            data.push((raw, font.family.clone()));
        }
        Ok(Self { data })
    }
    fn width(&self, family: &str, bold: bool, text: &str, px: u32) -> Result<f64> {
        let mut chosen = None;
        for (raw, name) in &self.data {
            if name.eq_ignore_ascii_case(family) {
                let face = ttf_parser::Face::parse(raw, 0)
                    .map_err(|_| anyhow::anyhow!("font parse failed"))?;
                if face.is_bold() == bold && !face.is_italic() {
                    chosen = Some(face);
                    break;
                }
            }
        }
        let face = chosen.context("requested family/weight is not in declared font set")?;
        let mut width = 0u64;
        for c in text.chars() {
            let id = face
                .glyph_index(c)
                .context("declared font lacks a requested glyph")?;
            width += u64::from(
                face.glyph_hor_advance(id)
                    .context("glyph has no horizontal metrics")?,
            );
        }
        Ok(width as f64 * px as f64 / face.units_per_em() as f64)
    }
}
struct Writer<'a> {
    text: String,
    faces: &'a Faces,
    body: &'a str,
    heading: &'a str,
    bold: bool,
    title_bold: bool,
}
#[derive(Clone, Copy)]
enum FaceRole {
    Body,
    Heading,
    TrackTitle,
    PageNumber,
}
struct TextStyle<'a> {
    size: u32,
    ink: &'a str,
    role: FaceRole,
    align: u8,
    max_width: f64,
}
impl Writer<'_> {
    fn text(
        &mut self,
        (a, b): (u64, u64),
        (x, y): (u32, u32),
        value: &str,
        style: TextStyle<'_>,
    ) -> Result<()> {
        let TextStyle {
            size,
            ink,
            role,
            align,
            max_width,
        } = style;
        if value.is_empty() {
            return Ok(());
        }
        safe(value)?;
        let (family, bold) = match role {
            FaceRole::Body => (self.body, false),
            FaceRole::Heading => (self.heading, self.bold),
            FaceRole::TrackTitle => (self.body, self.title_bold),
            FaceRole::PageNumber => (self.heading, false),
        };
        if self.faces.width(family, bold, value, size)? > max_width {
            bail!(
                "reading text exceeds declared template width; provide source-preserving wrapping"
            );
        }
        self.text += &format!(
            "Dialogue: 3,{},{},Default,,0,0,0,,{{\\an{}\\pos({},{})\\fs{}\\1c&H{}&\\b{}\\fn{}}}{}\n",
            clock(a),
            clock(b),
            align,
            x,
            y,
            size,
            bgr(ink)?,
            u8::from(bold),
            family,
            value
        );
        Ok(())
    }
    fn rectangle(
        &mut self,
        (a, b): (u64, u64),
        (x, y, w, h): (u32, u32, u32, u32),
        ink: &str,
    ) -> Result<()> {
        self.text += &format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,{{\\an7\\pos(0,0)\\p1\\bord0\\shad0\\1c&H{}&}}m {} {} l {} {} {} {} {} {}\n",
            clock(a),
            clock(b),
            bgr(ink)?,
            x,
            y,
            x + w,
            y,
            x + w,
            y + h,
            x,
            y + h
        );
        Ok(())
    }
}
fn phrase_frame(p: &Value, name: &str) -> Result<u64> {
    if let Some(v) = p.get(format!("{name}_frame")) {
        return v.as_u64().context("invalid phrase frame");
    }
    let v = p[name].as_f64().context("phrase clock absent")?;
    if !v.is_finite() || !(0.0..=14400.0).contains(&v) {
        bail!("invalid source phrase seconds");
    }
    Ok((v * RATE as f64).round_ties_even() as u64)
}
pub(super) fn compile(q: &Request, a: &Album, reading: Option<&Value>) -> Result<Compiled> {
    let faces = Faces::new(q)?;
    for f in [&q.style.body_family, &q.style.heading_family] {
        if !label(f) || f.contains([',', '{', '}', '\\']) {
            bail!("invalid presentation font family");
        }
    }
    let p = if let Some(reading) = reading {
        let schema = text(reading, "schema")?;
        if !matches!(
            schema,
            "reel.album-reading.v1" | "reel.album-bilingual-reading.v1"
        ) {
            bail!("unsupported album reading schema");
        }
        let binding = reading
            .get("source_album_receipt")
            .or_else(|| reading.get("source_album_check"))
            .context("reading album binding absent")?;
        if binding["sha256"] != q.album_receipt.sha256
            || binding
                .get("bytes")
                .is_some_and(|b| b != &json!(q.album_receipt.bytes))
        {
            bail!("reading uses a different album ledger");
        }
        if let Some(p) = reading.get("palette") {
            let p: Palette = serde_json::from_value(p.clone())?;
            if q.palette.as_ref().is_some_and(|other| other != &p) {
                bail!("two different palettes declared");
            }
            p
        } else {
            q.palette.clone().context("reading palette absent")?
        }
    } else {
        q.palette.clone().context("presentation palette absent")?
    };
    for c in [&p.paper, &p.ink, &p.accent, &p.secondary, &p.rule] {
        color(c)?;
    }
    let (width, height) = if reading.is_some() {
        (1920, 1080)
    } else {
        (1280, 720)
    };
    let mut w = Writer {
        text: format!(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: {width}\nPlayResY: {height}\nWrapStyle: 2\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,{},64,&H00{},&H00{},&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
            q.style.body_family,
            bgr(&p.ink)?,
            bgr(&p.ink)?
        ),
        faces: &faces,
        body: &q.style.body_family,
        heading: &q.style.heading_family,
        bold: q.style.heading_bold,
        title_bold: q.style.track_title_bold,
    };
    let end = a.frames + RATE / 5;
    let mut ledger = Vec::new();
    if let Some(reading) = reading {
        let mode = text(reading, "mode")?;
        if !matches!(mode, "editorial-reading-pages" | "performed-phrase-pages") {
            bail!("invalid reading clock mode");
        }
        let tracks = reading["tracks"]
            .as_array()
            .context("reading tracks missing")?;
        if tracks.len() != a.tracks.len() {
            bail!("reading track count differs");
        }
        if reading
            .get("typography")
            .is_some_and(|value| !value.is_object())
        {
            bail!("typography must be an object");
        }
        let body_px = match reading["typography"].get("body_font_px") {
            Some(v) => v.as_u64().context("body font size must be an integer")?,
            None => 64,
        };
        if !matches!(body_px, 58 | 64) {
            bail!("unsupported phone body size");
        }
        let clear = match reading["typography"].get("clear_instrumental_divider") {
            Some(v) => v.as_bool().context("invalid divider policy")?,
            None => false,
        };
        for (x, y, ww, hh) in [
            (48, 48, 1824, 3),
            (48, 1029, 1824, 3),
            (48, 48, 3, 984),
            (1869, 48, 3, 984),
            (64, 64, 1792, 1),
            (64, 1014, 1792, 1),
        ] {
            w.rectangle((0, end), (x, y, ww, hh), &p.rule)?;
        }
        w.rectangle((0, end), (118, 244, 1684, 3), &p.secondary)?;
        for (x, y, sx, sy) in [
            (48i32, 48i32, 1i32, 1i32),
            (1872, 48, -1, 1),
            (48, 1032, 1, -1),
            (1872, 1032, -1, -1),
        ] {
            for step in 0..3 {
                let xx = x + sx * (12 + step * 9);
                let yy = y + sy * (12 + step * 9);
                w.rectangle(
                    (0, end),
                    (xx.min(xx + sx * 90) as u32, yy as u32, 90, 3),
                    &p.accent,
                )?;
                w.rectangle(
                    (0, end),
                    (xx as u32, yy.min(yy + sy * 90) as u32, 3, 90),
                    &p.accent,
                )?;
            }
        }
        for i in 0..13 {
            let angle = std::f64::consts::PI + i as f64 * std::f64::consts::PI / 12.0;
            let x = 960 + (42.0 * angle.cos()).round_ties_even() as i32;
            let y = 300 + (42.0 * angle.sin()).round_ties_even() as i32;
            w.text += &format!(
                "Dialogue: 1,{},{},Default,,0,0,0,,{{\\an7\\pos(0,0)\\p1\\bord0\\shad0\\1c&H{}&}}m 960 300 l {} {} {} {}\n",
                clock(0),
                clock(end),
                bgr(&p.secondary)?,
                x - 1,
                y,
                x + 1,
                y
            );
        }
        if !clear {
            w.rectangle((0, end), (958, 300, 2, 632), &p.rule)?;
        }
        for (row, v) in labels(&a.title.to_uppercase(), 70)?.iter().enumerate() {
            if row > 0 {
                bail!("album heading exceeds phone header");
            }
            w.text(
                (0, end),
                (118, 83),
                v,
                TextStyle {
                    size: 30,
                    ink: &p.accent,
                    role: FaceRole::Heading,
                    align: 7,
                    max_width: 1684.0,
                },
            )?;
        }
        w.text(
            (0, end),
            (118, 966),
            &q.style.footer,
            TextStyle {
                size: 25,
                ink: &p.accent,
                role: FaceRole::Heading,
                align: 7,
                max_width: 1500.0,
            },
        )?;
        let mut total_pages = 0usize;
        for (i, (reading, track)) in tracks.iter().zip(&a.tracks).enumerate() {
            let id = reading
                .get("asset_id")
                .or_else(|| reading.get("song_id"))
                .and_then(Value::as_str)
                .context("reading track ID missing")?;
            if id != track.id {
                bail!("reading track order/ID differs");
            }
            let mode_track = text(reading, "mode")?;
            let hold_end = if i + 1 < a.tracks.len() {
                track.end
            } else {
                end
            };
            for (row, v) in labels(&track.title, 58)?.iter().enumerate() {
                if row > 1 {
                    bail!("track title exceeds phone header");
                }
                w.text(
                    (track.start, hold_end),
                    (118, 132 + row as u32 * 52),
                    v,
                    TextStyle {
                        size: 52,
                        ink: &p.ink,
                        role: FaceRole::TrackTitle,
                        align: 7,
                        max_width: 1684.0,
                    },
                )?;
            }
            w.text(
                (track.start, hold_end),
                (1800, 87),
                &format!("{:02} / {:02}", i + 1, a.tracks.len()),
                TextStyle {
                    size: 30,
                    ink: &p.secondary,
                    role: FaceRole::Heading,
                    align: 9,
                    max_width: 300.0,
                },
            )?;
            let pages = reading["pages"]
                .as_array()
                .context("reading pages absent")?;
            total_pages += pages.len();
            if total_pages > 10000 {
                bail!("reading page budget");
            }
            if matches!(
                mode_track,
                "instrumental" | "wordless-coro" | "wordless-chorus"
            ) {
                if !pages.is_empty() {
                    bail!("instrumental/wordless track cannot invent sung pages");
                }
                let (left, right) = if mode_track == "instrumental" {
                    (&q.style.instrumental_left, &q.style.instrumental_right)
                } else {
                    (&q.style.wordless_left, &q.style.wordless_right)
                };
                w.text(
                    (track.start, hold_end),
                    (960, 500),
                    left,
                    TextStyle {
                        size: 78,
                        ink: &p.accent,
                        role: FaceRole::Body,
                        align: 5,
                        max_width: 1600.0,
                    },
                )?;
                w.text(
                    (track.start, hold_end),
                    (960, 615),
                    right,
                    TextStyle {
                        size: 38,
                        ink: &p.ink,
                        role: FaceRole::Body,
                        align: 5,
                        max_width: 1600.0,
                    },
                )?;
                ledger.push(json!({"asset_id":id,"mode":mode_track,"pages":0}));
                continue;
            }
            if mode_track != "sung" || pages.is_empty() {
                bail!("sung track requires reading pages");
            }
            if clear {
                w.rectangle((track.start, hold_end), (958, 300, 2, 632), &p.rule)?;
            }
            w.text(
                (track.start, hold_end),
                (118, 205),
                &q.style.byline,
                TextStyle {
                    size: 25,
                    ink: &p.ink,
                    role: FaceRole::Body,
                    align: 7,
                    max_width: 1684.0,
                },
            )?;
            w.text(
                (track.start, hold_end),
                (118, 278),
                &q.style.left_heading,
                TextStyle {
                    size: 30,
                    ink: &p.accent,
                    role: FaceRole::Heading,
                    align: 7,
                    max_width: 792.0,
                },
            )?;
            w.text(
                (track.start, hold_end),
                (1010, 278),
                &q.style.right_heading,
                TextStyle {
                    size: 30,
                    ink: &p.accent,
                    role: FaceRole::Heading,
                    align: 7,
                    max_width: 792.0,
                },
            )?;
            let mut previous = track.start;
            let mut row_counts = Vec::new();
            for (n, page) in pages.iter().enumerate() {
                let start = number(page, "start_frame")?;
                let stop = number(page, "end_frame")?;
                let pairs = page["lines"].as_array().context("reading pairs missing")?;
                if start < previous
                    || stop <= start
                    || start < track.start
                    || stop > track.end
                    || pairs.is_empty()
                    || pairs.len() > 128
                {
                    bail!("reading page clock/order invalid");
                }
                if mode == "performed-phrase-pages" {
                    let expected = pairs
                        .iter()
                        .filter(|line| {
                            !matches!(
                                line["timing_status"].as_str(),
                                Some(
                                    "canonical-only-orphan" | "canonical-context-without-own-clock"
                                )
                            )
                        })
                        .count();
                    let phrases = page["phrase_evidence"]
                        .as_array()
                        .context("performed pages require bound phrase evidence")?;
                    if phrases.is_empty() || phrases.len() != expected {
                        bail!("one performed phrase is required per timed reading pair");
                    }
                    for phrase in phrases {
                        let a = phrase_frame(phrase, "start")?;
                        let b = phrase_frame(phrase, "end")?;
                        if a >= b
                            || b > track.source_end - track.start
                            || start > track.start + a + 2448
                            || stop + 9648 < track.start + b
                        {
                            bail!("reading page misses its performed phrase");
                        }
                    }
                }
                let mut rows = 0;
                for pair in pairs {
                    let (left_key, right_key) = if pair.get("left").is_some() {
                        ("left", "right")
                    } else {
                        ("es", "en")
                    };
                    let left = lines(pair, left_key)?;
                    let right = lines(pair, right_key)?;
                    let count = left.len().max(right.len());
                    if rows + count > 7 {
                        bail!("bilingual reading page exceeds seven rows");
                    }
                    for (i, v) in left.iter().enumerate() {
                        w.text(
                            (start, stop),
                            (118, 340 + (rows + i) as u32 * 78),
                            v,
                            TextStyle {
                                size: body_px as u32,
                                ink: &p.ink,
                                role: FaceRole::Body,
                                align: 7,
                                max_width: 792.0,
                            },
                        )?;
                    }
                    for (i, v) in right.iter().enumerate() {
                        w.text(
                            (start, stop),
                            (1010, 340 + (rows + i) as u32 * 78),
                            v,
                            TextStyle {
                                size: body_px as u32,
                                ink: &p.ink,
                                role: FaceRole::Body,
                                align: 7,
                                max_width: 792.0,
                            },
                        )?;
                    }
                    rows += count;
                }
                w.text(
                    (start, stop),
                    (1800, 966),
                    &format!("{} / {}", n + 1, pages.len()),
                    TextStyle {
                        size: 25,
                        ink: &p.accent,
                        role: FaceRole::PageNumber,
                        align: 9,
                        max_width: 300.0,
                    },
                )?;
                previous = stop;
                row_counts.push(rows);
            }
            ledger.push(json!({"asset_id":id,"mode":mode_track,"pages":pages.len(),"page_rows":row_counts,"timing":mode}));
        }
        Ok(Compiled {
            ass: w.text,
            ledger: json!({"profile":"bilingual-phone-1080p-v1","canvas":[width,height],"body_font_px":body_px,"row_limit":7,"timing":mode,"tracks":ledger}),
            width,
            height,
            paper: color(&p.paper)?,
        })
    } else {
        for (row, v) in labels(&a.title, 48)?.iter().enumerate() {
            if row > 1 {
                bail!("album title exceeds label profile");
            }
            w.text(
                (0, end),
                (640, 140 + row as u32 * 40),
                v,
                TextStyle {
                    size: 34,
                    ink: &p.accent,
                    role: FaceRole::Heading,
                    align: 5,
                    max_width: 1140.0,
                },
            )?;
        }
        for (i, track) in a.tracks.iter().enumerate() {
            let stop = if i + 1 < a.tracks.len() {
                track.end
            } else {
                end
            };
            for (row, v) in labels(&track.title, 40)?.iter().enumerate() {
                if row > 2 {
                    bail!("track title exceeds label profile");
                }
                w.text(
                    (track.start, stop),
                    (640, 360 + row as u32 * 52),
                    v,
                    TextStyle {
                        size: 48,
                        ink: &p.ink,
                        role: FaceRole::Body,
                        align: 5,
                        max_width: 1140.0,
                    },
                )?;
            }
            w.text(
                (track.start, stop),
                (640, 585),
                &format!("{:02} / {:02}", i + 1, a.tracks.len()),
                TextStyle {
                    size: 25,
                    ink: &p.secondary,
                    role: FaceRole::Heading,
                    align: 5,
                    max_width: 400.0,
                },
            )?;
        }
        Ok(Compiled {
            ass: w.text,
            ledger: json!({"profile":"album-labels-720p-v1","canvas":[width,height],"tracks":a.tracks.len()}),
            width,
            height,
            paper: color(&p.paper)?,
        })
    }
}

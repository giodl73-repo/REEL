//! Native, private, bounded adapter for retained `reel.song-short-edit.v1` data.
use super::{Recipe, lyrics, wave};
use anyhow::{Result, bail};
use reel_music::hash::sha256_bytes;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const META: u64 = 16 * 1024 * 1024;
const AUDIO: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Pin {
    sha256: String,
    bytes: u64,
}
fn pin(raw: &[u8]) -> Pin {
    Pin {
        sha256: sha256_bytes(raw),
        bytes: raw.len() as u64,
    }
}
fn bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    let f = File::open(path)?;
    if !f.metadata()?.is_file() || f.metadata()?.len() > max {
        bail!("edition input exceeds byte budget")
    }
    let mut bytes = Vec::new();
    f.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        bail!("edition input exceeds byte budget")
    }
    Ok(bytes)
}
fn checked(root: &Path, name: &str, expected: &Value, max: u64) -> Result<(Vec<u8>, Pin)> {
    let path = root.join(name).canonicalize()?;
    if !path.starts_with(root) {
        bail!("edition source escapes materialized input root")
    }
    let expected: Pin = serde_json::from_value(expected.clone())?;
    if expected.bytes > max
        || expected.sha256.len() != 64
        || !expected
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("edition input pin or budget");
    }
    let bytes = bounded(&path, expected.bytes)?;
    let actual = pin(&bytes);
    if actual != expected {
        bail!("edition input identity differs")
    }
    Ok((bytes, actual))
}
pub(super) fn parse(raw: &[u8]) -> Result<Value> {
    let bytes = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw);
    crate::approval_attestation::ensure_no_duplicate_keys(bytes)?;
    let value: Value = serde_json::from_slice(bytes)?;
    let mut stack = vec![(&value, 0usize)];
    let mut nodes = 0usize;
    while let Some((v, depth)) = stack.pop() {
        nodes += 1;
        if nodes > 2_000_000 || depth > 64 {
            bail!("edition metadata node/depth budget")
        }
        match v {
            Value::Array(a) => stack.extend(a.iter().map(|v| (v, depth + 1))),
            Value::Object(o) => stack.extend(o.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    Ok(value)
}
fn number(v: &Value, key: &str) -> Result<f64> {
    v[key]
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| anyhow::anyhow!("missing finite edition field: {key}"))
}
fn string<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| s.len() <= max)
        .ok_or_else(|| anyhow::anyhow!("invalid edition string: {key}"))
}
fn require_equal(a: &Value, b: &Value, label: &str) -> Result<()> {
    if a.is_null() || b.is_null() || a != b {
        bail!("edition lineage differs: {label}")
    };
    Ok(())
}
fn validate_original(v: &Value, text: &str) -> Result<()> {
    if lyrics::words(text).len() > 4000 {
        bail!("canonical lyric token budget")
    }
    if v["schema"] != "reel.local-song-request.v1"
        || !matches!(v["task"].as_str(), Some("text2music" | "cover"))
        || v["language"] != "es"
        || !(30.0..=600.0).contains(&number(v, "duration_seconds")?)
        || !(v["seed"].is_i64() || v["seed"].is_u64())
        || v["canonical_lyrics"].as_str() != Some(text)
        || v["permissions"]
            != json!({"private_only":true,"external_upload":false,"identifiable_imitation":false,"public_release":false})
    {
        bail!("unsupported original private song request or exact source text")
    }
    if string(v, "caption", 4096)?.trim().is_empty() {
        bail!("original generation brief missing")
    }
    let ceiling = v
        .get("max_duration_seconds")
        .map(|_| number(v, "max_duration_seconds"))
        .transpose()?
        .unwrap_or(600.0);
    if number(v, "duration_seconds")? > ceiling {
        bail!("original duration exceeds its ceiling")
    }
    let repetitions = v
        .get("performance_repetitions")
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| anyhow::anyhow!("repetition count"))
        })
        .transpose()?
        .unwrap_or(1);
    let section = v
        .get("repeat_section_lines")
        .map(|v| {
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| anyhow::anyhow!("repeat section size"))
        })
        .transpose()?;
    let rendered = string(v, "render_lyrics", 131072)?;
    let instrumental = v
        .get("instrumental")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| anyhow::anyhow!("instrumental boolean"))
        })
        .transpose()?
        .unwrap_or(false);
    if instrumental {
        if v["task"] != "text2music"
            || v.get("semantic_planner").is_some_and(|x| x != false)
            || !text.is_empty()
            || rendered != "[Instrumental]"
            || repetitions != 1
            || section.is_some()
        {
            bail!("instrumental original request differs")
        }
    } else {
        let performed = rendered
            .lines()
            .filter(|line| {
                let line = line.trim();
                !(line.starts_with('[')
                    && line.ends_with(']')
                    && line.len() > 2
                    && !line[1..line.len() - 1].contains(']'))
            })
            .collect::<Vec<_>>()
            .join("\n");
        if lyrics::words(&performed) != lyrics::render_words(text, repetitions, section)? {
            bail!("original render formatting changes lyric words")
        }
    }
    if let Some(dtype) = v.get("recognition_dtype") {
        if !matches!(dtype.as_str(), Some("float32" | "float16")) {
            bail!("original recognition dtype")
        }
    }
    if v["task"] == "cover" {
        for key in [
            "guide_audio",
            "guide_request",
            "guide_generation",
            "guide_version_lock",
        ] {
            if !v[key].is_object() {
                bail!("original cover guide proof missing")
            }
        }
        if !(0.2..=1.0).contains(&number(v, "audio_cover_strength")?)
            || v.get("semantic_planner").is_some_and(|x| x != false)
        {
            bail!("original cover conditioning")
        }
        let noise = v
            .get("cover_noise_strength")
            .map(|_| number(v, "cover_noise_strength"))
            .transpose()?
            .unwrap_or(0.0);
        if !(0.0..=1.0).contains(&noise) {
            bail!("original cover source-noise bound")
        }
        if let Some(short) = v.get("guide_short_edit") {
            if short.as_object().is_none_or(|o| {
                o.len() != 2 || !o.contains_key("recipe") || !o.contains_key("receipt")
            }) {
                bail!("original short-guide references")
            }
        }
    } else if v.get("guide_short_edit").is_some() || v.get("cover_noise_strength").is_some() {
        bail!("guide controls require cover source")
    }
    Ok(())
}

struct Loaded {
    recipe: Value,
    recipe_pin: Pin,
    pins: BTreeMap<String, Pin>,
    plan: super::Plan,
    audio: wave::Wave,
    passes: Vec<Value>,
    report: Value,
    recognition_context: Value,
}
fn load(path: &Path) -> Result<Loaded> {
    let path = path.canonicalize()?;
    let root = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("edition input root"))?;
    let raw = bounded(&path, 524288)?;
    let recipe_pin = pin(&raw);
    let recipe = parse(&raw)?;
    if recipe["schema"] != "reel.song-short-edit.v1" || recipe["task"] != "short-edit" {
        bail!("unsupported short-edition request")
    }
    let candidate = string(&recipe, "candidate_id", 256)?;
    if candidate.is_empty() || candidate.chars().any(char::is_control) {
        bail!("edition candidate ID")
    }
    let mut pins = BTreeMap::new();
    let mut jsons = BTreeMap::new();
    for (name, key) in [
        ("source-request.json", "source_request"),
        ("source-analysis.json", "source_analysis"),
        ("source-generation.json", "source_generation"),
    ] {
        let (raw, object) = checked(root, name, &recipe[key], META)?;
        jsons.insert(key, parse(&raw)?);
        pins.insert(key.to_owned(), object);
    }
    let original = &jsons["source_request"];
    let prior = &jsons["source_analysis"];
    let generation = &jsons["source_generation"];
    let (text, text_pin) = checked(root, "source.txt", &original["source"], 131072)?;
    let text = String::from_utf8(text)?;
    validate_original(original, &text)?;
    pins.insert("source".into(), text_pin);
    for key in ["source", "source_lock", "permissions", "caption"] {
        require_equal(&recipe[key], &original[key], key)?;
    }
    if !original["source_lock"].is_object()
        || number(&recipe, "source_generation_duration_seconds")?
            != number(original, "duration_seconds")?
    {
        bail!("edition original source lock/duration differs");
    }
    let lines = prior["lines"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("original canonical line evidence missing"))?;
    let canonical: Vec<_> = lines.iter().map(|l| l["canonical"].as_str()).collect();
    if canonical
        != lyrics::lines(&text)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>()
    {
        bail!("original recognition canonical lines differ")
    }
    if prior["schema"] != "reel.song-lyric-diagnostic.v1"
        || generation["schema"] != "reel.local-song-generation.v1"
    {
        bail!("original recognition/generation contract missing");
    }
    let original_passes = prior["passes"]
        .as_array()
        .filter(|p| !p.is_empty() && p.len() <= 16)
        .ok_or_else(|| anyhow::anyhow!("original independent passes missing or over budget"))?;
    let declared = prior.pointer("/recognition_settings/canonical_prompt");
    if declared.is_some_and(|v| v != false) {
        bail!("original analysis declares prompt-conditioned recognition")
    }
    let mut recognition_context =
        json!({"kind":"original-analysis-settings","settings":prior["recognition_settings"]});
    if let Some(archive) = recipe.get("source_transcripts") {
        let (raw, object) = checked(root, "source-transcripts.zip", archive, META)?;
        let prefix = recipe
            .get("source_transcripts_prefix")
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| anyhow::anyhow!("transcript prefix string"))
            })
            .transpose()?
            .unwrap_or("");
        recognition_context = super::transcripts::context(&raw, original_passes, prefix)?;
        recognition_context["archive"] = serde_json::to_value(&object)?;
        recognition_context["original_analysis_settings"] = prior["recognition_settings"].clone();
        pins.insert("source_transcripts".into(), object);
    } else if declared != Some(&Value::Bool(false)) {
        bail!("original unprompted conditions require pinned raw transcripts")
    }
    let (raw, recording) = checked(root, "source.wav", &recipe["source_recording"], AUDIO)?;
    let request = serde_json::to_value(&pins["source_request"])?;
    if let Some(source) = generation.get("source") {
        require_equal(
            source,
            &serde_json::to_value(&pins["source"])?,
            "original generation text",
        )?;
    }
    let recording_value = serde_json::to_value(&recording)?;
    for proof in [prior, generation] {
        require_equal(&proof["request"], &request, "original proof request")?;
        require_equal(
            &proof["recording"],
            &recording_value,
            "original proof recording",
        )?;
        if let Some(id) = proof.get("candidate_id") {
            require_equal(id, &original["candidate_id"], "original candidate ID")?;
        }
    }
    pins.insert("source_recording".into(), recording);
    let audio = wave::decode(&raw)?;
    drop(raw);
    let geometry: Recipe = serde_json::from_value(
        json!({"segments":recipe["segments"],"crossfade_seconds":recipe["crossfade_seconds"],
        "fade_in_seconds":recipe["fade_in_seconds"],"fade_out_seconds":recipe["fade_out_seconds"],"max_duration_seconds":recipe["max_duration_seconds"]}),
    )?;
    let plan = super::plan(
        &geometry,
        audio.rate,
        (audio.samples.len() / usize::from(audio.channels)) as u64,
    )?;
    if (number(&recipe, "duration_seconds")? - plan.output_frames as f64 / f64::from(audio.rate))
        .abs()
        > 1.0 / f64::from(audio.rate)
    {
        bail!("edition duration differs from its exact frame plan")
    }
    let mut passes = super::remap_passes(&plan, original_passes)?;
    // This assertion is admitted only here, after the original analysis explicitly
    // declares unprompted conditions; the standalone projection core makes none.
    for pass in &mut passes {
        pass["canonical_prompt"] = json!(false);
    }
    let mut report = lyrics::report(&text, &passes)?;
    let cap = recipe
        .get("max_one_word_gap_lines")
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| anyhow::anyhow!("invalid declared word-gap cap"))
        })
        .transpose()?;
    let accepted = report["passed"] == true
        && cap.is_none_or(|cap| report["one_word_gap_lines"].as_u64().unwrap() <= cap);
    report["edition_acceptance"] =
        json!({"line_gate_passed":report["passed"],"max_one_word_gap_lines":cap,"passed":accepted});
    Ok(Loaded {
        recipe,
        recipe_pin,
        pins,
        plan,
        audio,
        passes,
        report,
        recognition_context,
    })
}
fn serialized(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if bytes.len() > 32 * 1024 * 1024 {
        bail!("edition report byte budget")
    };
    Ok(bytes)
}
fn build(loaded: &Loaded) -> Result<(Vec<u8>, Value, Value)> {
    let samples = super::render(&loaded.plan, &loaded.audio.samples, loaded.audio.channels)?;
    let peak = samples
        .iter()
        .fold(0.0f64, |p, s| p.max(f64::from(s.abs())));
    let rms = (samples
        .iter()
        .map(|s| {
            let s = f64::from(*s);
            s * s
        })
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt();
    if !(0.001..=1.001).contains(&peak) || peak == 0.001 || rms <= 0.0001 {
        bail!("edition failed finite non-silent signal bounds")
    }
    let bytes = wave::encode(&samples, loaded.audio.rate, loaded.audio.channels)?;
    let mut report = loaded.report.clone();
    let object = report.as_object_mut().unwrap();
    for (key,value) in json!({"schema":"reel.song-lyric-diagnostic.v1","producer":"reel-song-edition","private_report":true,
        "candidate_id":loaded.recipe["candidate_id"],"request":loaded.recipe_pin,"recording":pin(&bytes),
        "analysis_kind":"source-derived-recognition-on-exact-unchanged-samples","source_recording":loaded.pins["source_recording"],
        "source_analysis":loaded.pins["source_analysis"],"source_request":loaded.pins["source_request"],
        "source_generation":loaded.pins["source_generation"],"source_recognition_context":loaded.recognition_context,
        "passes":loaded.passes,"new_recognition":false,
        "technical":{"seconds":loaded.plan.output_frames as f64/f64::from(loaded.audio.rate),"sample_rate":loaded.audio.rate,
            "channels":loaded.audio.channels,"peak":peak,"rms":rms,"complete_decode":true},
        "requested_complete_poem_passes":1,"source_sample_equality":true,"human_listening":"pending"}).as_object().unwrap() {
        object.insert(key.clone(),value.clone());
    }
    let receipt = json!({"schema":"reel.song-short-edit-receipt.v1","producer":"reel-song-edition","recipe":loaded.recipe_pin,
        "source_recording":loaded.pins["source_recording"],"source_request":loaded.pins["source_request"],
        "source_analysis":loaded.pins["source_analysis"],"source_generation":loaded.pins["source_generation"],"source_transcripts":loaded.pins.get("source_transcripts"),
        "recording":pin(&bytes),"analysis":pin(&serialized(&report)?),"plan":loaded.plan.segments,
        "sample_rate":loaded.audio.rate,"output_frames":loaded.plan.output_frames,"evidence_core_sample_equality":true,
        "new_recognition":false,"tempo_changed":false,"pitch_changed":false,"new_performance":false,"new_human_approval":false,
        "acoustic_continuity":"unreviewed","transformed_regions":"Declared cuts/crossfades/edge fades; word evidence excludes every changed edge"});
    Ok((bytes, report, receipt))
}
pub fn plan(request: &Path) -> Result<Value> {
    let loaded = load(request)?;
    Ok(
        json!({"schema":"reel.song-short-edit-plan.v1","sample_rate":loaded.audio.rate,"channels":loaded.audio.channels,
        "output_frames":loaded.plan.output_frames,"plan":loaded.plan.segments,"maximum_output_wave_bytes":56+loaded.plan.output_frames*u64::from(loaded.audio.channels)*4,
        "estimated_peak_memory_bytes":loaded.pins["source_recording"].bytes+loaded.audio.samples.len() as u64*16+128*1024*1024,
        "lyric_gate_passed":loaded.report["edition_acceptance"]["passed"],"qualifying_lines":loaded.report["qualifying_lines"],"total_lines":loaded.report["total_lines"],
        "new_recognition":false,"new_performance":false,"human_approval":false,"published":false}),
    )
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub fn render(request: &Path, output: &Path) -> Result<Value> {
    if output.exists() {
        bail!("edition output already exists")
    }
    let loaded = load(request)?;
    let (bytes, analysis, receipt) = build(&loaded)?;
    fs::create_dir(output)?;
    write_new(&output.join("recording.wav"), &bytes)?;
    drop(bytes);
    write_new(&output.join("analysis.json"), &serialized(&analysis)?)?;
    write_new(&output.join("edit-receipt.json"), &serialized(&receipt)?)?;
    check(request, output)
}
pub fn check(request: &Path, output: &Path) -> Result<Value> {
    let loaded = load(request)?;
    let (expected, analysis, receipt) = build(&loaded)?;
    let root = output.canonicalize()?;
    let (raw, _) = checked(
        &root,
        "recording.wav",
        &serde_json::to_value(pin(&expected))?,
        AUDIO,
    )?;
    drop(expected);
    let decoded = wave::decode(&raw)?;
    drop(raw);
    if decoded.rate != loaded.audio.rate || decoded.channels != loaded.audio.channels {
        bail!("edition output sample clock changed")
    }
    super::verify_cores(
        &loaded.plan,
        &loaded.audio.samples,
        &decoded.samples,
        decoded.channels,
    )?;
    let (raw, _) = checked(
        &root,
        "analysis.json",
        &receipt["analysis"],
        32 * 1024 * 1024,
    )?;
    if parse(&raw)? != analysis {
        bail!("edition analysis differs")
    }
    let (raw, _) = checked(
        &root,
        "edit-receipt.json",
        &serde_json::to_value(pin(&serialized(&receipt)?))?,
        4 * 1024 * 1024,
    )?;
    if parse(&raw)? != receipt {
        bail!("edition receipt differs")
    }
    Ok(
        json!({"schema":"reel.song-short-edit-check.v1","verified":true,"source_core_sample_equality":true,
        "output_frames":loaded.plan.output_frames,"sample_rate":decoded.rate,"lyric_gate_passed":analysis["edition_acceptance"]["passed"],
        "line_gate_passed":analysis["passed"],
        "qualifying_lines":analysis["qualifying_lines"],"total_lines":analysis["total_lines"],"new_recognition":false,
        "new_performance":false,"human_approval":false,"published":false}),
    )
}

#[cfg(test)]
mod tests;

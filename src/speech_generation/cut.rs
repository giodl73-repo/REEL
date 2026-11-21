//! Source-bound PCM extraction. A valid prefix can be checked independently of
//! an invalid later timestamp, without correcting or approving that later data.
use super::*;
use std::collections::BTreeSet;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    id: String,
    start_character: usize,
    end_character: usize,
    expected_text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    schema: String,
    wave: Object,
    alignment: Object,
    approval: Object,
    segments: Vec<Segment>,
}
struct Cut {
    id: String,
    wave: Vec<u8>,
    alignment: Vec<u8>,
    report: Value,
}
fn pcm(bytes: &[u8]) -> Result<&[u8]> {
    if bytes.len() < 46
        || &bytes[..4] != b"RIFF"
        || &bytes[8..16] != b"WAVEfmt "
        || &bytes[36..40] != b"data"
        || u32::from_le_bytes(bytes[16..20].try_into()?) != 16
        || u16::from_le_bytes(bytes[20..22].try_into()?) != 1
        || u16::from_le_bytes(bytes[22..24].try_into()?) != 1
        || u32::from_le_bytes(bytes[24..28].try_into()?) != 44_100
        || u32::from_le_bytes(bytes[28..32].try_into()?) != 88_200
        || u16::from_le_bytes(bytes[32..34].try_into()?) != 2
        || u16::from_le_bytes(bytes[34..36].try_into()?) != 16
        || u32::from_le_bytes(bytes[4..8].try_into()?) as usize != bytes.len() - 8
        || u32::from_le_bytes(bytes[40..44].try_into()?) as usize != bytes.len() - 44
        || (bytes.len() - 44) % 2 != 0
    {
        bail!("canonical mono PCM16 44100 WAV required");
    }
    Ok(&bytes[44..])
}
fn extract(
    request: &Request,
    wav: &[u8],
    evidence: &Value,
    segments: &[Segment],
) -> Result<Vec<Cut>> {
    let raw = pcm(wav)?;
    let samples = raw.len() / 2;
    if segments.is_empty() || segments.len() > 64 {
        bail!("1..64 bounded segments required")
    }
    let a: Alignment = serde_json::from_value(evidence["alignment"].clone())?;
    let count = a.characters.len();
    if count > 32768
        || count != a.character_start_times_seconds.len()
        || count != a.character_end_times_seconds.len()
        || a.characters.iter().any(|c| c.chars().count() != 1)
        || a.characters.concat() != request.source_text
    {
        bail!("exact source character evidence required")
    }
    let mut ids = BTreeSet::new();
    let mut cuts = vec![];
    let mut output_bytes = 0;
    for s in segments {
        if !token(&s.id)
            || !ids.insert(&s.id)
            || s.start_character >= s.end_character
            || s.end_character > count
            || s.expected_text.trim().is_empty()
            || a.characters[s.start_character..s.end_character].concat() != s.expected_text
        {
            bail!("invalid segment identity or source range")
        }
        let mut last_start = 0.0;
        let mut last_end = 0.0;
        for i in s.start_character..s.end_character {
            let start = a.character_start_times_seconds[i];
            let end = a.character_end_times_seconds[i];
            if !start.is_finite()
                || !end.is_finite()
                || start < last_start
                || end < last_end
                || end < start
                || start < 0.0
                || end > samples as f64 / RATE as f64
            {
                bail!("selected character interval is not bounded by source PCM")
            }
            last_start = start;
            last_end = end;
        }
        let first =
            (a.character_start_times_seconds[s.start_character] * RATE as f64).floor() as usize;
        let end = (last_end * RATE as f64).ceil() as usize;
        if first >= end || end > samples {
            bail!("invalid selected sample span")
        }
        let wave = wave(&raw[first * 2..end * 2])?;
        let relative = json!({"alignment":{"characters":a.characters[s.start_character..s.end_character],
            "character_start_times_seconds":a.character_start_times_seconds[s.start_character..s.end_character].iter().map(|v|((v*RATE as f64).floor() as usize-first) as f64/RATE as f64).collect::<Vec<_>>(),
            "character_end_times_seconds":a.character_end_times_seconds[s.start_character..s.end_character].iter().map(|v|((v*RATE as f64).ceil() as usize-first) as f64/RATE as f64).collect::<Vec<_>>()},"normalized_alignment":null});
        let mut sub = request.clone();
        sub.source_text = s.expected_text.clone();
        sub.source_text_sha256 = Object::from_bytes(sub.source_text.as_bytes())
            .sha256
            .as_str()
            .into();
        sub.audio_tag = None;
        let qualified = timing(&sub, &relative, (end - first) as u64);
        if qualified["qualified"] != true {
            bail!("selected source-word timing is unqualified")
        }
        let alignment = serde_json::to_vec(
            &json!({"alignment":relative["alignment"],"source_word_timing":qualified}),
        )?;
        output_bytes += wave.len() + alignment.len();
        if output_bytes > 64 * 1024 * 1024 {
            bail!("cut packet budget")
        }
        let report = json!({"id":s.id,"source_character_start":s.start_character,"source_character_end":s.end_character,
            "source_sample_start":first,"source_sample_end":end,"samples":end-first,"text_sha256":sub.source_text_sha256,
            "wave":Object::from_bytes(&wave),"alignment":Object::from_bytes(&alignment),"timing_qualified":true,"pcm_samples_unchanged":true,
            "derived_alignment_quantization":"native sample grid: start floor, end ceil; at most one sample per endpoint"});
        cuts.push(Cut {
            id: s.id.clone(),
            wave,
            alignment,
            report,
        });
    }
    Ok(cuts)
}
fn approved(value: &Value, request: &Request, wave: &Object) -> bool {
    value["approved"] == true
        && value["public_release"] == false
        && value["subjects"].as_array().is_some_and(|subjects| {
            subjects.iter().any(|s| {
                s["role_id"] == request.role_id
                    && s["language"] == request.language_code
                    && s["model_id"] == request.model_id
                    && s["source_text_sha256"] == request.source_text_sha256
                    && serde_json::from_value::<Object>(s["audio"].clone())
                        .is_ok_and(|object| object == *wave)
            })
        })
}
pub fn cut(profile: &Path, spec: &Path, output: &Path) -> Result<Value> {
    let (prepared, mut cache) = prepare(profile)?;
    let mut raw = vec![];
    fs::File::open(spec)?
        .take(CONTROL_MAX + 1)
        .read_to_end(&mut raw)?;
    if raw.len() as u64 > CONTROL_MAX {
        bail!("cut specification exceeds budget")
    }
    let selected: Spec = serde_json::from_slice(&raw)?;
    if selected.schema != "reel.speech-cut.v1" {
        bail!("unsupported speech cut schema")
    }
    let wave = read_object(&mut cache, &selected.wave, RESPONSE_MAX)?;
    let alignment: Value = serde_json::from_slice(&read_object(
        &mut cache,
        &selected.alignment,
        2 * 1024 * 1024,
    )?)?;
    let authority: Value =
        serde_json::from_slice(&read_object(&mut cache, &selected.approval, CONTROL_MAX)?)?;
    if !approved(&authority, &prepared.request, &selected.wave) {
        bail!("source audition approval is absent or mismatched")
    }
    let source_qualified = timing(
        &prepared.request,
        &alignment,
        (pcm(&wave)?.len() / 2) as u64,
    )["qualified"]
        .clone();
    let cuts = extract(&prepared.request, &wave, &alignment, &selected.segments)?;
    if !output.is_absolute() || !output.parent().is_some_and(Path::is_dir) {
        bail!("new absolute cut directory required")
    }
    fs::create_dir(output)?;
    write_new(&output.join("spec.json"), &raw)?;
    for cut in &cuts {
        write_new(&output.join(format!("{}.wav", cut.id)), &cut.wave)?;
        write_new(
            &output.join(format!("{}.alignment.json", cut.id)),
            &cut.alignment,
        )?;
    }
    let receipt = json!({"schema":"reel.speech-cut-receipt.v1","source_request":prepared.profile.request,"source_wave":selected.wave,
        "source_alignment":selected.alignment,"source_approval":selected.approval,"source_global_timing_qualified":source_qualified,
        "spec":Object::from_bytes(&raw),"clips":cuts.iter().map(|c|c.report.clone()).collect::<Vec<_>>(),
        "provider_calls":0,"raw_source_evidence_changed":false,"human_edit_approval":false,"public_release":false});
    write_new(&output.join("receipt.json"), &serde_json::to_vec(&receipt)?)?;
    Ok(
        json!({"schema":"reel.speech-cut-result.v1","clips":cuts.len(),"provider_calls":0,"selected_timing_qualified":true,"source_global_timing_qualified":source_qualified}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_prefix_survives_invalid_later_endpoint_without_repair() {
        let mut p = super::super::tests::fixture();
        p.request.source_text = "Hi. Bye.".into();
        let raw: Vec<u8> = (0..44_100).flat_map(|n| (n as i16).to_le_bytes()).collect();
        let wav = wave(&raw).unwrap();
        let evidence = json!({"alignment":{"characters":"Hi. Bye.".chars().map(|c|c.to_string()).collect::<Vec<_>>(),"character_start_times_seconds":[0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8],"character_end_times_seconds":[0.2,0.3,0.4,0.5,0.6,0.7,0.8,1.08]}});
        let good = Segment {
            id: "first".into(),
            start_character: 0,
            end_character: 3,
            expected_text: "Hi.".into(),
        };
        let cut = extract(&p.request, &wav, &evidence, &[good]).unwrap();
        assert_eq!(pcm(&cut[0].wave).unwrap(), &raw[4410 * 2..17640 * 2]);
        let bad = Segment {
            id: "last".into(),
            start_character: 4,
            end_character: 8,
            expected_text: "Bye.".into(),
        };
        assert!(extract(&p.request, &wav, &evidence, &[bad]).is_err());
    }
    #[test]
    fn source_ranges_and_canonical_transport_refuse() {
        let p = super::super::tests::fixture();
        let wav = wave(&[0; 8820]).unwrap();
        let evidence: Value =
            serde_json::from_slice(&super::super::tests::response(&p.request.source_text)).unwrap();
        let bad = Segment {
            id: "../outside".into(),
            start_character: 0,
            end_character: 1,
            expected_text: "wrong".into(),
        };
        assert!(extract(&p.request, &wav, &evidence, &[bad]).is_err());
        assert!(pcm(b"not-a-wave").is_err());
    }
}

//! Read-only replay of retained provider bytes; never opens the installed model.
use super::*;

fn held_bytes(root: &Path, name: &str, expected: &[u8], max: u64) -> Result<source::Held> {
    source::held(
        source::Input {
            path: root.join(name),
            object: pin(expected),
        },
        max,
    )
}

/// The caller must independently admit the original inputs, recipe and exact
/// execution control. Matching retained receipts prove correspondence, not an
/// actual historical provider execution, runtime custody or musical quality.
pub fn check_generation(
    admission: &Path,
    recipe: &Path,
    execution: &Path,
    output: &Path,
) -> Result<Value> {
    let loaded = load(admission, recipe)?;
    let root = output.canonicalize()?;
    let execution_raw = read_metadata(execution)?;
    let e: Execution = serde_json::from_value(packet::parse(&execution_raw)?)?;
    validate_execution(&loaded, &e)?;
    let mut held = vec![
        source::held(
            source::Input {
                path: execution.canonicalize()?,
                object: pin(&execution_raw),
            },
            META,
        )?,
        source::held(
            source::Input {
                path: recipe.canonicalize()?,
                object: loaded.recipe_pin.clone(),
            },
            META,
        )?,
    ];
    let admission_raw = read_metadata(admission)?;
    if loaded.original.context()["interpretation_admission_sha256"] != sha256_bytes(&admission_raw)
    {
        bail!("original admission changed during retained replay");
    }
    held.push(source::held(
        source::Input {
            path: admission.canonicalize()?,
            object: pin(&admission_raw),
        },
        META,
    )?);
    let context_raw = serde_json::to_vec_pretty(loaded.original.context())?;
    held.push(held_bytes(
        &root,
        "source-context.json",
        &context_raw,
        META,
    )?);
    held.push(held_bytes(
        &root,
        "source.wav",
        loaded.original.recording_bytes(),
        AUDIO,
    )?);
    let runtime = source::held(
        source::Input {
            path: root.join("runtime.json"),
            object: e.runtime.object.clone(),
        },
        4 * 1024 * 1024,
    )?;
    let runtime_value = packet::parse(&runtime.bytes)?;
    let ace_root = PathBuf::from(
        runtime_value["ace_root"]
            .as_str()
            .context("retained runtime source root missing")?,
    );
    let ace_commit = runtime_value["ace_commit"]
        .as_str()
        .context("retained runtime source version missing")?;
    if runtime_value["schema"] != "reel.local-song-runtime.v1"
        || !ace_root.is_absolute()
        || !matches!(ace_commit.len(), 40 | 64)
        || !ace_commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("retained runtime source identity differs");
    }
    for object in [&e.backend.object, &e.backend_helper.object] {
        if object.bytes == 0
            || object.bytes > 1024 * 1024
            || object.sha256.len() != 64
            || !object
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("retained provider backend pin differs");
        }
    }
    // Reconstruct the recorded read-set accounting from its retained descriptor.
    // Never probe these historical installed paths during a retained check.
    let files = runtime_value["generation_files"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 4096)
        .context("retained provider read set missing or oversized")?;
    let mut entries = vec![serde_json::from_value::<Installed>(
        runtime_value["python"].clone(),
    )?];
    for v in files {
        entries.push(serde_json::from_value::<Installed>(v.clone())?);
    }
    let mut seen = BTreeMap::new();
    let mut total = 0u64;
    for entry in entries {
        if !entry.path.is_absolute()
            || entry.bytes == 0
            || entry.bytes > 4 * 1024 * 1024 * 1024
            || entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("retained provider installed descriptor differs");
        }
        let key = entry.path.to_string_lossy().replace('\\', "/");
        let key = if cfg!(windows) {
            key.to_lowercase()
        } else {
            key
        };
        let object = source::Pin {
            sha256: entry.sha256,
            bytes: entry.bytes,
        };
        if let Some(old) = seen.insert(key, object.clone()) {
            if old != object {
                bail!("retained provider read-set pins conflict");
            }
            continue;
        }
        total = total
            .checked_add(object.bytes)
            .context("retained provider read-set overflow")?;
        if total > 16 * 1024 * 1024 * 1024 {
            bail!("retained provider read set exceeds bound");
        }
    }
    let mut geometry = loaded.geometry.clone();
    let channels = usize::from(loaded.audio.channels);
    for (index, g) in geometry.iter_mut().enumerate() {
        let start = g["context_start_frame"].as_u64().unwrap() as usize;
        let end = g["context_end_frame"].as_u64().unwrap() as usize;
        let bytes = wave::encode(
            &loaded.audio.samples[start * channels..end * channels],
            loaded.audio.rate,
            loaded.audio.channels,
        )?;
        g["context_audio"] = serde_json::to_value(pin(&bytes))?;
        held.push(held_bytes(
            &root,
            &format!("patch-source-{index}.wav"),
            &bytes,
            AUDIO,
        )?);
    }
    let request = json!({"schema":"reel.original-song-repair-provider-request.v1",
        "recipe":loaded.recipe_pin,"source":loaded.recipe.recording,"source_context":pin(&context_raw),
        "runtime":e.runtime.object,"backend":e.backend.object,"backend_helper":e.backend_helper.object,
        "sample_rate":loaded.audio.rate,"channels":loaded.audio.channels,"source_frames":loaded.recipe.source_frames,
        "patches":geometry,"caption":e.caption,"seed":e.seed,"repaint_mode":e.repaint_mode,
        "repaint_strength":e.repaint_strength,"permissions":e.permissions,"attempts_per_patch":1,"semantic_planner":false});
    let request_raw = serde_json::to_vec_pretty(&request)?;
    held.push(held_bytes(
        &root,
        "provider-request.json",
        &request_raw,
        META,
    )?);
    let receipt_raw = read_metadata(&root.join("provider-receipt.json"))?;
    held.push(held_bytes(
        &root,
        "provider-receipt.json",
        &receipt_raw,
        META,
    )?);
    let receipt = packet::parse(&receipt_raw)?;
    if receipt["schema"] != "reel.original-song-repair-provider-receipt.v1"
        || receipt["request"] != serde_json::to_value(pin(&request_raw))?
        || receipt["runtime"] != request["runtime"]
        || receipt["source_context"] != request["source_context"]
        || receipt["external_upload"] != false
        || receipt["human_approval"] != false
        || receipt["selected"] != false
    {
        bail!("retained provider receipt correspondence or restrictions differ");
    }
    let attempts = receipt["patches"]
        .as_array()
        .filter(|a| a.len() == geometry.len())
        .context("retained provider attempt roster differs")?;
    let mut candidates = Vec::new();
    let mut normalization = Vec::new();
    for (index, (attempt, g)) in attempts.iter().zip(&geometry).enumerate() {
        if attempt["index"] != index
            || attempt["attempt"] != 1
            || attempt["seed"] != u64::from(e.seed) + index as u64
            || attempt["context_start_frame"] != g["context_start_frame"]
            || attempt["context_end_frame"] != g["context_end_frame"]
            || attempt["context_audio"] != g["context_audio"]
        {
            bail!("retained provider attempt, seed or conditioning differs");
        }
        let raw = source::held(
            source::Input {
                path: root.join(format!("raw-patch-{index}.wav")),
                object: serde_json::from_value(attempt["raw_audio"].clone())?,
            },
            AUDIO,
        )?;
        let audio = wave::decode(&raw.bytes)?;
        let start = g["context_start_frame"].as_u64().unwrap();
        let end = g["context_end_frame"].as_u64().unwrap();
        let frames = (audio.samples.len() / usize::from(audio.channels)) as u64;
        if audio.rate != loaded.audio.rate
            || audio.channels != loaded.audio.channels
            || audio.float_data.is_none()
            || frames.abs_diff(end - start) > u64::from(audio.rate) / 10
            || frames < g["end_frame"].as_u64().unwrap() - start
            || audio.samples.iter().any(|s| s.abs() > 1.001)
        {
            bail!("retained raw provider clock, extent or signal differs");
        }
        let mut normalized =
            loaded.audio.samples[start as usize * channels..end as usize * channels].to_vec();
        let count = normalized.len().min(audio.samples.len());
        normalized[..count].copy_from_slice(&audio.samples[..count]);
        let bytes = wave::encode(&normalized, audio.rate, audio.channels)?;
        let name = format!("candidate-patch-{index}.wav");
        held.push(held_bytes(&root, &name, &bytes, AUDIO)?);
        candidates.push(json!({"path":name,"object":pin(&bytes)}));
        normalization.push(json!({"raw_audio":raw.pin,"raw_frames":frames,"native_context_frames":end-start,
            "appended_original_tail_frames":(end-start).saturating_sub(frames),"truncated_tail_frames":frames.saturating_sub(end-start),"patch_fully_from_raw_provider":true}));
        held.push(raw);
    }
    let roster = serde_json::to_vec_pretty(
        &json!({"schema":"reel.original-song-repair-candidates.v1","recipe":loaded.recipe_pin,"patches":candidates}),
    )?;
    held.push(held_bytes(&root, "candidates.json", &roster, META)?);
    let materialization = check(
        admission,
        recipe,
        &root.join("candidates.json"),
        &root.join("materialized"),
    )?;
    let generation_raw = read_metadata(&root.join("generation-receipt.json"))?;
    held.push(held_bytes(
        &root,
        "generation-receipt.json",
        &generation_raw,
        META,
    )?);
    let generation = packet::parse(&generation_raw)?;
    let recorded_execution = generation["provider_process_executed"]
        .as_bool()
        .context("recorded provider execution flag missing")?;
    let expected = json!({"schema":"reel.original-song-repair-generation-receipt.v1","execution":pin(&execution_raw),
        "recipe":loaded.recipe_pin,"provider_request":pin(&request_raw),"provider_receipt":pin(&receipt_raw),
        "runtime":runtime.pin,"backend":e.backend.object,"backend_helper":e.backend_helper.object,
        "installed_guarded_files":seen.len(),"installed_bytes":total,"provider_process_executed":recorded_execution,
        "declared_attempts_per_patch":1,"raw_candidate_normalization":normalization,"materialization":materialization,
        "new_original_request":false,"performed_words_verified":false,"mapping_authority_verified":false,
        "acoustic_continuity":"unreviewed","beat_alignment":"unreviewed","human_listening":"pending",
        "human_approval":false,"selected":false,"shareable":false});
    if generation != expected {
        bail!("retained generation receipt or normalization differs");
    }
    held.push(runtime);
    Ok(
        json!({"schema":"reel.original-song-repair-generation-check.v1",
        "generation_receipt":pin(&generation_raw),"raw_to_normalized_sample_bits_exact":true,
        "outside_mask_sample_bits_exact":true,"patch_count":geometry.len(),
        "recorded_provider_process_executed":recorded_execution,"provider_executed_now":false,
        "historical_process_execution_independently_verified":false,"installed_runtime_reopened":false,
        "performed_words_verified":false,"human_approval":false,"selected":false,"shareable":false}),
    )
}

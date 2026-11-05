//! Original evidence -> one bounded raw-provider pass -> independent materialization.
//! The caller supplies a qualified installation and an outer managed-job admission.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Component, PathBuf},
};
mod replay;
pub use replay::check_generation;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    schema: String,
    runtime: source::Input,
    backend: source::Input,
    backend_helper: source::Input,
    caption: String,
    seed: u32,
    repaint_mode: String,
    repaint_strength: f64,
    permissions: Value,
    timeout_seconds: u64,
    memory_bytes: usize,
    processes: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Installed {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}
struct InstalledGuard {
    _file: File,
}
struct Prepared {
    loaded: Loaded,
    execution: Execution,
    execution_pin: source::Pin,
    runtime: source::Held,
    backend: source::Held,
    helper: source::Held,
    backend_path: PathBuf,
    python: Installed,
    _control_guards: Vec<source::Held>,
    _installed: Vec<InstalledGuard>,
    installed_bytes: u64,
}
fn named(root: &Path, mut input: source::Input) -> Result<source::Input> {
    let relative = !input.path.is_absolute();
    if relative {
        if !input
            .path
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        {
            bail!("provider input needs an explicit ordinary relative or absolute path");
        }
        input.path = root.join(input.path);
    }
    let parent = input
        .path
        .parent()
        .context("provider input parent missing")?
        .canonicalize()?;
    if relative && !parent.starts_with(root) {
        bail!("provider input escapes execution view");
    }
    input.path = parent.join(
        input
            .path
            .file_name()
            .context("provider input leaf missing")?,
    );
    Ok(input)
}
fn hold_installed(input: &Installed) -> Result<InstalledGuard> {
    if !input.path.is_absolute()
        || input.bytes == 0
        || input.bytes > 4 * 1024 * 1024 * 1024
        || input.sha256.len() != 64
        || !input
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("provider installed-file pin or bound differs");
    }
    let metadata = fs::symlink_metadata(&input.path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if !metadata.is_file() || reparse || metadata.len() != input.bytes {
        bail!("provider installed file shape differs");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(&input.path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        };
        bytes += n as u64;
        if bytes > input.bytes {
            bail!("provider installed file grew")
        };
        hash.update(&buffer[..n]);
    }
    let actual = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if bytes != input.bytes || actual != input.sha256 {
        bail!("provider installed bytes differ");
    }
    Ok(InstalledGuard { _file: file })
}
fn validate_execution(loaded: &Loaded, e: &Execution) -> Result<()> {
    if e.schema != "reel.original-song-repair-execution.v1"
        || e.caption.trim().is_empty()
        || e.caption.len() > 4096
        || e.caption
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        || e.seed > u32::MAX - 3
        || !matches!(
            e.repaint_mode.as_str(),
            "conservative" | "balanced" | "aggressive"
        )
        || !e.repaint_strength.is_finite()
        || !(0.0..=1.0).contains(&e.repaint_strength)
        || e.permissions != loaded.original.context()["private_scope"]
        || !(1..=3600).contains(&e.timeout_seconds)
        || !(64 * 1024 * 1024..=16u64 * 1024 * 1024 * 1024).contains(&(e.memory_bytes as u64))
        || !(1..=32).contains(&e.processes)
    {
        bail!("provider execution parameters or original private restrictions differ");
    }
    Ok(())
}
fn prepare(admission: &Path, recipe: &Path, execution: &Path) -> Result<Prepared> {
    let loaded = load(admission, recipe)?;
    let execution_path = execution.canonicalize()?;
    let root = execution_path.parent().context("execution view missing")?;
    let execution_raw = read_metadata(&execution_path)?;
    let execution_pin = pin(&execution_raw);
    let e: Execution = serde_json::from_value(packet::parse(&execution_raw)?)?;
    validate_execution(&loaded, &e)?;
    let control = source::held(
        source::Input {
            path: execution_path.clone(),
            object: execution_pin.clone(),
        },
        META,
    )?;
    let recipe_guard = source::held(
        source::Input {
            path: recipe.canonicalize()?,
            object: loaded.recipe_pin.clone(),
        },
        META,
    )?;
    let admission_raw = read_metadata(admission)?;
    if loaded.original.context()["interpretation_admission_sha256"] != sha256_bytes(&admission_raw)
    {
        bail!("original admission changed");
    }
    let admission_guard = source::held(
        source::Input {
            path: admission.canonicalize()?,
            object: pin(&admission_raw),
        },
        META,
    )?;
    // Move only the three file inputs out; no policy defaults are inferred.
    let runtime_input = named(
        root,
        source::Input {
            path: e.runtime.path.clone(),
            object: e.runtime.object.clone(),
        },
    )?;
    let backend_input = named(
        root,
        source::Input {
            path: e.backend.path.clone(),
            object: e.backend.object.clone(),
        },
    )?;
    let helper_input = named(
        root,
        source::Input {
            path: e.backend_helper.path.clone(),
            object: e.backend_helper.object.clone(),
        },
    )?;
    if helper_input.path.file_name() != Some(std::ffi::OsStr::new("local_song_backend.py"))
        || helper_input.path.parent() != backend_input.path.parent()
    {
        bail!("provider helper must be the pinned adjacent shared module");
    }
    let backend_path = backend_input.path.clone();
    let runtime = source::held(runtime_input, 4 * 1024 * 1024)?;
    let backend = source::held(backend_input, 1024 * 1024)?;
    let helper = source::held(helper_input, 1024 * 1024)?;
    let value = packet::parse(&runtime.bytes)?;
    let ace_root = PathBuf::from(
        value["ace_root"]
            .as_str()
            .context("provider source root missing")?,
    );
    let commit = value["ace_commit"]
        .as_str()
        .context("provider source version missing")?;
    if value["schema"] != "reel.local-song-runtime.v1"
        || !ace_root.is_absolute()
        || !ace_root.is_dir()
        || !matches!(commit.len(), 40 | 64)
        || !commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("provider runtime source identity differs");
    }
    let python: Installed = serde_json::from_value(value["python"].clone())?;
    let files = value["generation_files"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 4096)
        .context("provider installed read set missing or oversized")?;
    let mut descriptors = vec![Installed {
        path: python.path.clone(),
        sha256: python.sha256.clone(),
        bytes: python.bytes,
    }];
    for file in files {
        descriptors.push(serde_json::from_value::<Installed>(file.clone())?);
    }
    let mut seen = BTreeMap::new();
    let mut guards = Vec::new();
    let mut total = 0u64;
    for file in descriptors {
        let key = file.path.to_string_lossy().replace('\\', "/");
        let key = if cfg!(windows) {
            key.to_lowercase()
        } else {
            key
        };
        let file_pin = source::Pin {
            sha256: file.sha256.clone(),
            bytes: file.bytes,
        };
        if let Some(old) = seen.insert(key, file_pin.clone()) {
            if old != file_pin {
                bail!("provider read-set pins conflict")
            };
            continue;
        }
        total = total
            .checked_add(file.bytes)
            .context("provider read-set size overflow")?;
        if total > 16 * 1024 * 1024 * 1024 {
            bail!("provider read set exceeds bound");
        }
        guards.push(hold_installed(&file)?);
    }
    Ok(Prepared {
        loaded,
        execution: e,
        execution_pin,
        runtime,
        backend,
        helper,
        backend_path,
        python,
        _control_guards: vec![control, recipe_guard, admission_guard],
        _installed: guards,
        installed_bytes: total,
    })
}
pub fn plan(admission: &Path, recipe: &Path, execution: &Path) -> Result<Value> {
    let p = prepare(admission, recipe, execution)?;
    Ok(
        json!({"schema":"reel.original-song-repair-provider-plan.v1","recipe":p.loaded.recipe_pin,
        "patch_count":p.loaded.recipe.patches.len(),"installed_guarded_files":p._installed.len(),
        "installed_bytes":p.installed_bytes,"provider_executed":false,"human_approval":false,"shareable":false}),
    )
}
struct Invocation<'a> {
    prepared: &'a Prepared,
    output: &'a Path,
}
fn run_provider(invocation: &Invocation<'_>) -> Result<bool> {
    #[cfg(windows)]
    {
        let p = invocation.prepared;
        reel_assets::jobs::run_quiet(
            &p.python.path,
            &[
                "-B".into(),
                p.backend_path.to_string_lossy().into_owned(),
                "--original-repair".into(),
            ],
            invocation.output,
            std::time::Duration::from_secs(p.execution.timeout_seconds),
            reel_assets::jobs::QuietLimits {
                memory_bytes: p.execution.memory_bytes,
                processes: p.execution.processes,
            },
            || Ok(()),
        )
        .map_err(|e| anyhow::anyhow!("bounded original-song provider failed: {e}"))?;
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        let _ = (
            &invocation.prepared.backend_path,
            &invocation.prepared.python.path,
            invocation.output,
        );
        bail!("original-song provider currently requires qualified Windows Job Objects")
    }
}
fn generate_with(
    admission: &Path,
    recipe: &Path,
    execution: &Path,
    output: &Path,
    runner: impl FnOnce(&Invocation<'_>) -> Result<bool>,
) -> Result<Value> {
    if !output.is_absolute() || output.exists() {
        bail!("provider output must be new and absolute")
    };
    let p = prepare(admission, recipe, execution)?;
    fs::create_dir(output)?;
    let context_raw = serde_json::to_vec_pretty(p.loaded.original.context())?;
    write_new(&output.join("source-context.json"), &context_raw)?;
    write_new(
        &output.join("source.wav"),
        p.loaded.original.recording_bytes(),
    )?;
    write_new(&output.join("runtime.json"), &p.runtime.bytes)?;
    let mut input_guards = Vec::new();
    let mut geometry = p.loaded.geometry.clone();
    for (index, patch) in geometry.iter_mut().enumerate() {
        let start = patch["context_start_frame"].as_u64().unwrap() as usize;
        let end = patch["context_end_frame"].as_u64().unwrap() as usize;
        let channels = usize::from(p.loaded.audio.channels);
        let raw = wave::encode(
            &p.loaded.audio.samples[start * channels..end * channels],
            p.loaded.audio.rate,
            p.loaded.audio.channels,
        )?;
        let path = output.join(format!("patch-source-{index}.wav"));
        write_new(&path, &raw)?;
        patch["context_audio"] = serde_json::to_value(pin(&raw))?;
        input_guards.push(source::held(
            source::Input {
                path,
                object: pin(&raw),
            },
            AUDIO,
        )?);
    }
    let request = json!({"schema":"reel.original-song-repair-provider-request.v1",
        "recipe":p.loaded.recipe_pin,"source":p.loaded.recipe.recording,"source_context":pin(&context_raw),
        "runtime":p.runtime.pin,"backend":p.backend.pin,"backend_helper":p.helper.pin,
        "sample_rate":p.loaded.audio.rate,"channels":p.loaded.audio.channels,"source_frames":p.loaded.recipe.source_frames,
        "patches":geometry,"caption":p.execution.caption,"seed":p.execution.seed,
        "repaint_mode":p.execution.repaint_mode,"repaint_strength":p.execution.repaint_strength,
        "permissions":p.execution.permissions,"attempts_per_patch":1,"semantic_planner":false});
    let request_raw = serde_json::to_vec_pretty(&request)?;
    write_new(&output.join("provider-request.json"), &request_raw)?;
    for (name, object) in [
        ("source-context.json", pin(&context_raw)),
        ("source.wav", p.loaded.recipe.recording.clone()),
        ("runtime.json", p.runtime.pin.clone()),
        ("provider-request.json", pin(&request_raw)),
    ] {
        input_guards.push(source::held(
            source::Input {
                path: output.join(name),
                object,
            },
            AUDIO,
        )?);
    }
    let executed = runner(&Invocation {
        prepared: &p,
        output,
    })?;
    let receipt_raw = read_metadata(&output.join("provider-receipt.json"))?;
    let receipt = packet::parse(&receipt_raw)?;
    if receipt["schema"] != "reel.original-song-repair-provider-receipt.v1"
        || receipt["request"] != serde_json::to_value(pin(&request_raw))?
        || receipt["runtime"] != request["runtime"]
        || receipt["source_context"] != request["source_context"]
        || receipt["external_upload"] != false
        || receipt["human_approval"] != false
        || receipt["selected"] != false
    {
        bail!("provider receipt correspondence or restrictions differ")
    };
    let attempts = receipt["patches"]
        .as_array()
        .filter(|a| a.len() == p.loaded.geometry.len())
        .context("provider attempt roster differs")?;
    let mut raw_guards = Vec::new();
    let mut candidates = Vec::new();
    let mut normalization = Vec::new();
    for (index, (attempt, geometry)) in attempts.iter().zip(&p.loaded.geometry).enumerate() {
        if attempt["index"] != index
            || attempt["attempt"] != 1
            || attempt["seed"] != u64::from(p.execution.seed) + index as u64
            || attempt["context_start_frame"] != geometry["context_start_frame"]
            || attempt["context_end_frame"] != geometry["context_end_frame"]
            || attempt["context_audio"] != request["patches"][index]["context_audio"]
        {
            bail!("provider attempt count, seed or source range differs")
        };
        let raw_pin: source::Pin = serde_json::from_value(attempt["raw_audio"].clone())?;
        let raw = source::held(
            source::Input {
                path: output.join(format!("raw-patch-{index}.wav")),
                object: raw_pin.clone(),
            },
            AUDIO,
        )?;
        let audio = wave::decode(&raw.bytes)?;
        let start = geometry["context_start_frame"].as_u64().unwrap();
        let end = geometry["context_end_frame"].as_u64().unwrap();
        let frames = (audio.samples.len() / usize::from(audio.channels)) as u64;
        if audio.rate != p.loaded.audio.rate
            || audio.channels != p.loaded.audio.channels
            || audio.float_data.is_none()
            || frames.abs_diff(end - start) > u64::from(audio.rate) / 10
            || frames < geometry["end_frame"].as_u64().unwrap() - start
            || audio.samples.iter().any(|s| s.abs() > 1.001)
        {
            bail!("raw provider clock, extent or signal differs")
        };
        let channels = usize::from(audio.channels);
        let mut normalized =
            p.loaded.audio.samples[start as usize * channels..end as usize * channels].to_vec();
        let count = normalized.len().min(audio.samples.len());
        normalized[..count].copy_from_slice(&audio.samples[..count]);
        let bytes = wave::encode(&normalized, audio.rate, audio.channels)?;
        let path = output.join(format!("candidate-patch-{index}.wav"));
        write_new(&path, &bytes)?;
        candidates
            .push(json!({"path":format!("candidate-patch-{index}.wav"),"object":pin(&bytes)}));
        normalization.push(json!({"raw_audio":raw_pin,"raw_frames":frames,"native_context_frames":end-start,
            "appended_original_tail_frames":(end-start).saturating_sub(frames),"truncated_tail_frames":frames.saturating_sub(end-start),"patch_fully_from_raw_provider":true}));
        raw_guards.push(raw);
    }
    let candidates_path = output.join("candidates.json");
    write_new(
        &candidates_path,
        &serde_json::to_vec_pretty(
            &json!({"schema":"reel.original-song-repair-candidates.v1","recipe":p.loaded.recipe_pin,"patches":candidates}),
        )?,
    )?;
    let final_path = output.join("materialized");
    let checked = materialize(admission, recipe, &candidates_path, &final_path)?;
    let result = json!({"schema":"reel.original-song-repair-generation-receipt.v1","execution":p.execution_pin,
        "recipe":p.loaded.recipe_pin,"provider_request":pin(&request_raw),"provider_receipt":pin(&receipt_raw),
        "runtime":p.runtime.pin,"backend":p.backend.pin,"backend_helper":p.helper.pin,
        "installed_guarded_files":p._installed.len(),"installed_bytes":p.installed_bytes,"provider_process_executed":executed,
        "declared_attempts_per_patch":1,"raw_candidate_normalization":normalization,"materialization":checked,
        "new_original_request":false,"performed_words_verified":false,"mapping_authority_verified":false,
        "acoustic_continuity":"unreviewed","beat_alignment":"unreviewed","human_listening":"pending",
        "human_approval":false,"selected":false,"shareable":false});
    write_new(
        &output.join("generation-receipt.json"),
        &serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(
        json!({"schema":"reel.original-song-repair-generation-summary.v1","provider_process_executed":executed,
        "patch_count":p.loaded.geometry.len(),"outside_mask_sample_bits_exact":true,"performed_words_verified":false,
        "human_approval":false,"selected":false,"shareable":false}),
    )
}
pub fn generate(admission: &Path, recipe: &Path, execution: &Path, output: &Path) -> Result<Value> {
    generate_with(admission, recipe, execution, output, run_provider)
}

#[cfg(test)]
mod tests;

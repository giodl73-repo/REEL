use super::*;

const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROFILE_BYTES: usize = 128 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    profile: PathBuf,
    profile_pin: Object,
    dispatch: PathBuf,
    dispatch_pin: Object,
    replacements: BTreeMap<String, Replacement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Replacement {
    task_id: String,
    master_artifact: String,
}

pub fn bind(request: &Path, output: &Path) -> Result<Value> {
    let r: Request = read(request, MAX_DOCUMENT_BYTES, None)?;
    if r.schema != "reel.tig-episode-repair-binding.v1"
        || r.replacements.is_empty()
        || r.replacements.len() > 128
    {
        bail!("invalid episode repair binding");
    }
    let mut document: Value = read(&r.profile, MAX_PROFILE_BYTES, Some(&r.profile_pin))?;
    let profile: Profile = serde_json::from_value(document.clone())?;
    validate(&profile)?;
    if profile.inputs.purpose != Purpose::Assemble {
        bail!("episode repair requires an assembly profile");
    }
    let dispatch: Value = read(&r.dispatch, MAX_DOCUMENT_BYTES, Some(&r.dispatch_pin))?;
    if dispatch["ok"] != true {
        bail!("episode repair requires a successful dispatch report");
    }
    let scenes = document["inputs"]["scenes"]
        .as_array_mut()
        .context("episode repair profile scenes unavailable")?;
    let mut tasks = std::collections::BTreeSet::new();
    for (id, replacement) in &r.replacements {
        let mut matches = profile
            .inputs
            .scenes
            .iter()
            .enumerate()
            .filter(|(_, scene)| scene.id == *id);
        let (index, scene) = matches
            .next()
            .context("repair segment is absent from the episode")?;
        if matches.next().is_some() || !tasks.insert(&replacement.task_id) {
            bail!("ambiguous episode repair segment or task");
        }
        let edition = &dispatch["editions"][&replacement.task_id];
        let retained = &edition["retained_scene"];
        if edition["ok"] != true
            || !(edition["retained_verified"] == true
                || edition["retained_owner_acknowledged"] == true)
            || retained["task_id"] != replacement.task_id
            || retained["language"] != scene.request.language
            || retained["worker_receipt"].is_null()
            || retained["retained_acknowledgement"].is_null()
        {
            bail!("retained film result does not match episode segment language and task");
        }
        let artifact: reel_publication::Artifact =
            serde_json::from_value(retained["artifacts"][&replacement.master_artifact].clone())?;
        let task_key = retained["task_key"]
            .as_str()
            .context("retained task key missing")?;
        if artifact.media_type != "video/x-matroska"
            || artifact.logical_id
                != format!("workflow/film/{task_key}/{}", replacement.master_artifact)
        {
            bail!("episode repair requires a Matroska output of the retained film task");
        }
        let master = Object {
            sha256: Digest::parse(&artifact.sha256)?,
            bytes: artifact.bytes,
        };
        if master.bytes == 0
            || scene
                .expected_master
                .as_ref()
                .is_some_and(|pin| pin != &master)
        {
            bail!("episode repair would replace an existing master pin or bind an empty master");
        }
        scenes[index]["expected_master"] = json!(master);
    }
    preparation::report_parent(&profile, output)?;
    let bytes = identity::canonical_json(&document)?;
    if bytes.len() > MAX_PROFILE_BYTES {
        bail!("bound episode profile exceeds byte budget");
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(json!({"profile":Object::from_bytes(&bytes),"bound_segments":r.replacements.len()}))
}

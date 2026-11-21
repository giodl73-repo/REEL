use git_asset::{
    protocol::AgentError,
    upload::{Backend, Plan, Profile},
};
use git_asset_core::identity;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::Path,
};
pub(super) fn read(p: &Path) -> Result<Vec<u8>, AgentError> {
    if !p.is_absolute() {
        return Err(AgentError::InvalidProfile);
    }
    let m = fs::symlink_metadata(p)?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > 65536 {
        return Err(AgentError::InvalidProfile);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(AgentError::InvalidProfile);
        }
    }
    let mut b = Vec::new();
    fs::File::open(p)?.take(65537).read_to_end(&mut b)?;
    if b.len() > 65536 {
        return Err(AgentError::Limit);
    }
    Ok(b)
}
fn run(args: &[OsString]) -> Result<Value, &'static str> {
    let Some(action) = args.first().and_then(|a| a.to_str()) else {
        return Err("usage");
    };
    if action == "describe" && args.len() == 1 {
        return Ok(
            json!({"schema":"tig.lfs-upload-interface.design-1","profile_schema":"tig.lfs-upload.design-1","operations":["plan PROFILE OBJECT","peak PROFILE PLAN","status PROFILE PLAN","apply PROFILE PLAN","resume PROFILE PLAN"],"standalone_agent":"--upload-profile PROFILE","default_upload":false,"max_object_bytes":8*1024*1024,"retained_custody":false,"removal_authority":false,"initializes_stores":false}),
        );
    }
    if args.len() != 3 || !matches!(action, "plan" | "peak" | "status" | "apply" | "resume") {
        return Err("usage");
    }
    let profile = Profile::from_json(&read(Path::new(&args[1])).map_err(|_| "invalid_document")?)
        .map_err(|e| e.code())?;
    let mut backend = Backend::new(profile).map_err(|e| e.code())?;
    let bytes = read(Path::new(&args[2])).map_err(|_| "invalid_document")?;
    let plan = if action == "plan" {
        let object =
            serde_json::from_value(identity::parse_json(&bytes).map_err(|_| "invalid_document")?)
                .map_err(|_| "invalid_document")?;
        backend.plan(object).map_err(|e| e.code())?
    } else {
        Plan::from_json(&bytes).map_err(|e| e.code())?
    };
    if action == "plan" {
        return serde_json::to_value(plan).map_err(|_| "invalid_document");
    }
    if action == "peak" {
        return Ok(
            json!({"schema":"tig.lfs-upload-peak.design-1","plan":plan.id().map_err(|e|e.code())?,"local_witness_peak_bytes":backend.peak(&plan).map_err(|e|e.code())?,"remote_quota_reserved":false}),
        );
    }
    if action == "status" {
        return backend.status(&plan).map_err(|e| e.code());
    }
    let path = backend.source_path(&plan.object);
    backend.apply(&plan, &path).map_err(|e| e.code())
}
pub(super) fn cli(args: &[OsString]) -> Result<(), AgentError> {
    let (value, failed) = match run(args) {
        Ok(v) => (v, false),
        Err(code) => (
            json!({"schema":"tig.lfs-upload-response.design-1","ok":false,"code":code}),
            true,
        ),
    };
    let b = identity::canonical_json(&value).map_err(|_| AgentError::Limit)?;
    std::io::stdout().lock().write_all(&b)?;
    if failed {
        std::process::exit(1);
    }
    Ok(())
}

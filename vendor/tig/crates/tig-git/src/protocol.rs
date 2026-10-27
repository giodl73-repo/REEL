use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};
use tig_core::{Digest, Object, identity};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentError {
    InvalidProfile,
    Authority,
    Unavailable,
    Integrity,
    Capacity,
    Busy,
    Io,
    Limit,
    State,
    Malformed,
    Unsupported,
}
impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for AgentError {}
impl From<io::Error> for AgentError {
    fn from(_: io::Error) -> Self {
        Self::Io
    }
}
pub type Result<T> = std::result::Result<T, AgentError>;
/// A backend must supply a newly owned disposable name, never its canonical CAS path.
pub struct Download {
    path: PathBuf,
    relinquished: bool,
}
impl Download {
    pub fn from_owned_path(path: PathBuf) -> Self {
        Self {
            path,
            relinquished: false,
        }
    }
    fn relinquish(&mut self) -> Result<String> {
        let path = self
            .path
            .to_str()
            .ok_or(AgentError::InvalidProfile)?
            .to_owned();
        self.relinquished = true;
        Ok(path)
    }
}
impl Drop for Download {
    fn drop(&mut self) {
        if !self.relinquished {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
pub trait DownloadBackend {
    fn initialize(&mut self) -> Result<()>;
    fn download(&mut self, object: &Object) -> Result<Download>;
}
fn send(out: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *out, value).map_err(|_| AgentError::Io)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
pub fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    backend: &mut impl DownloadBackend,
) -> Result<()> {
    let mut initialized = false;
    let mut frames = 0;
    loop {
        let mut frame = Vec::new();
        let n = std::io::Read::take(&mut input, 65_538).read_until(b'\n', &mut frame)?;
        if n == 0 {
            return Ok(());
        }
        frames += 1;
        if frames > 4096 {
            return Err(AgentError::Limit);
        }
        if n > 65_537 || frame.last() != Some(&b'\n') {
            return Err(AgentError::Malformed);
        }
        frame.pop();
        let request = identity::parse_json(&frame).map_err(|_| AgentError::Malformed)?;
        match request["event"].as_str() {
            Some("terminate") => return Ok(()),
            Some("init") => {
                if initialized {
                    return Err(AgentError::State);
                }
                if request["operation"] != "download" {
                    send(
                        &mut output,
                        &json!({"error":{"code":1,"message":"This local profile supports downloads only"}}),
                    )?;
                    return Ok(());
                }
                if request
                    .get("concurrenttransfers")
                    .is_some_and(|n| n.as_u64().is_none_or(|n| n == 0 || n > 64))
                    || request.get("concurrent").is_some_and(|v| !v.is_boolean())
                {
                    return Err(AgentError::Malformed);
                }
                if let Err(e) = backend.initialize() {
                    send(
                        &mut output,
                        &json!({"error":{"code":2,"message":e.to_string()}}),
                    )?;
                    return Ok(());
                }
                initialized = true;
                send(&mut output, &json!({}))?;
            }
            Some("download") => {
                if !initialized {
                    return Err(AgentError::State);
                }
                let oid = request["oid"].as_str().and_then(|s| Digest::parse(s).ok());
                let result = (|| {
                    if !request["action"].is_null() {
                        return Err(AgentError::Unsupported);
                    }
                    let sha256 = oid.clone().ok_or(AgentError::Malformed)?;
                    let bytes = request["size"].as_u64().ok_or(AgentError::Malformed)?;
                    let object = Object { sha256, bytes };
                    let download = backend.download(&object)?;
                    Ok((object, download))
                })();
                match result {
                    Ok((object, mut download)) => {
                        send(
                            &mut output,
                            &json!({"event":"progress","oid":object.sha256.as_str(),"bytesSoFar":object.bytes,"bytesSinceLast":object.bytes}),
                        )?;
                        // Complete may be observed even if write/flush reports failure.
                        let path = download.relinquish()?;
                        send(
                            &mut output,
                            &json!({"event":"complete","oid":object.sha256.as_str(),"path":path}),
                        )?;
                    }
                    Err(e) => send(
                        &mut output,
                        &json!({"event":"complete","oid":oid.map(|d|d.as_str().to_owned()),"error":{"code":2,"message":e.to_string()}}),
                    )?,
                }
            }
            _ => return Err(AgentError::Unsupported),
        }
    }
}

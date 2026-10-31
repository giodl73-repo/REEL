//! Small private local transaction metadata, never payload/approval authority.
use super::*;
use reel_assets::jobs::{WorkspaceEntry, WorkspaceSnapshot};
use serde::{Deserialize, Serialize};
use std::io::Write;
const MAX: usize = 8 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    schema: String,
    scope: Value,
    root: WorkspaceEntry,
    snapshot: Option<WorkspaceSnapshot>,
    logical_bytes: u64,
    completed: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    body: Body,
    sha256: Digest,
}

pub(super) struct Transaction {
    path: PathBuf,
    scope: Value,
    body: Option<Body>,
    resumed: bool,
}
impl Transaction {
    pub fn read(coordination: &Path, scope: Value) -> Result<Self> {
        let id = Digest::blob(&identity::canonical_json(&scope)?);
        let path = coordination.join(format!("retirement-{}.json", id.as_str()));
        let body = match fs::symlink_metadata(&path) {
            Ok(_) => {
                if regular(&path)?.len() > MAX as u64 {
                    bail!("retirement transaction exceeds bound");
                }
                let mut raw = Vec::new();
                File::open(&path)?
                    .take(MAX as u64 + 1)
                    .read_to_end(&mut raw)?;
                if raw.len() > MAX {
                    bail!("retirement transaction exceeds bound");
                }
                let envelope: Envelope = serde_json::from_value(identity::parse_json(&raw)?)?;
                let body = envelope.body;
                if body.schema != "reel.tig-retirement-transaction.experimental-1"
                    || body.scope != scope
                    || envelope.sha256
                        != Digest::blob(&identity::canonical_json(&serde_json::to_value(&body)?)?)
                    || body.completed == body.snapshot.is_some()
                    || body
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| s.root != body.root || s.entries.len() > 10000)
                {
                    bail!("retirement transaction scope or integrity differs");
                }
                Some(body)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let resumed = body.as_ref().is_some_and(|b| !b.completed);
        Ok(Self {
            path,
            scope,
            body,
            resumed,
        })
    }
    pub fn prepared(&self) -> bool {
        self.body.as_ref().is_some_and(|b| !b.completed)
    }
    pub fn check(&self, observed: &WorkspaceSnapshot) -> Result<()> {
        if let Some(body) = &self.body {
            if body.completed
                || !observed.is_survivor_of(
                    body.snapshot
                        .as_ref()
                        .context("retirement snapshot missing")?,
                )
            {
                bail!("workspace identity or surviving entries differ from retirement transaction");
            }
        }
        Ok(())
    }
    fn write(&self, body: &Body, replace: bool) -> Result<()> {
        let sha256 = Digest::blob(&identity::canonical_json(&serde_json::to_value(body)?)?);
        let raw = identity::canonical_json(&serde_json::to_value(Envelope {
            body: body.clone(),
            sha256,
        })?)?;
        if raw.len() > MAX {
            bail!("retirement transaction exceeds bound");
        }
        let mut file = tempfile::NamedTempFile::new_in(
            self.path.parent().context("transaction parent missing")?,
        )?;
        file.write_all(&raw)?;
        file.flush()?;
        file.as_file().sync_all()?;
        if replace {
            file.persist(&self.path).map_err(|e| e.error)?;
        } else {
            file.persist_noclobber(&self.path).map_err(|e| e.error)?;
        }
        Ok(())
    }
    pub fn begin(&mut self, snapshot: WorkspaceSnapshot, logical_bytes: u64) -> Result<()> {
        if self.body.is_none() {
            let body = Body {
                schema: "reel.tig-retirement-transaction.experimental-1".into(),
                scope: self.scope.clone(),
                root: snapshot.root.clone(),
                snapshot: Some(snapshot),
                logical_bytes,
                completed: false,
            };
            self.write(&body, false)?;
            self.body = Some(body);
        }
        Ok(())
    }
    pub fn complete(&mut self) -> Result<()> {
        if let Some(body) = &self.body {
            if !body.completed {
                let mut completed = body.clone();
                completed.completed = true;
                completed.snapshot = None;
                self.write(&completed, true)?;
                self.body = Some(completed);
            }
        }
        Ok(())
    }
    pub fn report(&self) -> Value {
        json!({"recorded":self.body.is_some(),"resumed":self.resumed,
            "state":self.body.as_ref().map(|b| if b.completed {"completed"} else {"prepared"})})
    }
}

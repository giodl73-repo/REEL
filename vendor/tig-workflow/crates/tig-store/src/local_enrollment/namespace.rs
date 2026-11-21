use super::*;
pub(super) use crate::initialization::present;
use crate::initialization::{Fence, Namespace};
#[cfg(test)]
use fs2::FileExt;
use std::{fs::OpenOptions, io::Write};
pub(super) struct Context {
    pub request: Request,
    pub intent: Digest,
    pub stage: PathBuf,
    namespace: Namespace,
}
impl Context {
    pub fn prepare(request: &Request) -> Result<Self> {
        if request.schema != INPUT_SCHEMA
            || request.profile.max_object_bytes == 0
            || request.profile.max_object_bytes > local::MAX_CONFIGURED_OBJECT_BYTES
        {
            return Err(Error::Invalid);
        }
        local::check_root_spelling(&request.root).map_err(|_| Error::Invalid)?;
        let leaf = request
            .root
            .file_name()
            .and_then(|p| p.to_str())
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .ok_or(Error::Invalid)?;
        let parent = request.root.parent().ok_or(Error::Invalid)?;
        local::directory(parent).map_err(|_| Error::Invalid)?;
        let parent = fs::canonicalize(parent)?;
        let mut request = request.clone();
        request.root = parent.join(leaf);
        let capacity = match &request.capacity {
            Capacity::New {
                domain,
                epoch,
                floor,
            } => serde_json::json!({"kind":"new","domain":domain,"epoch":epoch,"floor":floor}),
            Capacity::Existing {
                root,
                ledger,
                minimum_checkpoint,
            } => {
                local::check_root_spelling(root).map_err(|_| Error::Invalid)?;
                let actual =
                    CapacityLedger::open(root, minimum_checkpoint).map_err(|_| Error::Conflict)?;
                if actual.identity() != ledger {
                    return Err(Error::Conflict);
                }
                serde_json::json!({"kind":"existing","root":root,"ledger":ledger})
            }
        };
        let value = serde_json::json!([
            "tig.local-enrollment-intent.experimental-1",
            request.root,
            request.operation,
            request.place,
            request.profile,
            capacity
        ]);
        let intent = Digest::blob(&identity::canonical_json(&value).map_err(|_| Error::Invalid)?);
        let namespace = Namespace::new(&request.root, intent.clone())?;
        let stage = namespace.stage().to_path_buf();
        Ok(Self {
            request,
            intent,
            stage,
            namespace,
        })
    }
    pub fn fence(&self) -> Result<Fence> {
        self.namespace.fence()
    }
    #[cfg(test)]
    fn fence_hook(&self, opened: impl FnOnce(bool)) -> Result<Fence> {
        self.namespace.fence_hook(opened)
    }
    pub fn prepare_stage(&self) -> Result<()> {
        if !present(&self.stage)? {
            fs::create_dir(&self.stage)?;
        }
        local::directory(&self.stage).map_err(|_| Error::Conflict)?;
        for (n, entry) in fs::read_dir(&self.stage)?.enumerate() {
            if n >= 4 {
                return Err(Error::Conflict);
            }
            let entry = entry?;
            let name = entry.file_name();
            match name.to_str() {
                Some("store") => local::directory(&entry.path()).map_err(|_| Error::Conflict)?,
                Some("capacity") if matches!(self.request.capacity, Capacity::New { .. }) => {
                    local::directory(&entry.path()).map_err(|_| Error::Conflict)?
                }
                Some("enrollment.json" | "enrollment.pending") => {
                    local::regular(&entry.path()).map_err(|_| Error::Conflict)?
                }
                _ => return Err(Error::Conflict),
            }
        }
        Ok(())
    }
    pub fn write_record(&self, record: &Record) -> Result<()> {
        let target = self.stage.join("enrollment.json");
        if present(&target)? {
            let old: Record =
                local::read_record(&target).map_err(|_| Error::PendingInitialization)?;
            if old.schema != record.schema
                || old.intent != record.intent
                || old.store != record.store
                || old.capacity.root != record.capacity.root
                || old.capacity.ledger != record.capacity.ledger
            {
                return Err(Error::Conflict);
            }
            let path = match &self.request.capacity {
                Capacity::New { .. } => self.stage.join("capacity"),
                Capacity::Existing { root, .. } => root.clone(),
            };
            let authority = CapacityLedger::open(&path, &old.capacity.minimum_checkpoint)
                .map_err(|_| Error::Conflict)?;
            if authority.identity() != &old.capacity.ledger {
                return Err(Error::Conflict);
            }
            return Ok(());
        }
        let pending = self.stage.join("enrollment.pending");
        if present(&pending)? {
            local::regular(&pending).map_err(|_| Error::Conflict)?;
        }
        let body = encode(record)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&pending)?;
        file.write_all(&body)?;
        file.sync_all()?;
        drop(file);
        fs::rename(pending, target)?;
        Ok(())
    }
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creator_finishes_pinning_after_racing_empty_marker_reader() {
        use std::{sync::mpsc, thread};
        let temp = tempfile::tempdir().unwrap();
        let input = super::super::tests::request(&temp);
        let context = Context::prepare(&input).unwrap();
        let marker = context.namespace.marker().to_path_buf();
        let (created_tx, created_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let creator = thread::spawn(move || {
            let fence = context
                .fence_hook(|new| {
                    assert!(new);
                    created_tx.send(()).unwrap();
                    continue_rx.recv().unwrap();
                })
                .unwrap();
            drop(fence);
            done_tx.send(()).unwrap();
        });
        created_rx.recv().unwrap();
        let reader = OpenOptions::new()
            .read(true)
            .write(true)
            .open(marker)
            .unwrap();
        reader.lock_exclusive().unwrap();
        continue_tx.send(()).unwrap();
        assert!(matches!(
            done_rx.recv_timeout(std::time::Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        FileExt::unlock(&reader).unwrap();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        creator.join().unwrap();
        let enrolled = initialize(&input).unwrap();
        assert_eq!(enrolled.capacity.minimum_checkpoint.sequence(), 0);
    }
}

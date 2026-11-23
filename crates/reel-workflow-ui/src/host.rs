//! Explicit private Windows viewer. No writes, discovery, hydration or execution.
use crate::{error, model::*};
use reel_assets::{
    jobs::{WorkspaceDirectory, open_workspace_directory},
    loopback,
};
use reel_workflow::{ErrorKind, Result, handoff::Journal};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};
fn io_error(value: std::io::Error) -> reel_workflow::Error {
    error(
        match value.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::Missing,
            std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
            _ if matches!(value.raw_os_error(), Some(32 | 33)) => ErrorKind::Busy,
            _ => ErrorKind::NeedsRecovery,
        },
        "board-file-access",
    )
}
fn directory(path: &Path) -> Result<WorkspaceDirectory> {
    open_workspace_directory(path)
        .map_err(|_| error(ErrorKind::Denied, "board-directory-namespace"))
}
fn file(path: &Path) -> Result<(File, WorkspaceDirectory)> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    let parent = directory(
        path.parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "board-file-parent"))?,
    )?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x0020_0000)
        .open(path)
        .map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err(error(ErrorKind::Denied, "board-ordinary-file"));
    }
    Ok((file, parent))
}
fn bytes(file: &mut File, maximum: u64) -> Result<Vec<u8>> {
    if file.metadata().map_err(io_error)?.len() > maximum {
        return Err(error(ErrorKind::Budget, "board-file-bytes"));
    }
    let mut raw = Vec::new();
    Read::by_ref(file)
        .take(maximum + 1)
        .read_to_end(&mut raw)
        .map_err(io_error)?;
    if raw.len() as u64 > maximum {
        return Err(error(ErrorKind::Budget, "board-file-growth"));
    }
    Ok(raw)
}
pub struct Source {
    pub board: Board,
    pub pin: String,
    _file: File,
    _parent: WorkspaceDirectory,
}
pub fn load(path: &Path, pin: &str) -> Result<Source> {
    let (mut file, parent) = file(path)?;
    let raw = bytes(&mut file, MAX_CONTROL_BYTES)?;
    if reel_workflow::digest(&raw) != pin {
        return Err(error(ErrorKind::Integrity, "board-profile-pin"));
    }
    let board: Board = serde_json::from_slice(&raw)
        .map_err(|_| error(ErrorKind::Invalid, "board-profile-json"))?;
    board.check()?;
    Ok(Source {
        board,
        pin: pin.into(),
        _file: file,
        _parent: parent,
    })
}
struct Watcher {
    directory: Option<WorkspaceDirectory>,
    records: Vec<File>,
    sizes: Vec<u64>,
    last_result: Option<reel_workflow::ReturnedResult>,
    last_language: Option<String>,
}
impl Watcher {
    fn new() -> Self {
        Self {
            directory: None,
            records: vec![],
            sizes: vec![],
            last_result: None,
            last_language: None,
        }
    }
    fn observe(&mut self, watch: &Watch, project: &str, budget: &mut u64) -> Result<Value> {
        if self.directory.is_none() {
            if fs::symlink_metadata(&watch.journal)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                return Ok(json!({"state":"unavailable","label":"Journal not available"}));
            }
            self.directory = Some(directory(&watch.journal)?);
        }
        let root = self.directory.as_ref().unwrap().path();
        let mut count = 0;
        for entry in fs::read_dir(root).map_err(io_error)? {
            entry.map_err(io_error)?;
            count += 1;
            if count > 3 {
                return Err(error(ErrorKind::Budget, "board-journal-entry-count"));
            }
        }
        if count < self.records.len() {
            return Err(error(ErrorKind::Stale, "board-journal-rollback"));
        }
        let mut amount = self.sizes.iter().sum::<u64>();
        if budget.saturating_add(amount) > 16 * 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "board-observation-budget"));
        }
        for index in self.records.len()..count {
            let (mut record, _parent) = file(&root.join(format!("{index:05}.json")))?;
            let declared = record.metadata().map_err(io_error)?.len();
            if budget.saturating_add(amount).saturating_add(declared) > 16 * 1024 * 1024 {
                return Err(error(ErrorKind::Budget, "board-observation-budget"));
            }
            let raw = bytes(&mut record, reel_workflow::MAX_CONTROL_BYTES as u64)?;
            amount += raw.len() as u64;
            self.sizes.push(raw.len() as u64);
            self.records.push(record);
        }
        *budget = budget
            .checked_add(amount)
            .filter(|n| *n <= 16 * 1024 * 1024)
            .ok_or_else(|| error(ErrorKind::Budget, "board-observation-budget"))?;
        let journal = Journal::open(root)?;
        if reel_workflow::canonical_digest(journal.ticket())? != watch.ticket_sha256
            || journal.ticket().packet.task_key != watch.task_key
            || journal.ticket().attempt_id != watch.attempt_id
            || journal.ticket().packet.project != project
        {
            return Err(error(ErrorKind::Conflict, "board-journal-ticket-binding"));
        }
        self.last_result = journal.result().cloned();
        self.last_language = journal.ticket().packet.task.language.clone();
        let (state, label) = match journal.state() {
            reel_workflow::handoff::State::Waiting => ("waiting", "Waiting for result"),
            reel_workflow::handoff::State::ImportPending => ("publishing", "Import pending"),
            reel_workflow::handoff::State::Retained => ("retained-recorded", "Retention recorded"),
            reel_workflow::handoff::State::Cancelled => ("cancelled", "Cancelled"),
        };
        Ok(
            json!({"state":state,"label":label,"task_key":watch.task_key,"attempt_id":watch.attempt_id,"source_commit":journal.ticket().packet.source_commit,"language":self.last_language,"records":count,"native_custody_verified":false}),
        )
    }
}
struct State {
    watchers: BTreeMap<String, Watcher>,
    resources: BTreeMap<String, Media>,
    data: Vec<u8>,
    basis: String,
    revision: u64,
    refreshed: Option<Instant>,
}
impl State {
    fn new(board: &Board) -> Self {
        Self {
            watchers: board
                .tasks
                .keys()
                .map(|k| (k.clone(), Watcher::new()))
                .collect(),
            resources: board.media.clone(),
            data: vec![],
            basis: String::new(),
            revision: 0,
            refreshed: None,
        }
    }
    fn refresh(&mut self, source: &Source) -> Result<()> {
        if self
            .refreshed
            .is_some_and(|time| time.elapsed() < Duration::from_millis(750))
        {
            return Ok(());
        }
        let mut observations = BTreeMap::new();
        let mut budget = 0u64;
        let mut view = serde_json::to_value(&source.board)
            .map_err(|_| error(ErrorKind::Invalid, "board-view-json"))?;
        view.as_object_mut().unwrap().remove("media");
        view.as_object_mut().unwrap().remove("tasks");
        for unit in view["units"].as_array_mut().unwrap() {
            for version in unit["versions"].as_array_mut().unwrap() {
                if let Some(key) = version["preview"].as_str() {
                    version["preview"] = json!(format!("media/{key}"));
                }
            }
        }
        for cue in view["cues"].as_array_mut().unwrap() {
            if let Some(key) = cue["preview"].as_str() {
                cue["preview"] = json!(format!("media/{key}"));
            }
        }
        for (index, (key, watch)) in source.board.tasks.iter().enumerate() {
            let observer = self.watchers.get_mut(key).unwrap();
            let observation=observer.observe(watch,&source.board.project,&mut budget).unwrap_or_else(|e|json!({"state":if e.kind==ErrorKind::Busy {"updating"}else{"needs-recovery"},"label":if e.kind==ErrorKind::Busy {"Updating observation"}else{"Observation needs attention"},"code":e.code,"native_custody_verified":false}));
            observations.insert(key, observation);
            if let Some(result) = &observer.last_result {
                for (output_index, (output, preview)) in watch.outputs.iter().enumerate() {
                    let Some(asset) = result.artifacts.get(output) else {
                        continue;
                    };
                    let media_key = format!("candidate-{index}-{output_index}");
                    let available = mime(&asset.media_type).is_some()
                        && asset.media_type.starts_with("image/")
                        && asset.bytes <= MAX_PREVIEW_BYTES;
                    if available {
                        self.resources.insert(
                            media_key.clone(),
                            Media {
                                artifact: asset.clone(),
                                path: preview.path.clone(),
                                subject: None,
                            },
                        );
                    }
                    let unit = view["units"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|u| u["id"] == preview.unit_id)
                        .ok_or_else(|| error(ErrorKind::Conflict, "board-candidate-unit"))?;
                    unit["versions"].as_array_mut().unwrap().push(json!({"id":format!("candidate-{key}-{output}"),"label":preview.label,"asset":asset,"designation":"candidate","language":observer.last_language,"preview":if available{Some(format!("media/{media_key}"))}else{None},"approval_reference":null,"task":key}));
                }
            }
        }
        view["tasks"] = json!(observations);
        view["board_pin"] = json!(source.pin);
        view["read_only"] = json!(true);
        view["approvals_verified"] = json!(false);
        let basis = reel_workflow::digest(
            &serde_json::to_vec(&view).map_err(|_| error(ErrorKind::Invalid, "board-view-json"))?,
        );
        if basis != self.basis {
            self.revision += 1;
            self.basis = basis;
        }
        view["revision"] = json!(self.revision);
        view["basis_sha256"] = json!(self.basis);
        self.data =
            serde_json::to_vec(&view).map_err(|_| error(ErrorKind::Invalid, "board-view-json"))?;
        if self.data.len() as u64 > MAX_CONTROL_BYTES {
            return Err(error(ErrorKind::Budget, "board-view-bytes"));
        }
        self.refreshed = Some(Instant::now());
        Ok(())
    }
}
struct OpenMedia {
    file: Mutex<File>,
    _parent: WorkspaceDirectory,
    bytes: u64,
    modified: Option<SystemTime>,
    sha256: String,
    mime: &'static str,
}
impl OpenMedia {
    fn open(media: &Media, deadline: Instant) -> Result<Self> {
        let mime = mime(&media.artifact.media_type)
            .ok_or_else(|| error(ErrorKind::Unsupported, "board-preview-type"))?;
        if media.artifact.bytes > MAX_PREVIEW_BYTES {
            return Err(error(ErrorKind::Budget, "board-preview-bytes"));
        }
        let (mut file, parent) = file(&media.path)?;
        let metadata = file.metadata().map_err(io_error)?;
        if metadata.len() != media.artifact.bytes {
            return Err(error(ErrorKind::Integrity, "board-preview-length"));
        }
        let mut hash = Sha256::new();
        let mut read = 0u64;
        let mut buffer = [0; 65536];
        loop {
            if Instant::now() >= deadline {
                return Err(error(ErrorKind::Budget, "board-preview-deadline"));
            }
            let count = file.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            read += count as u64;
            if read > media.artifact.bytes {
                return Err(error(ErrorKind::Integrity, "board-preview-growth"));
            }
            hash.update(&buffer[..count]);
        }
        let sha256: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if read != media.artifact.bytes || sha256 != media.artifact.sha256 {
            return Err(error(ErrorKind::Integrity, "board-preview-content"));
        }
        Ok(Self {
            file: Mutex::new(file),
            _parent: parent,
            bytes: metadata.len(),
            modified: metadata.modified().ok(),
            sha256,
            mime,
        })
    }
}
pub struct Viewer {
    source: Source,
    state: Mutex<State>,
    media: Mutex<BTreeMap<String, Arc<OpenMedia>>>,
}
impl Viewer {
    pub fn new(source: Source) -> Self {
        let state = State::new(&source.board);
        Self {
            source,
            state: Mutex::new(state),
            media: Mutex::new(BTreeMap::new()),
        }
    }
    pub fn view(&self) -> Result<Vec<u8>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| error(ErrorKind::Busy, "board-observation-busy"))?;
        state.refresh(&self.source)?;
        Ok(state.data.clone())
    }
    fn preview(&self, key: &str, deadline: Instant) -> Result<Arc<OpenMedia>> {
        if let Some(media) = self
            .media
            .lock()
            .map_err(|_| error(ErrorKind::Busy, "board-readers-busy"))?
            .get(key)
            .cloned()
        {
            return Ok(media);
        }
        let descriptor = self
            .state
            .lock()
            .map_err(|_| error(ErrorKind::Busy, "board-observation-busy"))?
            .resources
            .get(key)
            .cloned()
            .ok_or_else(|| error(ErrorKind::Missing, "board-preview-not-declared"))?;
        let resource = Arc::new(OpenMedia::open(&descriptor, deadline)?);
        let mut cache = self
            .media
            .lock()
            .map_err(|_| error(ErrorKind::Busy, "board-readers-busy"))?;
        if cache.len() >= 8 {
            let first = cache.keys().next().unwrap().clone();
            cache.remove(&first);
        }
        cache.insert(key.into(), Arc::clone(&resource));
        Ok(resource)
    }
    fn handle(
        &self,
        stream: &mut TcpStream,
        host: &str,
        token: &str,
        deadline: Instant,
    ) -> Result<bool> {
        let request = match loopback::read_request(stream, host, token, deadline) {
            Ok(r) => r,
            Err(status) => {
                let _ = loopback::response(stream, status, "text/plain", 0, "");
                return Ok(false);
            }
        };
        if let Some(key) = request.path.strip_prefix("media/") {
            let resource = match self.preview(key, deadline) {
                Ok(r) => r,
                Err(error) => {
                    let status = if error.kind == ErrorKind::Missing {
                        404
                    } else {
                        409
                    };
                    loopback::response(stream, status, "text/plain", 0, "").map_err(io_error)?;
                    return Ok(false);
                }
            };
            let mut file = resource
                .file
                .lock()
                .map_err(|_| error(ErrorKind::Busy, "board-preview-reader"))?;
            let metadata = file.metadata().map_err(io_error)?;
            if metadata.len() != resource.bytes || metadata.modified().ok() != resource.modified {
                loopback::response(stream, 409, "text/plain", 0, "").map_err(io_error)?;
                return Ok(false);
            }
            let (start, length, partial) =
                match loopback::byte_range(request.range.as_deref(), resource.bytes) {
                    Ok(v) => v,
                    Err(status) => {
                        loopback::response(
                            stream,
                            status,
                            "text/plain",
                            0,
                            &format!("Content-Range: bytes */{}\r\n", resource.bytes),
                        )
                        .map_err(io_error)?;
                        return Ok(false);
                    }
                };
            let extra = if partial {
                format!(
                    "Accept-Ranges: bytes\r\nContent-Range: bytes {}-{}/{}\r\nETag: \"{}\"\r\n",
                    start,
                    start + length - 1,
                    resource.bytes,
                    resource.sha256
                )
            } else {
                format!("Accept-Ranges: bytes\r\nETag: \"{}\"\r\n", resource.sha256)
            };
            loopback::response(
                stream,
                if partial { 206 } else { 200 },
                resource.mime,
                length,
                &extra,
            )
            .map_err(io_error)?;
            if !request.head {
                file.seek(SeekFrom::Start(start)).map_err(io_error)?;
                let mut remaining = length;
                let mut buffer = [0; 65536];
                while remaining > 0 {
                    if Instant::now() >= deadline {
                        return Ok(false);
                    }
                    let count = file
                        .read(&mut buffer[..remaining.min(65536) as usize])
                        .map_err(io_error)?;
                    if count == 0 {
                        return Err(error(ErrorKind::Integrity, "board-preview-stream"));
                    }
                    stream.write_all(&buffer[..count]).map_err(io_error)?;
                    remaining -= count as u64;
                }
            }
            return Ok(true);
        }
        let (body, mime): (Vec<u8>, &str) = match request.path.as_str() {
            "index.html" => (
                include_bytes!("board.html").to_vec(),
                "text/html; charset=utf-8",
            ),
            "board.js" => (
                include_bytes!("board.js").to_vec(),
                "application/javascript; charset=utf-8",
            ),
            "board.css" => (
                include_bytes!("board.css").to_vec(),
                "text/css; charset=utf-8",
            ),
            "state.json" => (self.view()?, "application/json; charset=utf-8"),
            _ => {
                loopback::response(stream, 404, "text/plain", 0, "").map_err(io_error)?;
                return Ok(false);
            }
        };
        let (start, length, partial) =
            match loopback::byte_range(request.range.as_deref(), body.len() as u64) {
                Ok(v) => v,
                Err(status) => {
                    loopback::response(stream, status, "text/plain", 0, "").map_err(io_error)?;
                    return Ok(false);
                }
            };
        let extra = if partial {
            format!(
                "Content-Range: bytes {}-{}/{}\r\n",
                start,
                start + length - 1,
                body.len()
            )
        } else {
            String::new()
        };
        loopback::response(
            stream,
            if partial { 206 } else { 200 },
            mime,
            length,
            &extra,
        )
        .map_err(io_error)?;
        if !request.head {
            stream
                .write_all(&body[start as usize..(start + length) as usize])
                .map_err(io_error)?;
        }
        Ok(true)
    }
}
pub struct Options {
    pub port: u16,
    pub idle_seconds: u64,
    pub max_seconds: u64,
    pub open: bool,
}
pub fn serve(source: Source, options: Options) -> Result<()> {
    if !(1..=86400).contains(&options.max_seconds) || !(1..=3600).contains(&options.idle_seconds) {
        return Err(error(ErrorKind::Invalid, "board-host-deadlines"));
    }
    let viewer = Arc::new(Viewer::new(source));
    let listener =
        TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, options.port)).map_err(io_error)?;
    listener.set_nonblocking(true).map_err(io_error)?;
    let host = listener.local_addr().map_err(io_error)?.to_string();
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|_| error(ErrorKind::Unsupported, "board-randomness"))?;
    let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let url = format!("http://{host}/{token}/index.html");
    println!(
        "{}",
        json!({"schema":"reel.workflow-board-host.v1","url":url,"read_only":true,"media_opened_at_startup":0})
    );
    std::io::stdout().flush().map_err(io_error)?;
    if options.open && loopback::open_browser(&url).is_err() {
        eprintln!("Browser launch unavailable; use the private startup URL.");
    }
    let epoch = Instant::now();
    let deadline = epoch + Duration::from_secs(options.max_seconds);
    let last = Arc::new(AtomicU64::new(0));
    let mut workers: Vec<thread::JoinHandle<()>> = vec![];
    loop {
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                workers
                    .swap_remove(index)
                    .join()
                    .map_err(|_| error(ErrorKind::NeedsRecovery, "board-reader-failed"))?;
            } else {
                index += 1;
            }
        }
        let idle = epoch
            + Duration::from_millis(last.load(Ordering::Relaxed))
            + Duration::from_secs(options.idle_seconds);
        if Instant::now() >= deadline || Instant::now() >= idle {
            break;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if workers.len() >= 16 {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                    let _ = loopback::response(&mut stream, 503, "text/plain", 0, "");
                    continue;
                }
                let viewer = Arc::clone(&viewer);
                let host = host.clone();
                let token = token.clone();
                let last = Arc::clone(&last);
                let request_deadline = deadline
                    .min(idle)
                    .min(Instant::now() + Duration::from_secs(10));
                workers.push(thread::spawn(move || {
                    if matches!(
                        viewer.handle(&mut stream, &host, &token, request_deadline),
                        Ok(true)
                    ) {
                        last.fetch_max(epoch.elapsed().as_millis() as u64, Ordering::Relaxed);
                    }
                }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(io_error(e)),
        }
    }
    drop(listener);
    for worker in workers {
        worker
            .join()
            .map_err(|_| error(ErrorKind::NeedsRecovery, "board-reader-failed"))?;
    }
    println!(
        "{}",
        json!({"schema":"reel.workflow-board-stopped.v1","jobs_cancelled":false,"claims_released":false,"scratch_retired":false})
    );
    Ok(())
}

//! Owned quiet child trees; no cache initialization or capacity reservation.
use anyhow::{Context, Result, bail};
use reel_assets::jobs::{QuietLimits, run_quiet};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) fn usage(root: &Path, budget: u64) -> Result<u64> {
    fn walk(
        root: &Path,
        depth: usize,
        files: &mut usize,
        total: &mut u64,
        budget: u64,
    ) -> Result<()> {
        use std::os::windows::fs::MetadataExt;
        if depth > 32 {
            bail!("browser workspace depth exceeded");
        }
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let meta = match fs::symlink_metadata(entry.path()) {
                Ok(meta) => meta,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            *files += 1;
            if *files > 16384 || meta.file_attributes() & 0x400 != 0 {
                bail!("browser workspace entry bound or type exceeded");
            }
            if meta.is_dir() {
                walk(&entry.path(), depth + 1, files, total, budget)?;
            } else if meta.is_file() {
                *total = total
                    .checked_add(meta.len())
                    .context("browser workspace size overflow")?;
                if *total > budget {
                    bail!("browser workspace byte budget exceeded");
                }
            } else {
                bail!("browser workspace contains unsupported entries");
            }
        }
        Ok(())
    }
    let mut total = 0;
    walk(root, 0, &mut 0, &mut total, budget)?;
    Ok(total)
}

pub(super) struct Task {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<(), String>>>,
}
impl Task {
    pub fn start(
        program: &Path,
        args: Vec<String>,
        workspace: &Path,
        budget: u64,
        timeout: Duration,
        limits: QuietLimits,
    ) -> Result<Self> {
        let program = program.to_owned();
        let workspace = workspace.to_owned();
        let stop = Arc::new(AtomicBool::new(false));
        let child_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("reel-browser-child".into())
            .spawn(move || {
                run_quiet(&program, &args, &workspace, timeout, limits, || {
                    if child_stop.load(Ordering::SeqCst) {
                        return Err("contained child cancelled".into());
                    }
                    usage(&workspace, budget).map_err(|e| e.to_string())?;
                    Ok(())
                })
                .map_err(|e| e.to_string())
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
    pub fn ended(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
    pub fn check_running(&mut self) -> Result<()> {
        if self.ended() {
            self.join()?;
            bail!("contained child exited before its stream completed");
        }
        Ok(())
    }
    pub fn wait(&mut self, deadline: Instant) -> Result<()> {
        while !self.ended() {
            pause(deadline)?;
        }
        self.join()
    }
    fn join(&mut self) -> Result<()> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        thread
            .join()
            .map_err(|_| anyhow::anyhow!("contained child controller panicked"))?
            .map_err(|e| anyhow::anyhow!(e))
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.join();
    }
}
pub(super) fn pause(deadline: Instant) -> Result<()> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|v| !v.is_zero())
        .context("browser worker deadline elapsed")?;
    thread::sleep(remaining.min(Duration::from_millis(2)));
    Ok(())
}
pub(super) fn native_path(path: &Path) -> Result<String> {
    let path: PathBuf = path.canonicalize()?;
    let value = path.to_str().context("browser native path is not UTF-8")?;
    Ok(value.strip_prefix(r"\\?\").unwrap_or(value).to_owned())
}

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
    let mut total = 0;
    walk_usage(root, 0, &mut 0, &mut total, budget)?;
    Ok(total)
}

fn walk_usage(
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
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        // Chromium can remove an observed child directory between its
        // parent enumeration and this descent. Its bytes are now gone.
        // A missing owned root, access error or other failure still refuses.
        Err(error) if depth > 0 && error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        // Windows directory enumeration already supplies attributes and size.
        // Reopening each path for metadata races Chromium's exclusive handles
        // (and delete-pending files) and can fail with AccessDenied mid-frame.
        // DirEntry metadata does not follow reparse points; keep rejecting them.
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        *files += 1;
        if *files > 16384 || meta.file_attributes() & 0x400 != 0 {
            bail!("browser workspace entry bound or type exceeded");
        }
        if meta.is_dir() {
            walk_usage(&entry.path(), depth + 1, files, total, budget)?;
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

/// Sample observational disk usage independently of protocol polling frequency.
/// Child controllers keep their own checks; callers force a final full scan.
/// This is never a filesystem quota or a substitute for authority checks.
pub(super) struct UsageObserver {
    root: PathBuf,
    budget: u64,
    last: Instant,
    failed: Option<String>,
    #[cfg(test)]
    scans: usize,
}
impl UsageObserver {
    pub fn new(root: &Path, budget: u64) -> Result<Self> {
        let mut observer = Self {
            root: root.to_owned(),
            budget,
            last: Instant::now(),
            failed: None,
            #[cfg(test)]
            scans: 0,
        };
        observer.force()?;
        Ok(observer)
    }
    pub fn check(&mut self) -> Result<()> {
        self.check_at(Instant::now())
    }
    fn check_at(&mut self, now: Instant) -> Result<()> {
        if let Some(error) = &self.failed {
            bail!("{error}");
        }
        if now.saturating_duration_since(self.last) >= Duration::from_millis(250) {
            self.force()?;
        }
        Ok(())
    }
    pub fn force(&mut self) -> Result<()> {
        if let Some(error) = &self.failed {
            bail!("{error}");
        }
        #[cfg(test)]
        {
            self.scans += 1;
        }
        if let Err(error) = usage(&self.root, self.budget) {
            self.failed = Some(error.to_string());
            return Err(error);
        }
        self.last = Instant::now();
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_observed_child_does_not_cancel_the_owned_workspace() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("transient-browser-cache");
        fs::create_dir(&child).unwrap();
        // Reproduce the exact gap after the parent observed a directory and
        // before recursive enumeration, without relying on thread scheduling.
        assert!(fs::metadata(&child).unwrap().is_dir());
        fs::remove_dir(&child).unwrap();
        let mut files = 1;
        let mut total = 7;
        walk_usage(&child, 1, &mut files, &mut total, 7).unwrap();
        assert_eq!((files, total), (1, 7));
        assert!(walk_usage(&child, 0, &mut files, &mut total, 7).is_err());
        fs::write(root.path().join("retained.bin"), b"still-counted").unwrap();
        assert_eq!(usage(root.path(), 13).unwrap(), 13);
        assert!(usage(root.path(), 12).is_err());
    }
    #[test]
    fn exclusive_child_files_are_counted_without_reopening_or_ignoring_them() {
        use std::{io::Write, os::windows::fs::OpenOptionsExt};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("exclusive.bin");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        file.write_all(b"child-owned").unwrap();
        file.sync_all().unwrap();
        assert_eq!(usage(root.path(), 11).unwrap(), 11);
        assert!(usage(root.path(), 10).is_err());
        drop(file);
    }
    #[test]
    fn protocol_polling_does_not_rescan_workspace_and_forced_exit_checks_new_bytes() {
        let root = tempfile::tempdir().unwrap();
        let mut observed = UsageObserver::new(root.path(), 4).unwrap();
        let now = observed.last;
        for _ in 0..1000 {
            observed.check_at(now).unwrap();
        }
        assert_eq!(observed.scans, 1);
        fs::write(root.path().join("new.bin"), b"over budget").unwrap();
        assert!(observed.force().is_err());
        fs::remove_file(root.path().join("new.bin")).unwrap();
        assert!(
            observed.check().is_err(),
            "a failed admission must stay failed"
        );
        assert!(observed.force().is_err());
    }
    #[test]
    fn expired_observation_samples_the_current_workspace() {
        let root = tempfile::tempdir().unwrap();
        let mut observed = UsageObserver::new(root.path(), 4).unwrap();
        fs::write(root.path().join("new.bin"), b"over budget").unwrap();
        observed.last = Instant::now() - Duration::from_secs(1);
        assert!(observed.check().is_err());
        assert_eq!(observed.scans, 2);
    }
}

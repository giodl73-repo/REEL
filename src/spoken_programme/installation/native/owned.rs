//! One explicit quiet child tree. Actual process reaping precedes lease release.
use super::*;
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
struct Task {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<std::result::Result<(), String>>>,
}
impl Drop for Task {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
pub(super) fn run(
    program: &Path,
    args: Vec<String>,
    root: &Path,
    timeout: Duration,
    cancellation: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    ensure!(
        !timeout.is_zero() && timeout <= Duration::from_secs(2),
        "installation native deadline profile"
    );
    cancellation()?;
    let stop = Arc::new(AtomicBool::new(false));
    let child_stop = stop.clone();
    let program = program.to_owned();
    let root = root.to_owned();
    let deadline = Instant::now() + timeout;
    let thread = std::thread::Builder::new()
        .name("reel-installation-child-owner".into())
        .spawn(move || {
            reel_assets::jobs::run_quiet(
                &program,
                &args,
                &root,
                timeout,
                reel_assets::jobs::QuietLimits {
                    memory_bytes: 256 * 1024 * 1024,
                    processes: 1,
                },
                || {
                    if child_stop.load(Ordering::Acquire) {
                        return Err("installation native operation cancelled".into());
                    }
                    let mut total = 0;
                    let mut count = 0;
                    for entry in fs::read_dir(&root)? {
                        let entry = entry?;
                        let meta = entry.metadata()?;
                        count += 1;
                        if !meta.is_file() || count > 3 {
                            return Err("installation probe workspace shape".into());
                        }
                        total += meta.len();
                    }
                    if total > 32 * 1024 {
                        return Err("installation probe workspace budget".into());
                    }
                    Ok(())
                },
            )
            .map_err(|e| e.to_string())
        })?;
    let mut task = Task {
        stop,
        thread: Some(thread),
    };
    while !task.thread.as_ref().is_none_or(|t| t.is_finished()) {
        cancellation()?;
        ensure!(
            Instant::now() < deadline,
            "installation native operation deadline elapsed"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    task.thread
        .take()
        .unwrap()
        .join()
        .map_err(|_| anyhow::anyhow!("installation child owner panicked"))?
        .map_err(anyhow::Error::msg)?;
    cancellation()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{Foundation::*, System::Threading::*};
    struct Process(HANDLE);
    impl Drop for Process {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    #[test]
    #[ignore = "owned subprocess fixture only; explicitly launched by parent"]
    fn worker_fixture() {
        let root = std::env::current_dir().unwrap();
        if !root.join("fixture.mode").is_file() {
            panic!("missing explicit worker fixture")
        }
        fs::write(root.join("pid"), std::process::id().to_string()).unwrap();
        std::thread::sleep(Duration::from_secs(30));
        fs::write(root.join("late"), b"not reaped").unwrap();
    }
    #[test]
    fn timeout_and_cancellation_reap_actual_owned_process_before_return() {
        let exe = std::env::current_exe().unwrap();
        let identity = crate::media_runtime::ByteIdentity::observe_file(&exe).unwrap();
        let tool = crate::media_runtime::BoundTool::bind(&crate::scene_delivery::FileRef {
            path: exe,
            sha256: identity.sha256,
            bytes: identity.bytes,
        })
        .unwrap();
        for cancel in [true, false] {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join("fixture.mode"), b"stall").unwrap();
            let mut process = None;
            let began = Instant::now();
            let args = vec![
                "--ignored".into(),
                "--exact".into(),
                "spoken_programme::installation::native::owned::tests::worker_fixture".into(),
                "--nocapture".into(),
            ];
            let result = run(
                tool.path(),
                args,
                root.path(),
                Duration::from_secs(2),
                &mut || {
                    if process.is_none() && root.path().join("pid").is_file() {
                        let raw = fs::read_to_string(root.path().join("pid"))?;
                        if let Ok(pid) = raw.parse::<u32>() {
                            process = Some(Process(unsafe {
                                OpenProcess(
                                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                                    false,
                                    pid,
                                )
                            }?));
                        }
                    }
                    if cancel && process.is_some() {
                        anyhow::bail!("operator cancelled")
                    }
                    Ok(())
                },
            );
            assert!(result.is_err());
            assert!(began.elapsed() < Duration::from_secs(4));
            let process =
                process.expect("fixture actually started and supplied a held process handle");
            assert_eq!(unsafe { WaitForSingleObject(process.0, 0) }, WAIT_OBJECT_0);
            assert!(!root.path().join("late").exists());
        }
    }
}

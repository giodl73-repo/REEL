//! Owned, deadline-limited subprocess pipes. No movie files or shell commands.
use anyhow::{Context, Result, bail};
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct Pipeline {
    children: Vec<Arc<Mutex<Child>>>,
    errors: Vec<JoinHandle<std::io::Result<usize>>>,
    stop: Option<mpsc::Sender<()>>,
    watchdog: Option<JoinHandle<()>>,
}
impl Drop for Pipeline {
    fn drop(&mut self) {
        for child in &self.children {
            if let Ok(mut child) = child.lock() {
                let _ = child.kill();
            }
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.watchdog.take() {
            let _ = task.join();
        }
        for child in &self.children {
            if let Ok(mut child) = child.lock() {
                let _ = child.wait();
            }
        }
        for task in self.errors.drain(..) {
            let _ = task.join();
        }
    }
}
pub(super) fn run(commands: Vec<Command>, maximum: usize, deadline: Instant) -> Result<Vec<u8>> {
    if commands.is_empty() || commands.len() > 130 || Instant::now() >= deadline {
        bail!("invalid or expired primary reference pipeline");
    }
    let mut pipeline = Pipeline {
        children: Vec::new(),
        errors: Vec::new(),
        stop: None,
        watchdog: None,
    };
    let mut previous = None;
    for mut command in commands {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
            .stdin(previous.take().map(Stdio::from).unwrap_or_else(Stdio::null))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().context("start primary reference process")?;
        previous = child.stdout.take();
        let mut errors = child
            .stderr
            .take()
            .context("reference process stderr missing")?;
        pipeline.errors.push(thread::spawn(move || {
            let mut buffer = [0; 4096];
            let mut count = 0usize;
            loop {
                let n = errors.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                count = count.saturating_add(n);
            }
            Ok(count)
        }));
        pipeline.children.push(Arc::new(Mutex::new(child)));
    }
    let owned = pipeline.children.clone();
    let (stop, cancelled) = mpsc::channel();
    pipeline.stop = Some(stop);
    pipeline.watchdog = Some(thread::spawn(move || {
        if matches!(
            cancelled.recv_timeout(deadline.saturating_duration_since(Instant::now())),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            for child in owned {
                if let Ok(mut child) = child.lock() {
                    let _ = child.kill();
                }
            }
        }
    }));
    let mut output = Vec::new();
    previous
        .context("reference stdout missing")?
        .take(maximum as u64 + 1)
        .read_to_end(&mut output)?;
    if output.len() > maximum {
        bail!("reference process diagnostic budget exceeded");
    }
    for (index, child) in pipeline.children.iter().enumerate() {
        let status = loop {
            if let Some(status) = child
                .lock()
                .map_err(|_| anyhow::anyhow!("reference process owner poisoned"))?
                .try_wait()?
            {
                break status;
            }
            thread::sleep(Duration::from_millis(10));
        };
        if !status.success() {
            bail!("primary reference process stage {index} failed ({status})");
        }
    }
    for errors in pipeline.errors.drain(..) {
        if errors
            .join()
            .map_err(|_| anyhow::anyhow!("reference stderr task failed"))??
            != 0
        {
            bail!("primary reference process reported a decode error");
        }
    }
    if Instant::now() >= deadline {
        bail!("primary reference deadline exceeded");
    }
    Ok(output)
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;

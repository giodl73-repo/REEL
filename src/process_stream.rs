//! Reap a streamed child on every read/error path before caller leases end.
use anyhow::{Context, Result, bail};
use std::{
    io::{self, Read},
    process::{Child, ChildStdout},
    thread::JoinHandle,
};

const STDERR_MAX: usize = 65_536;
struct Reaper<'a> {
    child: &'a mut Child,
    stderr: Option<JoinHandle<io::Result<Vec<u8>>>>,
    waited: bool,
}
impl Drop for Reaper<'_> {
    fn drop(&mut self) {
        if !self.waited {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
    }
}

/// Call immediately after spawning a piped decoder. Always drain stderr while
/// consuming stdout, retaining at most 64KiB of private diagnostics.
pub(crate) fn read<T>(
    child: &mut Child,
    consume: impl FnOnce(ChildStdout) -> Result<T>,
) -> Result<T> {
    let mut guard = Reaper {
        child,
        stderr: None,
        waited: false,
    };
    let mut stderr = guard
        .child
        .stderr
        .take()
        .context("missing decoder stderr pipe")?;
    guard.stderr = Some(
        std::thread::Builder::new()
            .name("reel-decoder-stderr".into())
            .spawn(move || {
                let mut kept = Vec::new();
                let mut buffer = [0u8; 8192];
                loop {
                    let n = stderr.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    let retain = n.min(STDERR_MAX - kept.len());
                    kept.extend_from_slice(&buffer[..retain]);
                }
                Ok(kept)
            })?,
    );
    let stdout = guard
        .child
        .stdout
        .take()
        .context("missing decoder stdout pipe")?;
    let result = consume(stdout)?;
    let status = guard.child.wait()?;
    guard.waited = true;
    let stderr = guard
        .stderr
        .take()
        .context("missing stderr reader")?
        .join()
        .map_err(|_| anyhow::anyhow!("decoder stderr reader panicked"))??;
    if !status.success() {
        bail!(
            "streamed media process failed: {}",
            String::from_utf8_lossy(&stderr)
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Command, Stdio},
    };
    fn child(mode: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "process_stream::tests::fixture_child",
                "--nocapture",
            ])
            .env("REEL_STREAM_FIXTURE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
    #[test]
    #[ignore = "subprocess fixture only"]
    fn fixture_child() {
        let Ok(mode) = std::env::var("REEL_STREAM_FIXTURE") else {
            return;
        };
        if mode == "flood" {
            std::io::stderr()
                .write_all(&vec![b'x'; 1024 * 1024])
                .unwrap();
        }
        println!("READY");
        std::io::stdout().flush().unwrap();
        if mode == "wait" {
            std::thread::sleep(std::time::Duration::from_secs(120));
        }
    }
    #[test]
    fn drains_large_stderr_and_reaps_a_child_when_consumption_fails() {
        let mut flood = child("flood");
        let text = read(&mut flood, |mut stdout| {
            let mut text = String::new();
            stdout.read_to_string(&mut text)?;
            Ok(text)
        })
        .unwrap();
        assert!(text.contains("READY"));
        assert!(flood.try_wait().unwrap().is_some());
        let mut waiting = child("wait");
        let error = read(&mut waiting, |stdout| -> Result<()> {
            for line in BufReader::new(stdout).lines() {
                if line?.contains("READY") {
                    bail!("injected consumer read failure");
                }
            }
            bail!("fixture ended before readiness");
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected consumer read failure"));
        assert!(waiting.try_wait().unwrap().is_some());
    }
}

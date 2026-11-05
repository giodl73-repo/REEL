//! Deadline-limited private media process. Always reap before input guards drop.
use anyhow::{Context, Result, bail};
use std::{
    io::Read,
    process::{Child, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub(super) fn run<T: Send + 'static>(
    command: &mut Command,
    deadline: Instant,
    consume: impl FnOnce(ChildStdout) -> Result<T> + Send + 'static,
) -> Result<(T, Vec<u8>)> {
    if Instant::now() >= deadline {
        bail!("album deadline exhausted");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = Owned(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("start declared album tool")?,
    );
    let stdout = child.0.stdout.take().context("album stdout missing")?;
    let mut stderr = child.0.stderr.take().context("album stderr missing")?;
    thread::scope(|scope| -> Result<(T, Vec<u8>)> {
        let output = scope.spawn(move || consume(stdout));
        let errors = scope.spawn(move || -> std::io::Result<Vec<u8>> {
            let mut tail = Vec::new();
            let mut buf = [0; 8192];
            loop {
                let n = stderr.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > 131072 {
                    tail.drain(..tail.len() - 131072);
                }
            }
            Ok(tail)
        });
        let status = loop {
            if Instant::now() >= deadline {
                // Kill before joining pipe readers on every timeout path.
                let _ = child.0.kill();
                let _ = child.0.wait();
                break None;
            }
            match child.0.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = child.0.kill();
                    let _ = child.0.wait();
                    let _ = output.join();
                    let _ = errors.join();
                    return Err(error.into());
                }
            }
        };
        let output = output
            .join()
            .map_err(|_| anyhow::anyhow!("album output reader failed"))?;
        let errors = errors
            .join()
            .map_err(|_| anyhow::anyhow!("album diagnostic reader failed"))??;
        if !status.is_some_and(|status| status.success()) {
            bail!("album tool failed or exceeded its deadline");
        }
        Ok((output?, errors))
    })
}

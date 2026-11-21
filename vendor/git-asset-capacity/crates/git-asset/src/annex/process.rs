//! Reuses the prototype's bounded Windows job-object process boundary, read only.
use super::*;
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use std::{io, process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};

pub(super) struct Output {
    pub stdout: Vec<u8>,
}
pub(super) fn supported() -> bool {
    cfg!(windows) && tokio::runtime::Handle::try_current().is_err()
}
pub(super) fn run(p: &Profile, annex: bool, args: &[&str]) -> Result<Output> {
    if !supported() {
        return Err(Error::Unsupported);
    }
    if std::fs::metadata(&p.empty_config)
        .map_err(|_| Error::Observation)?
        .len()
        != 0
        || std::fs::read_dir(&p.empty_hooks)
            .map_err(|_| Error::Observation)?
            .next()
            .is_some()
        || !p.temporary.is_dir()
    {
        return Err(Error::Invalid);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::Observation)?;
    runtime.block_on(execute(p, annex, args, true))
}
#[cfg(all(test, windows))]
pub(super) fn fixture_command(p: &Profile, annex: bool, args: &[&str]) -> Result<Output> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::Observation)?;
    runtime.block_on(execute(p, annex, args, false))
}
async fn execute(p: &Profile, annex: bool, args: &[&str], strict_stderr: bool) -> Result<Output> {
    let path = std::env::join_paths(&p.tools).map_err(|_| Error::Invalid)?;
    let mut wrap = CommandWrap::with_new(if annex { &p.annex } else { &p.git }, |c| {
        c.args(args)
            .current_dir(&p.repository)
            .env_clear()
            .env("PATH", path)
            .env("TEMP", &p.temporary)
            .env("TMP", &p.temporary)
            .env("USERPROFILE", &p.temporary)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &p.empty_config)
            .env("GIT_CONFIG_COUNT", "4")
            .env("GIT_CONFIG_KEY_0", "core.hooksPath")
            .env("GIT_CONFIG_VALUE_0", &p.empty_hooks)
            .env("GIT_CONFIG_KEY_1", "core.fsmonitor")
            .env("GIT_CONFIG_VALUE_1", "false")
            .env("GIT_CONFIG_KEY_2", "credential.helper")
            .env("GIT_CONFIG_VALUE_2", "")
            .env("GIT_CONFIG_KEY_3", "annex.alwayscommit")
            .env("GIT_CONFIG_VALUE_3", "false")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("GIT_DIR", if p.bare { "." } else { ".git" })
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !p.bare {
            c.env("GIT_WORK_TREE", ".");
        }
        if let Some(value) = std::env::var_os("SystemRoot") {
            c.env("SystemRoot", value);
        }
    });
    wrap.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        use process_wrap::tokio::{CreationFlags, JobObject};
        let mut flags = CreationFlags(Default::default());
        flags.0.0 = 0x08000000;
        wrap.wrap(flags).wrap(JobObject);
    }
    let mut child = wrap.spawn().map_err(|_| Error::Observation)?;
    let stdout = child.stdout().take().ok_or(Error::Observation)?;
    let stderr = child.stderr().take().ok_or(Error::Observation)?;
    let observed = tokio::time::timeout(Duration::from_millis(p.command_millis), async {
        tokio::try_join!(
            capture(stdout, p.output_bytes),
            capture(stderr, p.output_bytes),
            child.wait()
        )
    })
    .await;
    match observed {
        Ok(Ok((stdout, stderr, status))) => {
            if status.success() && (!strict_stderr || stderr.is_empty()) {
                Ok(Output { stdout })
            } else if annex && status.code() == Some(1) && stdout.is_empty() && stderr.is_empty() {
                Err(Error::Missing)
            } else {
                Err(Error::Observation)
            }
        }
        failure => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            match failure {
                Ok(Err(e)) if e.kind() == io::ErrorKind::OutOfMemory => Err(Error::Budget),
                _ => Err(Error::Observation),
            }
        }
    }
}
async fn capture(reader: impl AsyncRead + Unpin, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![];
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(io::Error::from(io::ErrorKind::OutOfMemory));
    }
    Ok(bytes)
}

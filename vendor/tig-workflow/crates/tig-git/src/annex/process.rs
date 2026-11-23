//! One contained, bounded subprocess boundary. No shell or arbitrary annex argv.
use super::identity::Key;
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, io, path::PathBuf, process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub git: PathBuf,
    pub executable: PathBuf,
    pub repository: PathBuf,
    pub tools: Vec<PathBuf>,
    pub empty_config: PathBuf,
    pub empty_hooks: PathBuf,
    pub temporary: PathBuf,
    pub legacy_profile: Option<PathBuf>,
    pub command_millis: u64,
    pub output_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<ControlBinding>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlBinding {
    pub root: PathBuf,
    pub index: tig_core::Digest,
    pub minimum: tig_store::source_claims::Checkpoint,
    pub store: tig_core::Digest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote(String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uuid(String);
impl Uuid {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn store(&self) -> tig_core::Digest {
        tig_core::Digest::blob(format!("tig.annex-store.design-1\n{}\n", self.0).as_bytes())
    }
    pub fn parse(value: &str) -> io::Result<Self> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(n, b)| {
                if [8, 13, 18, 23].contains(&n) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                }
            })
        {
            return Err(invalid());
        }
        Ok(Self(value.into()))
    }
}
impl Remote {
    pub fn parse(value: &str) -> io::Result<Self> {
        if value.is_empty()
            || value.len() > 64
            || value.starts_with('-')
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(invalid());
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Deliberately no Drop/Unused/Force command: deletion requires the admitted
/// control-plane bridge, not a boolean or an arbitrary extra argument.
pub enum Command<'a> {
    Version,
    Info(Option<&'a Remote>),
    CheckPresent {
        key: &'a Key,
        remote: &'a Remote,
    },
    RegisterPresent {
        key: &'a Key,
        uuid: &'a Uuid,
    },
    Get {
        key: &'a Key,
        from: &'a Remote,
    },
    Copy {
        key: &'a Key,
        from: Option<&'a Remote>,
        to: &'a Remote,
    },
    ContentLocation(&'a Key),
    ContentLocationBatch(&'a Key),
}
impl Command<'_> {
    fn argv(&self) -> Vec<OsString> {
        let s = |v: &str| OsString::from(v);
        match self {
            Self::Version => vec![s("version")],
            Self::Info(remote) => {
                let mut out = vec![s("info"), s("--json")];
                if let Some(remote) = remote {
                    out.push(s(remote.as_str()));
                } else {
                    out.push(s("here"));
                }
                out
            }
            Self::CheckPresent { key, remote } => {
                vec![s("checkpresentkey"), s(key.as_str()), s(remote.as_str())]
            }
            Self::RegisterPresent { key, uuid } => {
                vec![s("setpresentkey"), s(key.as_str()), s(&uuid.0), s("1")]
            }
            Self::Get { key, from } => vec![
                s("get"),
                s("--json"),
                s(&format!("--from={}", from.0)),
                s(&format!("--key={}", key.as_str())),
            ],
            Self::Copy { key, from, to } => {
                let mut out = vec![
                    s("copy"),
                    s("--json"),
                    s(&format!("--to={}", to.0)),
                    s(&format!("--key={}", key.as_str())),
                ];
                if let Some(from) = from {
                    out.push(s(&format!("--from={}", from.0)));
                }
                out
            }
            Self::ContentLocation(key) => vec![s("contentlocation"), s(key.as_str())],
            Self::ContentLocationBatch(_) => vec![s("contentlocation"), s("--batch")],
        }
    }
}

pub struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub struct Runner {
    enrollment: Enrollment,
}
fn invalid() -> io::Error {
    io::Error::other("invalid annex enrollment or command")
}
impl Runner {
    pub fn control(&self) -> Option<&ControlBinding> {
        self.enrollment.control.as_ref()
    }
    pub fn repository(&self) -> &std::path::Path {
        &self.enrollment.repository
    }
    pub fn new(enrollment: Enrollment) -> io::Result<Self> {
        let e = &enrollment;
        if !e.git.is_absolute()
            || !e.git.is_file()
            || !e.executable.is_absolute()
            || !e.executable.is_file()
            || !e.repository.is_absolute()
            || !e.repository.is_dir()
            || !e.empty_config.is_absolute()
            || std::fs::metadata(&e.empty_config)?.len() != 0
            || !e.empty_hooks.is_absolute()
            || !e.empty_hooks.is_dir()
            || std::fs::read_dir(&e.empty_hooks)?.next().is_some()
            || !e.temporary.is_absolute()
            || !e.temporary.is_dir()
            || e.tools.is_empty()
            || e.tools.len() > 8
            || e.tools.iter().any(|p| !p.is_absolute() || !p.is_dir())
            || e.legacy_profile
                .as_ref()
                .is_some_and(|p| !p.is_absolute() || !p.is_file())
            || !(1..=3_600_000).contains(&e.command_millis)
            || !(1024..=8_388_608).contains(&e.output_bytes)
            || e.control.as_ref().is_some_and(|c| !c.root.is_absolute())
        {
            return Err(invalid());
        }
        Ok(Self { enrollment })
    }
    pub fn run(&self, command: Command<'_>) -> io::Result<Output> {
        let input = match &command {
            Command::ContentLocationBatch(key) => Some(format!("{}\n", key.as_str()).into_bytes()),
            _ => None,
        };
        self.run_argv(command.argv(), input)
    }
    // Caller creates this alias itself after exact-source verification. Never
    // pass an original cache path: reinject consumes its staging input.
    pub(crate) fn reinject_alias(&self, path: &std::path::Path, key: &Key) -> io::Result<Output> {
        if !path.is_absolute()
            || !path.starts_with(&self.enrollment.repository)
            || path.file_name().and_then(|s| s.to_str()) != Some(key.as_str())
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(invalid());
        }
        self.run_argv(
            vec![
                "reinject".into(),
                "--json".into(),
                "--guesskeys".into(),
                path.as_os_str().to_owned(),
            ],
            None,
        )
    }
    pub(crate) fn remove(
        &self,
        key: &Key,
        final_loss: bool,
        worker: &mut super::worker::Worker,
        index: &tig_core::Digest,
        request: &tig_store::source_claims::protection::effects::Request,
    ) -> io::Result<Output> {
        worker.admit(index, request)?;
        if request.object != key.object()
            || self
                .control()
                .is_none_or(|c| c.index != *index || c.store != request.store)
        {
            return Err(invalid());
        }
        let mut argv = vec![
            OsString::from("drop"),
            OsString::from("--json"),
            OsString::from(format!("--key={}", key.as_str())),
        ];
        if final_loss {
            argv.push(OsString::from("--force"));
        }
        self.run_argv(argv, None)
    }
    pub(crate) fn control_command(
        &self,
        executable: &std::path::Path,
        root: &std::path::Path,
        command: ControlCommand,
        profile: &std::path::Path,
        millis: u64,
    ) -> io::Result<Output> {
        if executable.canonicalize()? != std::env::current_exe()?.canonicalize()?
            || !profile.is_absolute()
        {
            return Err(invalid());
        }
        let mut e = self.enrollment.clone();
        e.executable = executable.to_owned();
        e.repository = root.to_owned();
        e.control = None;
        e.legacy_profile = None;
        e.command_millis = millis;
        Self::new(e)?.run_argv(
            vec![command.name().into(), profile.as_os_str().to_owned()],
            None,
        )
    }
    fn run_argv(&self, argv: Vec<OsString>, input: Option<Vec<u8>>) -> io::Result<Output> {
        // Supported managed process containment is Windows-only. Do not silently
        // downgrade to Unix parent-only kill semantics.
        if !cfg!(windows) {
            return Err(io::Error::from(io::ErrorKind::Unsupported));
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(self.execute(argv, input))
    }
    async fn execute(&self, argv: Vec<OsString>, input: Option<Vec<u8>>) -> io::Result<Output> {
        let e = &self.enrollment;
        let path = std::env::join_paths(&e.tools).map_err(|_| invalid())?;
        let mut wrap = CommandWrap::with_new(&e.executable, |c| {
            c.args(argv)
                .current_dir(&e.repository)
                .env_clear()
                .env("PATH", path)
                .env("TEMP", &e.temporary)
                .env("TMP", &e.temporary)
                .env("USERPROFILE", &e.temporary)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", &e.empty_config)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_ALLOW_PROTOCOL", "file")
                .env("GIT_CONFIG_COUNT", "3")
                .env("GIT_CONFIG_KEY_0", "core.hooksPath")
                .env("GIT_CONFIG_VALUE_0", &e.empty_hooks)
                .env("GIT_CONFIG_KEY_1", "core.fsmonitor")
                .env("GIT_CONFIG_VALUE_1", "false")
                .env("GIT_CONFIG_KEY_2", "credential.helper")
                .env("GIT_CONFIG_VALUE_2", "")
                .stdin(if input.is_some() {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if let Some(value) = std::env::var_os("SystemRoot") {
                c.env("SystemRoot", value);
            }
            if let Some(profile) = &e.legacy_profile {
                c.env("TIG_ANNEX_CACHE_PROFILE", profile);
            }
        });
        wrap.wrap(KillOnDrop);
        #[cfg(windows)]
        {
            use process_wrap::tokio::{CreationFlags, JobObject};
            let mut flags = CreationFlags(Default::default());
            flags.0.0 = 0x08000000; // CREATE_NO_WINDOW, composed with suspension.
            wrap.wrap(flags).wrap(JobObject);
        }
        let mut child = wrap.spawn()?;
        let stdout = child.stdout().take().ok_or_else(invalid)?;
        let stderr = child.stderr().take().ok_or_else(invalid)?;
        let stdin = child.stdin().take();
        let result = tokio::time::timeout(Duration::from_millis(e.command_millis), async {
            if let Some(input) = input {
                let mut stdin = stdin.ok_or_else(invalid)?;
                stdin.write_all(&input).await?;
                stdin.shutdown().await?;
            }
            tokio::try_join!(
                capture(stdout, e.output_bytes),
                capture(stderr, e.output_bytes),
                child.wait()
            )
        })
        .await;
        match result {
            Ok(Ok((stdout, stderr, status))) => Ok(Output {
                success: status.success(),
                stdout,
                stderr,
            }),
            failure => {
                // Kill the entire Windows job; retain the handle until the
                // bounded termination wait finishes, then kill-on-close applies.
                child.start_kill()?;
                let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
                match failure {
                    Ok(Err(error)) => Err(error),
                    _ => Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "annex outcome uncertain",
                    )),
                }
            }
        }
    }
}
async fn capture(reader: impl AsyncRead + Unpin, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(io::Error::other("annex output budget exceeded"));
    }
    Ok(bytes)
}

pub(crate) enum ControlCommand {
    StagePolicy,
    Retire,
    Reconcile,
    Cancel,
}
impl ControlCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::StagePolicy => "stage-policy",
            Self::Retire => "retire",
            Self::Reconcile => "reconcile",
            Self::Cancel => "cancel",
        }
    }
}

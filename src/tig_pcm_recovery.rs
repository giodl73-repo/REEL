//! Exact-identity PCM recovery through an existing byte reader/publisher.
//! No movie render, cache discovery, source selection or retirement authority.
use crate::{
    media_runtime::{Bindings, BoundPrograms},
    scene_delivery::{InputFiles, MediaProgram},
    tig_publication::{ExistingPublisher, Profile as ProviderProfile},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    thread::{self, JoinHandle},
};
use tig_core::{Digest, Object, identity};
use tig_store::{
    Key, Request,
    existing::{self, ByteSource, ReadCache},
};

const SCHEMA: &str = "reel.tig-pcm-wave-recovery.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    byte_source: existing::Profile,
    carrier: Object,
    target: Object,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    #[serde(default)]
    audio_stream: u16,
    max_output_bytes: u64,
    runtime: Bindings,
    timeout_seconds: u64,
    publication: Option<Publication>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Publication {
    artifact_root: PathBuf,
    namespace: Key,
    storage: reel_assets::Config,
    destination: PathBuf,
    reader_limits: existing::Limits,
    protection: existing::RequiredProtection,
    request: Request,
}
impl Publication {
    fn open(&self) -> Result<ExistingPublisher> {
        Ok(ExistingPublisher::open(ProviderProfile {
            artifact_root: self.artifact_root.clone(),
            namespace: self.namespace.clone(),
            storage: self.storage.clone(),
            destination: self.destination.clone(),
            reader_limits: self.reader_limits,
            protection: self.protection,
        })?)
    }
}
fn header(profile: &Profile) -> Result<Vec<u8>> {
    let data = profile
        .target
        .bytes
        .checked_sub(44)
        .context("target lacks canonical WAV header")?;
    if profile.schema != SCHEMA
        || !(8000..=192000).contains(&profile.sample_rate)
        || !(1..=8).contains(&profile.channels)
        || ![16, 24, 32].contains(&profile.bits_per_sample)
        || data == 0
        || data > u64::from(u32::MAX) - 36
        || profile.target.bytes > profile.max_output_bytes
        || profile.max_output_bytes > u64::from(u32::MAX)
        || profile.audio_stream > 31
        || !(1..=3600).contains(&profile.timeout_seconds)
    {
        bail!("invalid bounded PCM recovery shape or budget");
    }
    let align = profile
        .channels
        .checked_mul(profile.bits_per_sample / 8)
        .context("PCM block alignment overflow")?;
    if data % u64::from(align) != 0 {
        bail!("PCM target is not a whole sample frame");
    }
    let mut raw = Vec::with_capacity(44);
    raw.extend(b"RIFF");
    raw.extend((data as u32 + 36).to_le_bytes());
    raw.extend(b"WAVEfmt ");
    raw.extend(16u32.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(profile.channels.to_le_bytes());
    raw.extend(profile.sample_rate.to_le_bytes());
    raw.extend((profile.sample_rate * u32::from(align)).to_le_bytes());
    raw.extend(align.to_le_bytes());
    raw.extend(profile.bits_per_sample.to_le_bytes());
    raw.extend(b"data");
    raw.extend((data as u32).to_le_bytes());
    Ok(raw)
}

/// Reader owns the decoder. EOF means exact declared length AND successful exit.
/// A publisher cannot acknowledge a truncated, overlong or failed decode.
struct DecodedWave {
    header: Cursor<Vec<u8>>,
    process: std::sync::Arc<std::sync::Mutex<Child>>,
    stdout: ChildStdout,
    stderr: Option<JoinHandle<io::Result<bool>>>,
    remaining: u64,
    finished: bool,
    emitted: u64,
    expected: Object,
    hash: Sha256,
    stop: Option<std::sync::mpsc::Sender<()>>,
    watchdog: Option<JoinHandle<()>>,
}
impl DecodedWave {
    fn start(
        profile: &Profile,
        header: Vec<u8>,
        path: &Path,
        programs: &BoundPrograms,
    ) -> Result<Self> {
        let local = PathBuf::from(".");
        let tools = programs.with_inputs(&local);
        let format = format!("s{}le", profile.bits_per_sample);
        let codec = format!("pcm_{format}");
        let mut child = Command::new(tools.media_program(MediaProgram::Ffmpeg)?)
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(path)
            .args([
                "-map",
                &format!("0:a:{}", profile.audio_stream),
                "-vn",
                "-c:a",
                &codec,
                "-ar",
                &profile.sample_rate.to_string(),
                "-ac",
                &profile.channels.to_string(),
                "-f",
                &format,
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().context("decoder stdout missing")?;
        let mut stderr = child.stderr.take().context("decoder stderr missing")?;
        let drain = thread::spawn(move || {
            let mut buffer = [0; 4096];
            let mut total = 0usize;
            loop {
                let n = stderr.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                total = total.saturating_add(n);
            }
            Ok(total <= 65536)
        });
        let child = std::sync::Arc::new(std::sync::Mutex::new(child));
        let (stop, watchdog) = watch(
            child.clone(),
            std::time::Duration::from_secs(profile.timeout_seconds),
        );
        Ok(Self {
            header: Cursor::new(header),
            process: child,
            stdout,
            stderr: Some(drain),
            remaining: profile.target.bytes - 44,
            finished: false,
            emitted: 0,
            expected: profile.target.clone(),
            hash: Sha256::new(),
            stop: Some(stop),
            watchdog: Some(watchdog),
        })
    }
}
fn watch(
    child: std::sync::Arc<std::sync::Mutex<Child>>,
    timeout: std::time::Duration,
) -> (std::sync::mpsc::Sender<()>, JoinHandle<()>) {
    let (stop, cancelled) = std::sync::mpsc::channel();
    let task = thread::spawn(move || {
        if matches!(
            cancelled.recv_timeout(timeout),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) && let Ok(mut child) = child.lock()
        {
            let _ = child.kill();
        }
    });
    (stop, task)
}

impl Read for DecodedWave {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let n = self.header.read(bytes)?;
        if n != 0 {
            self.hash.update(&bytes[..n]);
            self.emitted += n as u64;
            return Ok(n);
        }
        if self.finished {
            return Ok(0);
        }
        let maximum = bytes
            .len()
            .min(self.remaining.saturating_add(1).min(usize::MAX as u64) as usize);
        let n = self.stdout.read(&mut bytes[..maximum])?;
        if n as u64 > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PCM recovery exceeds declared target",
            ));
        }
        self.remaining -= n as u64;
        self.hash.update(&bytes[..n]);
        self.emitted += n as u64;
        if n == 0 {
            let status = loop {
                let status = self
                    .process
                    .lock()
                    .map_err(|_| io::Error::other("decoder owner poisoned"))?
                    .try_wait()?;
                if let Some(status) = status {
                    break status;
                }
                thread::sleep(std::time::Duration::from_millis(10));
            };
            let bounded = self
                .stderr
                .take()
                .expect("decoder stderr owner")
                .join()
                .map_err(|_| io::Error::other("decoder stderr task failed"))??;
            self.finished = true;
            if !status.success() || !bounded || self.remaining != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "PCM recovery decode or length differs",
                ));
            }
            if self.emitted != self.expected.bytes
                || Digest::from_sha256(self.hash.clone().finalize().into()) != self.expected.sha256
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "decoded recovery differs from declared target",
                ));
            }
        }
        Ok(n)
    }
}
impl Drop for DecodedWave {
    fn drop(&mut self) {
        if !self.finished {
            if let Ok(mut process) = self.process.lock() {
                let _ = process.kill();
                let _ = process.wait();
            }
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
    }
}
fn verify(reader: &mut impl Read, expected: &Object) -> Result<()> {
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0; 65536];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .filter(|n| *n <= expected.bytes)
            .context("recovery byte budget exceeded")?;
        hash.update(&buffer[..n]);
    }
    let actual = Digest::from_sha256(hash.finalize().into());
    if actual != expected.sha256 || count != expected.bytes {
        bail!("recovered bytes differ from independently declared target");
    }
    Ok(())
}

pub fn run(path: &Path, publish: bool) -> Result<Value> {
    let mut raw = Vec::new();
    fs::File::open(path)?.take(65537).read_to_end(&mut raw)?;
    if raw.len() > 65536 {
        bail!("recovery profile exceeds budget");
    }
    let profile: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    let header = header(&profile)?;
    let programs = BoundPrograms::bind(&profile.runtime)?;
    let publication = if publish {
        Some(
            profile
                .publication
                .as_ref()
                .context("explicit existing publication admission required")?,
        )
    } else {
        None
    };
    if let Some(publication) = publication {
        if publication.request.object != profile.target {
            bail!("publication target differs from recovery target");
        }
        let mut publisher = publication.open()?;
        if tig_store::recover(&mut publisher, &publication.request)?.is_some() {
            return Ok(
                json!({"schema":"reel.tig-pcm-wave-recovery-result.experimental-1","target":profile.target,"verified_target":true,"published":true,"reused_verified_object":true,"decoded_this_run":false,"payload_files_written_outside_publisher":0,"approval":false,"original_producer_assertion":false,"independent_custody_verified":false,"authorizes_eviction":false}),
            );
        }
        let mut destination = ReadCache::new(existing::Profile {
            places: vec![existing::Place {
                root: publication.destination.clone(),
                layout: existing::Layout::ReelCas,
            }],
            required: publication.protection,
            limits: publication.reader_limits,
        })?;
        match destination.open_verified(&profile.target) {
            Ok(mut existing) => {
                tig_store::publish(&mut publisher, &publication.request, &mut existing)?;
                drop(publisher);
                let mut reopened = publication.open()?;
                tig_store::recover(&mut reopened, &publication.request)?
                    .context("existing target unavailable after publisher reopen")?;
                return Ok(
                    json!({"schema":"reel.tig-pcm-wave-recovery-result.experimental-1","target":profile.target,"verified_target":true,"published":true,"reused_verified_object":true,"decoded_this_run":false,"approval":false,"original_producer_assertion":false,"independent_custody_verified":false,"authorizes_eviction":false}),
                );
            }
            Err(existing::Error::Missing) => (),
            Err(error) => return Err(error.into()),
        }
    }
    let mut cache = ReadCache::new(profile.byte_source.clone())?;
    let carrier = cache.open_verified(&profile.carrier)?;
    let carrier_path = carrier.with_path(Path::to_path_buf)?;
    let mut decoder = DecodedWave::start(&profile, header, &carrier_path, &programs)?;
    let mut decoded = true;
    if let Some(publication) = publication {
        let mut publisher = publication.open()?;
        tig_store::publish(&mut publisher, &publication.request, &mut decoder)?;
        // Require decoder EOF/status even if a provider stops after declared bytes.
        decoded = decoder.emitted != 0;
        if decoded {
            io::copy(&mut decoder, &mut io::sink())?;
        }
        drop(publisher);
        let mut reopened = publication.open()?;
        tig_store::recover(&mut reopened, &publication.request)?
            .context("recovered target unavailable after publisher reopen")?;
    } else {
        verify(&mut decoder, &profile.target)?;
    }
    Ok(
        json!({"schema":"reel.tig-pcm-wave-recovery-result.experimental-1","carrier":profile.carrier,"target":profile.target,"verified_target":true,"published":publish,"reused_verified_object":!decoded,"decoded_this_run":decoded,"payload_files_written_outside_publisher":0,"approval":false,"original_producer_assertion":false,"independent_custody_verified":false,"authorizes_eviction":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watchdog_terminates_only_its_owned_child() {
        #[cfg(windows)]
        let mut command = {
            use std::os::windows::process::CommandExt;
            let mut command = Command::new("powershell.exe");
            command
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Start-Sleep -Seconds 30",
                ])
                .creation_flags(0x08000000);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("/bin/sleep");
            command.arg("30");
            command
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let owned = std::sync::Arc::new(std::sync::Mutex::new(child));
        let (stop, task) = watch(owned.clone(), std::time::Duration::from_millis(100));
        let began = std::time::Instant::now();
        loop {
            if let Some(status) = owned.lock().unwrap().try_wait().unwrap() {
                assert!(!status.success());
                break;
            }
            if began.elapsed() > std::time::Duration::from_secs(5) {
                owned.lock().unwrap().kill().unwrap();
                panic!("owned decoder exceeded watchdog deadline");
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        drop(stop);
        task.join().unwrap();
    }
}

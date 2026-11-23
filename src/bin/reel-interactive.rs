//! Explicit local Windows demo for a checked bilingual programme.
#[cfg(windows)]
mod windows_demo {
    use anyhow::{Context, Result, ensure};
    use clap::Parser;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use reel::spoken_programme::{
        checker::{FileCheckRequest, check_file_package},
        compiler::CompilerIdentity,
        interactive::demo::{self, Demo},
    };
    use std::{
        fs::File,
        io::{self, BufRead, Read, Write},
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc::{self, TryRecvError},
        },
        thread,
        time::{Duration, Instant},
    };

    #[derive(Parser)]
    struct Args {
        #[arg(long)]
        demo: PathBuf,
        #[arg(long)]
        declaration: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long)]
        processing: Option<PathBuf>,
        #[arg(long)]
        source_root: PathBuf,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long)]
        access: PathBuf,
        #[arg(long)]
        compiler_producer: PathBuf,
        #[arg(long, required_unless_present = "check", conflicts_with = "check")]
        endpoint: Option<String>,
        #[arg(long)]
        check: bool,
        #[arg(long, value_parser = ["en", "es"])]
        language: String,
    }

    struct Sink {
        _stream: cpal::Stream,
        queue: rtrb::Producer<[f32; 2]>,
        consumed: Arc<AtomicU64>,
        failed: Arc<AtomicBool>,
        submitted: u64,
    }

    impl Sink {
        fn open(endpoint: &str) -> Result<Self> {
            let id: cpal::DeviceId = endpoint.parse().context("invalid output endpoint ID")?;
            ensure!(
                id.host() == cpal::HostId::Wasapi,
                "requires a WASAPI output"
            );
            let host = cpal::host_from_id(cpal::HostId::Wasapi)?;
            let device = host
                .device_by_id(&id)
                .context("output endpoint unavailable")?;
            let (queue, mut input) = rtrb::RingBuffer::new(4096);
            let consumed = Arc::new(AtomicU64::new(0));
            let failed = Arc::new(AtomicBool::new(false));
            let count = consumed.clone();
            let fault = failed.clone();
            let stream = device.build_output_stream::<f32, _, _>(
                cpal::StreamConfig {
                    channels: 2,
                    sample_rate: 48_000,
                    buffer_size: cpal::BufferSize::Fixed(480),
                },
                move |output, _| {
                    let mut used = 0;
                    for frame in output.chunks_exact_mut(2) {
                        let samples = input.pop().map_or([0.0, 0.0], |value| {
                            used += 1;
                            value
                        });
                        frame.copy_from_slice(&samples);
                    }
                    count.fetch_add(used, Ordering::Release);
                },
                move |_| fault.store(true, Ordering::Release),
                Some(Duration::from_secs(2)),
            )?;
            stream.play()?;
            Ok(Self {
                _stream: stream,
                queue,
                consumed,
                failed,
                submitted: 0,
            })
        }

        fn submit(&mut self, pcm: &[u8]) -> Result<()> {
            ensure!(!self.failed.load(Ordering::Acquire), "audio output failed");
            ensure!(pcm.len() % 6 == 0, "unaligned stereo PCM");
            ensure!(self.queue.slots() >= pcm.len() / 6, "audio buffer is full");
            for frame in pcm.chunks_exact(6) {
                let convert = |sample: &[u8]| {
                    let sign = if sample[2] & 128 == 0 { 0 } else { 255 };
                    i32::from_le_bytes([sample[0], sample[1], sample[2], sign]) as f32 / 8_388_608.0
                };
                self.queue
                    .push([convert(&frame[..3]), convert(&frame[3..])])
                    .map_err(|_| anyhow::anyhow!("audio buffer is full"))?;
                self.submitted += 1;
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.consumed.load(Ordering::Acquire) != self.submitted {
                ensure!(!self.failed.load(Ordering::Acquire), "audio output failed");
                ensure!(Instant::now() < deadline, "audio output timed out");
                thread::sleep(Duration::from_millis(1));
            }
            Ok(())
        }
    }

    fn plain(text: &str) -> String {
        text.chars().filter(|ch| !ch.is_control()).collect()
    }

    pub fn run() -> Result<()> {
        let args = Args::parse();
        let mut bytes = Vec::new();
        File::open(&args.demo)?
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 256 * 1024, "demo manifest is too large");
        let demo: Demo = serde_json::from_slice(&bytes)?;
        ensure!(
            args.compiler_producer.is_absolute(),
            "compiler/producer path must be absolute"
        );
        let identity = reel::media_runtime::ByteIdentity::observe_file(&args.compiler_producer)?;
        let runtime = CompilerIdentity {
            sha256: identity.sha256,
            bytes: identity.bytes,
        };
        let package = check_file_package(
            FileCheckRequest {
                declaration: &args.declaration,
                plan: &args.plan,
                processing: args.processing.as_deref(),
                source_root: &args.source_root,
                candidate: &args.candidate,
                access: Some(&args.access),
                write_access: None,
            },
            &runtime,
        )?;
        let mut session = demo.start(&package, &args.language)?;
        if args.check {
            println!("Checked the authored bilingual demo");
            return Ok(());
        }
        let mut sink = Sink::open(
            args.endpoint
                .as_deref()
                .context("output endpoint required")?,
        )?;
        let visit = Arc::new(AtomicU64::new(session.visit()));
        let input_visit = visit.clone();
        let (send, receive) = mpsc::sync_channel(16);
        thread::spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if send
                    .send((input_visit.load(Ordering::Acquire), line))
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut shown = 0;
        let mut out = io::stdout().lock();
        writeln!(
            out,
            "Commands: next, previous, activate, choose ID, pause, resume, seek FRAME, language en|es, close"
        )?;
        while let Some(node) = session.node() {
            if shown != session.visit() {
                shown = session.visit();
                visit.store(shown, Ordering::Release);
                writeln!(out, "{}", plain(node))?;
                for entry in demo::transcript(&session, &package)? {
                    writeln!(out, "{}", plain(&entry.text))?;
                }
                for (id, label) in session.choices() {
                    writeln!(out, "{}  {}", plain(id), plain(label))?;
                }
                if session.is_ended() {
                    writeln!(
                        out,
                        "{}",
                        plain(demo.ending(session.language()).unwrap_or(""))
                    )?;
                }
                out.flush()?;
            }
            let playing = session.selection().is_some() && !session.is_paused();
            let input = if playing {
                match receive.try_recv() {
                    Ok(message) => Some(message),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => break,
                }
            } else {
                match receive.recv() {
                    Ok(message) => Some(message),
                    Err(_) => break,
                }
            };
            if let Some((target, line)) = input {
                match demo::command(&mut session, target, &line) {
                    Ok(true) => {
                        if let Some((id, label)) = session.focused_choice() {
                            writeln!(out, "> {}  {}", plain(id), plain(label))?;
                        }
                    }
                    Ok(false) => writeln!(out, "Command unavailable")?,
                    Err(_) => writeln!(out, "Invalid command")?,
                }
                out.flush()?;
            } else {
                session.deliver(&package, &args.candidate, 480, |pcm| sink.submit(pcm))?;
            }
        }
        session.close();
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    windows_demo::run()
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("interactive audio demo requires Windows")
}

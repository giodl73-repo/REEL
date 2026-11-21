//! Explicit owned-service qualification driver. Never accepts a destination path.
use serde::Deserialize;
use std::{fs, io::Read, path::PathBuf};
use tig_core::{Digest, Object};
use tig_store::{
    AckLevel, Key, Request, accounted, acquire,
    ledger::CapacityLedger,
    local::LocalStore,
    provider::{
        self, ReadUse,
        s3_read::{Config, Limits},
    },
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    object: Object,
    version: String,
    head_url: String,
    get_url: String,
}
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("usage: s3_read_probe OWNED_LOOPBACK_PROFILE".into());
    }
    let path = PathBuf::from(&args[0]);
    if !path.is_absolute() {
        return Err("profile path must be absolute".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err("profile budget".into());
    }
    let input: Input = serde_json::from_slice(&bytes)?;
    let place = Digest::blob(b"owned S3 fixture");
    let mut remote = Config::loopback_http(
        place.clone(),
        input.object.clone(),
        &input.version,
        &input.head_url,
        &input.get_url,
        Limits {
            max_requests: 2,
            max_total_bytes: input.object.bytes.max(1),
            ..Limits::default()
        },
    )?
    .connect()?;
    let temp = tempfile::tempdir()?;
    let mut store = LocalStore::create(&temp.path().join("M"), key("M"))?;
    let request = Request {
        operation: key("s3-copy"),
        owner: key("public-fixture"),
        object: input.object,
        required: AckLevel::ProcessRestart,
    };
    let plan = accounted::plan(&store, &request)?;
    let mut ledger =
        CapacityLedger::create(&temp.path().join("ledger"), key("volume"), key("epoch"), 0)?;
    let probe = ledger.begin_probe(key("probe"))?;
    ledger.observe(key("sample"), probe.reference(), 1 << 30)?;
    let job = ledger
        .reserve(key("job"), key("public-fixture"), plan.required_peak())?
        .reference()
        .clone();
    let mut cold = None;
    for warm in [false, true] {
        let mut destination =
            provider::publication::native::Accounted::new(&store, &mut ledger, &job, &plan);
        acquire::publish_placed(
            &mut provider::Source::new(&mut remote, place.clone(), ReadUse::Transfer),
            &mut destination,
            store.identity(),
            &request,
        )?;
        if warm {
            assert_eq!(cold.as_ref(), Some(&ledger.checkpoint()))
        } else {
            cold = Some(ledger.checkpoint());
            store = LocalStore::open(&temp.path().join("M"))?;
            ledger = CapacityLedger::open(&temp.path().join("ledger"), cold.as_ref().unwrap())?;
        }
    }
    let stats = remote.stats()?;
    assert_eq!(stats.requests_charged, 2);
    assert_eq!(stats.active_streams, 0);
    assert_eq!(stats.transfer_bytes_charged, request.object.bytes);
    println!(
        "Verified exact S3 version into native M; 2 requests, {} payload bytes; reopened replay uses zero requests/events. No custody/removal claim.",
        request.object.bytes
    );
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("Owned S3 qualification failed; private binding details suppressed");
        std::process::exit(2)
    }
}

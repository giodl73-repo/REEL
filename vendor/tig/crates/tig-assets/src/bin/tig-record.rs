//! Explicit, read-only native envelope inspection. No cache/provider attachment.
#![forbid(unsafe_code)]
use serde_json::json;
use std::{
    env,
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
use tig_assets::native::{Instance, MAX_RECORD_BYTES};
use tig_core::identity::{Error, Result};

fn read(path: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| Error("cannot open explicit record input".into()))?
        .take((MAX_RECORD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error("cannot read explicit record input".into()))?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(Error("record input exceeds byte budget".into()));
    }
    Ok(bytes)
}

fn run(args: &[String]) -> Result<()> {
    if !matches!(args.first().map(String::as_str), Some("inspect" | "stream"))
        || (args[0] == "inspect" && args.len() != 4)
        || (args[0] == "stream" && args.len() != 5)
    {
        return Err(Error("usage: tig-record inspect INSTANCE DEFINITION SCHEMA | stream INSTANCE DEFINITION SCHEMA SLOT".into()));
    }
    let instance = Instance::parse(&read(&args[1])?)?;
    let bound = instance.bind(&read(&args[2])?, &read(&args[3])?)?;
    let output = if args[0] == "stream" {
        let stream = bound
            .instance()
            .value()
            .streams
            .iter()
            .find(|s| s.slot == args[4])
            .ok_or_else(|| Error("stream slot not found".into()))?;
        json!({"schema":"tig.record-stream.experimental-1", "object": stream.object,
            "complete_semantic_coverage": false, "payload_verified": false})
    } else {
        json!({"schema":"tig.record-inspection.experimental-1",
            "instance": bound.instance().object(), "definition": bound.definition().object(),
            "stream_count": bound.instance().value().streams.len(),
            "has_selected": bound.instance().value().selected.is_some(),
            "complete_semantic_coverage": bound.complete_semantic_coverage()})
    };
    writeln!(std::io::stdout().lock(), "{output}")
        .map_err(|_| Error("cannot write record result".into()))?;
    Ok(())
}

fn main() -> ExitCode {
    let args = env::args_os()
        .skip(1)
        .take(6)
        .map(|s| {
            s.into_string()
                .map_err(|_| Error("record arguments must be Unicode".into()))
        })
        .collect::<Result<Vec<_>>>();
    match args.and_then(|args| run(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

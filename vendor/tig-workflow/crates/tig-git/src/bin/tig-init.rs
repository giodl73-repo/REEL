//! Explicit native bootstrap. Never run on repository open or resume.
#![forbid(unsafe_code)]
use std::{
    env,
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
use tig_store::local_enrollment::{self, Request};
fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let mode = args.next().ok_or("usage: tig-init --apply PROFILE")?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(),"usage: tig-init --apply PROFILE\nCreate a native store and create or attach its explicit capacity authority; emit bindings.").map_err(|_|"cannot write help")?;
        return Ok(());
    }
    if mode != "--apply" {
        return Err("usage: tig-init --apply PROFILE".into());
    }
    let path = args.next().ok_or("usage: tig-init --apply PROFILE")?;
    if args.next().is_some() {
        return Err("usage: tig-init --apply PROFILE".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open enrollment profile")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read enrollment profile")?;
    if bytes.len() > 65_536 {
        return Err("enrollment profile exceeds metadata budget".into());
    }
    let request: Request =
        serde_json::from_slice(&bytes).map_err(|_| "invalid enrollment profile")?;
    let enrollment = local_enrollment::initialize(&request).map_err(|error| {
        format!("native enrollment {error}; metadata may exist; retry or reconcile exact input")
    })?;
    let output = tig_core::identity::canonical_json(
        &serde_json::to_value(enrollment).map_err(|_| "cannot encode enrollment")?,
    )
    .map_err(|_| "cannot encode enrollment")?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write enrollment result; initialization may be complete".into())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

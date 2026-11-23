//! Execute one exact already-admitted native removal attempt, never infer consent.
#![forbid(unsafe_code)]
#[path = "tig-remove/execution.rs"]
mod execution;
#[path = "tig-remove/profile.rs"]
mod profile;
use std::{
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
fn run() -> Result<u8, String> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or("usage: tig-remove --apply PROFILE")?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(), "usage: tig-remove --apply PROFILE\nExecute one saved admitted native removal; no consent, claim release, new admission or cache setup.").map_err(|_| "cannot write help")?;
        return Ok(0);
    }
    if mode != "--apply" {
        return Err("usage: tig-remove --apply PROFILE".into());
    }
    let path = args.next().ok_or("explicit removal profile required")?;
    if args.next().is_some() {
        return Err("unexpected removal arguments".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open removal profile")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read removal profile")?;
    let (out, code) = execution::execute(profile::parse(&bytes)?)?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&out)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write removal result; effects may exist")?;
    Ok(code)
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("tig-remove: {e}; effects may exist; reconcile exact work");
            ExitCode::from(2)
        }
    }
}

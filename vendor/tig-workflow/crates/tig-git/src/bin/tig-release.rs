//! Execute one exact admitted native claim release; never remove payload bytes.
#![forbid(unsafe_code)]
#[path = "tig-release/execution.rs"]
mod execution;
#[path = "tig-release/profile.rs"]
mod profile;
use std::{
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
fn run() -> Result<u8, String> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or("usage: tig-release --apply PROFILE")?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(),"usage: tig-release --apply PROFILE\nExecute exact admitted native claim retirement or custody handoff; no payload removal or new admission.").map_err(|_|"cannot write help")?;
        return Ok(0);
    }
    if mode != "--apply" {
        return Err("usage: tig-release --apply PROFILE".into());
    }
    let path = args.next().ok_or("explicit claim-work profile required")?;
    if args.next().is_some() {
        return Err("unexpected claim-work arguments".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open claim-work profile")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read claim-work profile")?;
    let (out, code) = execution::execute(profile::parse(&bytes)?)?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&out)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write claim-work result; effects may exist")?;
    Ok(code)
}
fn main() -> ExitCode {
    match run() {
        Ok(n) => ExitCode::from(n),
        Err(e) => {
            eprintln!("tig-release: {e}; effects may exist; reconcile exact work");
            ExitCode::from(2)
        }
    }
}

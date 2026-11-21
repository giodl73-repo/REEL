//! Exact Git placement inspection only; no provider attachment or mutation.
#![forbid(unsafe_code)]
#[path = "tig-plan/profile.rs"]
mod profile;
#[path = "tig-plan/report.rs"]
mod report;
use std::{
    env,
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};

fn run() -> Result<u8, &'static str> {
    let mut args = env::args_os().skip(1);
    let path = args.next().ok_or("usage: tig-plan PROFILE")?;
    if args.next().is_some() {
        return Err("usage: tig-plan PROFILE");
    }
    if path == "--help" {
        writeln!(
            std::io::stdout().lock(),
            "usage: tig-plan PROFILE\nInspect exact Git placement; no transfer or deletion."
        )
        .map_err(|_| "cannot write help")?;
        return Ok(0);
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open explicit profile")?
        .take(profile::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read explicit profile")?;
    let profile = profile::parse(&bytes)?;
    let (output, complete) = report::inspect(profile)?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write inspection")?;
    Ok(if complete { 0 } else { 3 })
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

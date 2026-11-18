//! Explicit source/protection adoption; no payload placement or deletion.
#![forbid(unsafe_code)]
#[path = "tig-source/command.rs"]
mod command;
#[path = "tig-source/init_profile.rs"]
mod init_profile;
#[path = "tig-source/initialize.rs"]
mod initialize;
#[path = "tig-source/profile.rs"]
mod profile;
use std::{
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
fn run() -> Result<u8, String> {
    let mut args = std::env::args_os().skip(1);
    let mode = args
        .next()
        .ok_or("usage: tig-source --init PROFILE | --stage PROFILE | --apply-work WORK")?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(),"usage: tig-source --init PROFILE | --stage PROFILE | --apply-work WORK\nExplicit source-authority bootstrap, exact Git staging and saved adoption. No payload placement/removal.").map_err(|_|"cannot write help")?;
        return Ok(0);
    }
    let path = args.next().ok_or("explicit source profile/work required")?;
    if args.next().is_some() {
        return Err("unexpected source arguments".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open source input")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read source input")?;
    let (out, code) = if mode == "--init" {
        (initialize::initialize(init_profile::parse(&bytes)?)?, 0)
    } else if mode == "--stage" {
        (command::stage(profile::parse(&bytes)?)?, 0)
    } else if mode == "--apply-work" {
        command::apply(command::parse_work(&bytes)?)?
    } else {
        return Err("unsupported source mode".into());
    };
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&out)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write source result; effects may exist")?;
    Ok(code)
}
fn main() -> ExitCode {
    match run() {
        Ok(n) => ExitCode::from(n),
        Err(e) => {
            eprintln!("tig-source: {e}");
            ExitCode::from(2)
        }
    }
}

//! Explicit positive acquisition into an already enrolled native destination.
#![forbid(unsafe_code)]
#[path = "tig-acquire/mod.rs"]
mod command;
#[path = "common/provider_profile.rs"]
mod provider_profile;
#[path = "tig-acquire/source_config.rs"]
mod source_config;
use std::{
    env,
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let mode = args.next().ok_or("usage: tig-acquire --apply PROFILE")?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(), "usage: tig-acquire --apply PROFILE\nAcquire exact bytes using an existing native store, saved plan and admitted job.")
            .map_err(|_| "cannot write help")?;
        return Ok(());
    }
    if mode != "--apply" {
        return Err("usage: tig-acquire --apply PROFILE".into());
    }
    let path = args.next().ok_or("usage: tig-acquire --apply PROFILE")?;
    if args.next().is_some() {
        return Err("usage: tig-acquire --apply PROFILE".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open explicit acquisition profile")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read explicit acquisition profile")?;
    let output = command::execution::execute(command::profile::parse(&bytes)?)?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write acquisition result; effects may have completed".into())
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

//! Fresh native capacity admission, or exact historical replay, for one publication.
#![forbid(unsafe_code)]
#[path = "tig-admit/mod.rs"]
mod command;
#[path = "common/native_operation.rs"]
mod native_operation;
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
    let mode = args.next().ok_or("usage: tig-admit --apply PROFILE")?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(),"usage: tig-admit --apply PROFILE\nBind one publication plan to a capacity job; emit its tig-acquire profile.")
            .map_err(|_| "cannot write help")?;
        return Ok(());
    }
    if mode != "--apply" {
        return Err("usage: tig-admit --apply PROFILE".into());
    }
    let path = args.next().ok_or("usage: tig-admit --apply PROFILE")?;
    if args.next().is_some() {
        return Err("usage: tig-admit --apply PROFILE".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open explicit admission input")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read admission input")?;
    let output = command::execution::execute(command::profile::parse(&bytes)?)?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write admission result; reservation may exist".into())
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

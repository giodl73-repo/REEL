//! Thin LocalStore host for the shared history application service.
#![forbid(unsafe_code)]
use std::{io::Write, path::Path, process::ExitCode};
use tig_git::{history_command, source::native::updates::publication::Error};
use tig_store::{existing::Layout, local::LocalStore};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).take(4).collect();
    if args.len() != 3 {
        return Err("usage: tig-history {check|publish|propose|batch-check|batch-publish|batch-propose} PROFILE REQUEST".into());
    }
    let action = args[0].to_str().ok_or("invalid action")?;
    let result = history_command::execute(
        action,
        Path::new(&args[1]),
        Path::new(&args[2]),
        |profile| {
            if !matches!(profile.layout, Layout::Flat) || !profile.settings.is_null() {
                return Err(Error::Unsupported);
            }
            LocalStore::open(&profile.root).map_err(|_| Error::Provider)
        },
    )?;
    writeln!(std::io::stdout().lock(), "{result}").map_err(|_| "cannot write history result")?;
    Ok(())
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

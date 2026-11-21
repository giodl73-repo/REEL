//! Establish/reuse selected native retained claims from an exact saved work recipe.
#![forbid(unsafe_code)]
#[path = "tig-admit/model.rs"]
mod admission;
#[path = "tig-retain/execute.rs"]
mod execute;
#[path = "tig-retain/issuer_profiles.rs"]
mod issuer_profiles;
#[path = "common/native_operation.rs"]
mod native_operation;
#[path = "tig-retain/prepare.rs"]
mod prepare;
#[path = "tig-retain/profile.rs"]
mod profile;
#[path = "common/provider_profile.rs"]
mod provider_profile;
#[path = "tig-acquire/source_config.rs"]
mod source_config;
#[path = "tig-reconcile/witnesses.rs"]
mod witnesses;
#[path = "tig-reconcile/work.rs"]
mod work;
use std::{
    env,
    fs::File,
    io::{Read, Write},
    path::Path,
    process::ExitCode,
};
fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open explicit retention input")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read retention input")?;
    if bytes.len() > 65_536 {
        return Err("retention input byte ceiling exceeded".into());
    }
    Ok(bytes)
}
fn run() -> Result<u8, String> {
    let mut args = env::args_os().skip(1);
    let mode = args.next().ok_or("usage: tig-retain --apply PROFILE")?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(),"usage: tig-retain --apply PROFILE\nEstablish/reuse selected native claims from saved placement work. No custody hold or removal.").map_err(|_|"cannot write help")?;
        return Ok(0);
    }
    if mode != "--apply" {
        return Err("usage: tig-retain --apply PROFILE".into());
    }
    let path = args.next().ok_or("explicit retention profile required")?;
    if args.next().is_some() {
        return Err("one retention profile required".into());
    }
    let config = profile::parse(&read(Path::new(&path))?)?;
    let work = work::parse(&read(&config.work_file)?)?;
    let previous = config
        .previous_claims_file
        .as_deref()
        .map(read)
        .transpose()?;
    let supplied = config.witnesses_file.as_deref().map(read).transpose()?;
    let (output, code) = execute::apply(config, work, previous.as_deref(), supplied.as_deref())?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write retention result; effects may exist")?;
    Ok(code)
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

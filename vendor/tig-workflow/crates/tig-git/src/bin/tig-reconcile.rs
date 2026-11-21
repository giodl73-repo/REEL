//! Compile exact placement requirements and execute a saved positive-copy phase.
#![forbid(unsafe_code)]
#[path = "tig-acquire/mod.rs"]
mod acquisition;
#[path = "tig-admit/mod.rs"]
mod admission;
#[path = "tig-reconcile/compile.rs"]
mod compile;
#[path = "tig-reconcile/execute.rs"]
mod execute;
#[path = "tig-plan/mod.rs"]
mod inspection;
#[path = "common/native_operation.rs"]
mod native_operation;
#[path = "common/provider_profile.rs"]
mod provider_profile;
#[path = "tig-acquire/source_config.rs"]
mod source_config;
#[path = "tig-reconcile/wake.rs"]
mod wake;
#[path = "tig-reconcile/witnesses.rs"]
mod witnesses;
#[path = "tig-reconcile/work.rs"]
mod work;
use std::{
    env,
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
fn read(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open explicit work/witness input")?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read work/witness input")?;
    Ok(bytes)
}
fn run() -> Result<u8, String> {
    let mut args = env::args_os().skip(1);
    let mode = args
        .next()
        .ok_or("usage: tig-reconcile --wake PROFILE | --plan PROFILE | --apply-work WORK")?;
    if mode == "--help" && args.next().is_none() {
        writeln!(std::io::stdout().lock(),"usage: tig-reconcile --wake PROFILE | --plan PROFILE | --apply-work WORK [--witnesses FILE]\nCompile exact desired placements; execute saved bounded positive copies. No removal.").map_err(|_|"cannot write help")?;
        return Ok(0);
    }
    if mode != "--plan" && mode != "--apply-work" && mode != "--wake" {
        return Err(
            "usage: tig-reconcile --wake PROFILE | --plan PROFILE | --apply-work WORK".into(),
        );
    }
    let path = args.next().ok_or("explicit input required")?;
    let witness_path = match args.next() {
        None => None,
        Some(flag) if mode == "--apply-work" && flag == "--witnesses" => {
            Some(args.next().ok_or("explicit witness input required")?)
        }
        _ => return Err("unexpected work arguments".into()),
    };
    if args.next().is_some() {
        return Err("unexpected work arguments".into());
    }
    let bytes = read(std::path::Path::new(&path))?;
    let (output, code) = if mode == "--wake" {
        (wake::observe(&bytes)?, 0)
    } else if mode == "--plan" {
        (compile::compile(&bytes)?, 0)
    } else {
        let minimums = match witness_path {
            Some(path) => witnesses::parse(&read(std::path::Path::new(&path))?)?,
            None => Default::default(),
        };
        execute::apply(work::parse(&bytes)?, minimums)?
    };
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write work result; effects may exist")?;
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

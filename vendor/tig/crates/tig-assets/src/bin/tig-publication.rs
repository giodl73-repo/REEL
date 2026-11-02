//! Read-only, explicit local publication evidence checks; no provider attachment.
#![forbid(unsafe_code)]
use std::{
    env,
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
use tig_assets::{
    native::{Instance, MAX_RECORD_BYTES},
    publication::Checker,
    schema::Schema,
};

fn read(path: &str, limit: usize) -> Result<Vec<u8>, ()> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| ())?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > limit {
        return Err(());
    }
    Ok(bytes)
}
fn run() -> Result<(), ()> {
    let args = env::args_os()
        .skip(1)
        .take(7)
        .map(|s| s.into_string().map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()?;
    if args.len() != 6 {
        return Err(());
    }
    let instance = Instance::parse(&read(&args[0], MAX_RECORD_BYTES)?).map_err(|_| ())?;
    let definition = read(&args[1], MAX_RECORD_BYTES)?;
    let schema = read(&args[2], MAX_RECORD_BYTES)?;
    let bound = instance.bind(&definition, &schema).map_err(|_| ())?;
    let engine = Schema::compile(
        bound.definition(),
        &schema,
        args[3].parse().map_err(|_| ())?,
    )
    .map_err(|_| ())?;
    let checked = engine.check(&bound).map_err(|_| ())?;
    let policy = read(&args[4], MAX_RECORD_BYTES)?;
    let checker = Checker::new(&bound, &checked, &policy).map_err(|_| ())?;
    let receipt = read(&args[5], checker.receipt().bytes as usize)?;
    let report = checker.check(&receipt).map_err(|_| ())?;
    let raw = serde_json::to_vec(&report).map_err(|_| ())?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&raw)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| ())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => {
            eprintln!("publication evidence check refused; verify explicit inputs and policy");
            ExitCode::FAILURE
        }
    }
}

use clap::Parser;
use std::{path::PathBuf, process::ExitCode};
#[derive(Parser)]
#[command(
    about = "Register pinned native scene source as candidate Tig files; no media copy or adoption"
)]
struct Args {
    profile: PathBuf,
    output: PathBuf,
}
fn main() -> ExitCode {
    let args = Args::parse();
    match reel::tig_native_registration::run(&args.profile, &args.output) {
        Ok(result) => {
            println!("{result}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

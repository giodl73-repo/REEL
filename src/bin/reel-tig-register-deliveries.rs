use clap::Parser;
use std::{path::PathBuf, process::ExitCode};
#[derive(Parser)]
#[command(
    about = "Propose independent file anchors from an exact retained delivery table; no media, adoption or upload"
)]
struct Args {
    profile: PathBuf,
    output: PathBuf,
}
fn main() -> ExitCode {
    let args = Args::parse();
    match reel::tig_delivery_table_registration::run(&args.profile, &args.output) {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("delivery table registration refused: {error}");
            ExitCode::FAILURE
        }
    }
}

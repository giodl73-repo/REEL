use clap::Parser;
use std::{path::PathBuf, process::ExitCode};
#[derive(Parser)]
#[command(
    about = "Register pinned native scenes or versioned packages as Tig candidates; no media copy or adoption"
)]
struct Args {
    /// Register a family-neutral versioned package record into one new file.
    #[arg(long, conflicts_with = "package_assets")]
    versioned: bool,
    /// Register explicitly mapped package assets, including prior versions.
    #[arg(long)]
    package_assets: bool,
    /// Register explicit component masters from exact native episode conforms.
    #[arg(long, conflicts_with_all = ["versioned", "package_assets"])]
    conform_components: bool,
    profile: PathBuf,
    output: PathBuf,
}
fn main() -> ExitCode {
    let args = Args::parse();
    let result = if args.conform_components {
        reel::tig_conform_registration::run(&args.profile, &args.output)
    } else if args.package_assets {
        reel::tig_package_registration::run(&args.profile, &args.output)
    } else if args.versioned {
        reel::tig_versioned_registration::run(&args.profile, &args.output)
    } else {
        reel::tig_native_registration::run(&args.profile, &args.output)
    };
    match result {
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

use anyhow::{Result, bail};
use std::{env, path::Path};

fn run() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    if let [
        _,
        command,
        manifest,
        input_flag,
        input_root,
        asset_flag,
        asset_root,
        output_flag,
        output,
    ] = args.as_slice()
    {
        if input_flag == "--input-root"
            && asset_flag == "--asset-root"
            && output_flag == "--output-dir"
        {
            if command == "build" {
                let receipt = reel::presentation_adopt::build(
                    Path::new(manifest),
                    Path::new(input_root),
                    Path::new(asset_root),
                    Path::new(output),
                )?;
                println!(
                    "{} {} {}",
                    receipt.role, receipt.language, receipt.master_sha256
                );
                return Ok(());
            }
            if command == "check" {
                let output = Path::new(output);
                reel::presentation_adopt::check(
                    Path::new(manifest),
                    Path::new(input_root),
                    Path::new(asset_root),
                    &output.join("master.mkv"),
                    &output.join("receipt.json"),
                    output.parent().unwrap_or(Path::new(".")),
                )?;
                println!("presentation adoption verified");
                return Ok(());
            }
        }
    }
    bail!(
        "usage: reel-presentation-adopt <build|check> <manifest.json> --input-root <authoring-root> --asset-root <hydrated-media-root> --output-dir <directory>"
    )
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

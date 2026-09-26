use anyhow::{Result, bail};
use std::{env, path::Path};

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let [
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
    else {
        bail!(
            "usage: reel-episode-conform build <manifest.json> --input-root <authoring-root> --asset-root <hydrated-media-root> --output-dir <new-dir>"
        );
    };
    if command != "build"
        || input_flag != "--input-root"
        || asset_flag != "--asset-root"
        || output_flag != "--output-dir"
    {
        bail!("invalid episode conform arguments");
    }
    let receipt = reel::episode_conform::build(
        Path::new(manifest),
        Path::new(input_root),
        Path::new(asset_root),
        Path::new(output),
    )?;
    println!(
        "{} {} {} frames {} samples",
        receipt.episode_id, receipt.language, receipt.total_frames, receipt.total_samples
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

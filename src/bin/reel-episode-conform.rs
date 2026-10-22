use anyhow::{Result, bail};
use std::{env, path::Path};

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if let [
        _,
        command,
        job,
        asset_flag,
        asset_root,
        output_flag,
        output,
        proof_flag,
        proof,
    ] = args.as_slice()
    {
        if command == "prove-scene" {
            if asset_flag != "--asset-root"
                || output_flag != "--output-dir"
                || proof_flag != "--proof-path"
            {
                bail!("invalid scene proof arguments");
            }
            reel::scene_delivery::prove_master(
                Path::new(job),
                Path::new(asset_root),
                Path::new(output),
                Path::new(proof),
            )?;
            println!("complete scene check verified; selected master proof written");
            return Ok(());
        }
    }
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
    if !matches!(command.as_str(), "build" | "prepare" | "finish")
        || input_flag != "--input-root"
        || asset_flag != "--asset-root"
        || output_flag != "--output-dir"
    {
        bail!("invalid episode conform arguments");
    }
    if command == "prepare" {
        reel::episode_conform::prepare(
            Path::new(manifest),
            Path::new(input_root),
            Path::new(asset_root),
            Path::new(output),
        )?;
        println!(
            "source preparation complete; full episode verification and delivery remain pending"
        );
        return Ok(());
    }
    if command == "finish" {
        if Path::new(input_root).canonicalize()? != Path::new(asset_root).canonicalize()? {
            bail!("finish requires one closed input/asset workspace");
        }
        let receipt = reel::episode_conform::finish(
            Path::new(manifest),
            Path::new(asset_root),
            Path::new(output),
        )?;
        println!(
            "{} {} {} frames {} samples",
            receipt.episode_id, receipt.language, receipt.total_frames, receipt.total_samples
        );
        return Ok(());
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

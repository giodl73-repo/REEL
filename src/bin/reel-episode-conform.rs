use anyhow::{Result, bail};
use std::{env, path::Path};

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if let [
        _,
        command,
        manifest,
        input_flag,
        input,
        asset_flag,
        assets,
        output_flag,
        output,
        limit_flag,
        limit,
    ] = args.as_slice()
    {
        if command == "prepare-batch"
            && input_flag == "--input-root"
            && asset_flag == "--asset-root"
            && output_flag == "--output-dir"
            && limit_flag == "--max-segments"
        {
            reel::episode_conform::prepare_batch(
                Path::new(manifest),
                Path::new(input),
                Path::new(assets),
                Path::new(output),
                limit.parse()?,
            )?;
            println!(
                "bounded normalization batch retained; assembly and final verification remain pending"
            );
            return Ok(());
        }
    }
    if let [_, command, prepared, asset_flag, assets] = args.as_slice() {
        if command == "assemble-preparation" && asset_flag == "--asset-root" {
            reel::episode_conform::assemble_preparation(Path::new(prepared), Path::new(assets))?;
            println!(
                "preparation assembled; full episode verification and delivery remain pending"
            );
            return Ok(());
        }
        if asset_flag == "--asset-root" && command == "verify-consume-normalized" {
            reel::episode_conform::verify_prepared_consuming_normalized(
                Path::new(prepared),
                Path::new(assets),
            )?;
            println!("ordered lossless checks retained; compact delivery remains pending");
            return Ok(());
        }
        if asset_flag == "--asset-root" && command == "encode-verified" {
            reel::episode_conform::encode_verified_preparation(
                Path::new(prepared),
                Path::new(assets),
            )?;
            println!("compact encoding retained; full decode and policy checks remain pending");
            return Ok(());
        }
    }
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
    if !matches!(
        command.as_str(),
        "build" | "prepare" | "finish" | "finish-consume-normalized" | "finish-encoded"
    ) || input_flag != "--input-root"
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
    if matches!(
        command.as_str(),
        "finish" | "finish-consume-normalized" | "finish-encoded"
    ) {
        if Path::new(input_root).canonicalize()? != Path::new(asset_root).canonicalize()? {
            bail!("finish requires one closed input/asset workspace");
        }
        let finish = match command.as_str() {
            "finish-consume-normalized" => reel::episode_conform::finish_consuming_normalized,
            "finish-encoded" => reel::episode_conform::finish_encoded_preparation,
            _ => reel::episode_conform::finish,
        };
        let receipt = finish(
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

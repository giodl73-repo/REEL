use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if let [_, command, root, manifest, asset_flag, asset_root] = args.as_slice()
        && command == "check-inputs"
        && asset_flag == "--asset-root"
    {
        let path = Path::new(root).join(manifest);
        let reference = reel::scene_delivery::FileRef {
            path: manifest.into(),
            sha256: Sha256::digest(fs::read(&path)?)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            bytes: fs::metadata(&path)?.len(),
        };
        reel::imported_scene_proof::check_inputs(
            Path::new(root),
            &reference,
            Path::new(asset_root),
        )?;
        println!("source bindings and native clocks verified; no media or proof produced");
        return Ok(());
    }
    if let [
        _,
        command,
        root,
        manifest,
        asset_flag,
        asset_root,
        output_flag,
        output,
    ] = args.as_slice()
        && command == "verify"
        && asset_flag == "--asset-root"
        && output_flag == "--output-dir"
    {
        let path = Path::new(root).join(manifest);
        let reference = reel::scene_delivery::FileRef {
            path: manifest.into(),
            sha256: Sha256::digest(fs::read(&path)?)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            bytes: fs::metadata(&path)?.len(),
        };
        let receipt = reel::imported_scene_proof::verify_render(
            Path::new(root),
            &reference,
            Path::new(asset_root),
            Path::new(output),
        )?;
        println!("{}", serde_json::to_string_pretty(&receipt)?);
        return Ok(());
    }
    bail!(
        "usage: reel-imported-scene-verify check-inputs <input-root> <manifest.json> --asset-root <cache-root> | verify <input-root> <manifest.json> --asset-root <cache-root> --output-dir <retained-render>"
    )
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

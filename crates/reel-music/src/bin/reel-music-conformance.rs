use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};
use reel_music::{
    capture_comparison, conformance,
    extensions::{self, FilePin, Registry},
};

fn main() -> Result<()> {
    let args: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    match args.as_slice() {
        [option, registry] if option.as_os_str() == "extensions" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Registry::load(registry)?.inspect())?
            );
        }
        [option, registry, source, hash, bytes, output]
            if option.as_os_str() == "capture-muscriptor" =>
        {
            let source = FilePin {
                path: source.canonicalize()?,
                sha256: hash.to_str().context("expected a UTF-8 SHA-256")?.into(),
                bytes: bytes
                    .to_str()
                    .context("expected a source byte size")?
                    .parse()?,
            };
            extensions::capture_muscriptor(&Registry::load(registry)?, source, output)?;
        }
        [option, registry, source, output] if option.as_os_str() == "run-demucs" => {
            extensions::run_demucs(&Registry::load(registry)?, source, output)?;
        }
        [option, registry, root, request, output] if option.as_os_str() == "run-pop2piano" => {
            extensions::run_pop2piano(&Registry::load(registry)?, root, request, output)?;
        }
        [option, left, right, left_label, right_label, tolerance]
            if option.as_os_str() == "compare-captures" =>
        {
            let report = capture_comparison::compare(
                left,
                right,
                left_label.to_str().context("invalid left label")?,
                right_label.to_str().context("invalid right label")?,
                tolerance.to_str().context("invalid tolerance")?.parse()?,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        [policy, analysis] => {
            println!(
                "{}",
                serde_json::to_string_pretty(&conformance::evaluate(policy, analysis)?)?
            );
        }
        _ => bail!(
            "usage: reel-music-conformance POLICY ANALYSIS | extensions REGISTRY | capture-muscriptor REGISTRY WAV SHA256 BYTES OUTPUT | run-demucs REGISTRY AUDIO OUTPUT | run-pop2piano REGISTRY ROOT REQUEST OUTPUT | compare-captures LEFT RIGHT LEFT_LABEL RIGHT_LABEL TOLERANCE_MS"
        ),
    }
    Ok(())
}

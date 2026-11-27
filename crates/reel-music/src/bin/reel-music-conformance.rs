use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};
use reel_music::{
    conformance,
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
        [policy, analysis] => {
            println!(
                "{}",
                serde_json::to_string_pretty(&conformance::evaluate(policy, analysis)?)?
            );
        }
        _ => bail!(
            "usage: reel-music-conformance POLICY ANALYSIS | extensions REGISTRY | capture-muscriptor REGISTRY WAV SHA256 BYTES OUTPUT"
        ),
    }
    Ok(())
}

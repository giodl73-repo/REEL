use anyhow::{Result, bail};
use reel_music::conformance;
use std::{env, path::PathBuf};

fn main() -> Result<()> {
    let args: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    let [policy, analysis] = args.as_slice() else {
        bail!("usage: reel-music-conformance POLICY.yaml ANALYSIS.yaml");
    };
    let report = conformance::evaluate(policy, analysis)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

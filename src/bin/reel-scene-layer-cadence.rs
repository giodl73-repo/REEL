use anyhow::{Result, bail};
use std::{env, fs, io::Write, path::Path};
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let [
        job,
        asset_flag,
        assets,
        render_flag,
        render,
        expect_flag,
        expectations,
        output_flag,
        output,
    ] = args.as_slice()
    else {
        bail!(
            "usage: reel-scene-layer-cadence <job> --asset-root <assets> --render-root <render> --expectations <request.json> --output <new-report.json>"
        );
    };
    if asset_flag != "--asset-root"
        || render_flag != "--render-root"
        || expect_flag != "--expectations"
        || output_flag != "--output"
    {
        bail!("invalid layer cadence arguments");
    }
    let output = Path::new(output);
    if output.exists() {
        bail!("layer cadence report must be new");
    }
    let report = reel::motioncraft_layers::analyze(
        Path::new(job),
        Path::new(assets),
        Path::new(render),
        Path::new(expectations),
    )?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&serde_json::to_vec_pretty(&report)?)?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    if report["passed"] != true {
        bail!("layer cadence failed or is inconclusive; report retained");
    }
    Ok(())
}

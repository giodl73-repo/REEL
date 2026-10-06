use anyhow::{Result, bail};
use std::{env, fs, io::Write, path::Path};
fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let [
        _,
        job,
        flag,
        assets,
        render_flag,
        render,
        output_flag,
        output,
    ] = args.as_slice()
    else {
        bail!(
            "usage: reel-scene-cadence <job.json> --asset-root <assets> --render-root <existing-render> --output <new-report.json>"
        );
    };
    if flag != "--asset-root" || render_flag != "--render-root" || output_flag != "--output" {
        bail!("invalid scene cadence arguments");
    }
    let output = Path::new(output);
    if output.exists() {
        bail!("cadence output must be new");
    }
    let report = reel::motioncraft_cadence::checked_report(
        Path::new(job),
        Path::new(assets),
        Path::new(render),
    )?;
    let parent = output.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&serde_json::to_vec_pretty(&report)?)?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    println!("cadence passed={}", report["passed"]);
    if report["passed"] != true {
        bail!("cadence check failed or needs separate layer analysis; report retained");
    }
    Ok(())
}

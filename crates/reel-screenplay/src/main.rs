use anyhow::{Result, ensure};
use std::{env, path::Path};
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    ensure!(
        args.len() == 4 && args[0] == "export",
        "Usage: reel-screenplay export REQUEST SOURCE_ROOT NEW_OUTPUT_DIRECTORY"
    );
    let session = reel_screenplay::export(
        Path::new(&args[2]),
        Path::new(&args[1]),
        Path::new(&args[3]),
    )?;
    println!(
        "{}",
        serde_json::json!({"show": session.request.show_id, "episodes": session.request.episodes.len(), "turns": session.turns.len(), "bilingual_structure_checked": session.bilingual_structure_checked})
    );
    Ok(())
}

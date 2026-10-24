use anyhow::{Result, bail};
use reel_music::note_editor;
use std::{env, path::Path};
fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_, command, model, output] if command == "export" => {
            let notes = note_editor::export(Path::new(model), Path::new(output))?;
            println!(
                "{}",
                serde_json::json!({"notes":notes,"media_resolved":false,"native_validation":"not-run"})
            );
        }
        [_, command, model, request, output] if command == "apply" => {
            let changed =
                note_editor::apply(Path::new(model), Path::new(request), Path::new(output))?;
            println!(
                "{}",
                serde_json::json!({"changed_notes":changed,"review":"not-reviewed","media_resolved":false,"native_validation":"not-run"})
            );
        }
        _ => bail!(
            "usage: reel-music-editor export <model> <new-editor-dir> | apply <model> <edit.json> <new-model.json-beside-source>"
        ),
    }
    Ok(())
}

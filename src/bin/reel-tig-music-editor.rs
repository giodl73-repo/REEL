//! Explicit private profile; no auto-configuration, publication or server.
use anyhow::Result;
use clap::{Parser, Subcommand};
use reel::{tig_inputs::Profile, tig_music_editor::Editor};
use std::path::PathBuf;
#[derive(Parser)]
struct Args {
    #[arg(long)]
    profile: PathBuf,
    #[command(subcommand)]
    operation: Operation,
}
#[derive(Subcommand)]
enum Operation {
    Export {
        asset_id: String,
        output: PathBuf,
    },
    Propose {
        source_lock: PathBuf,
        edit: PathBuf,
        output: PathBuf,
    },
    ExportUnderlay {
        asset_id: String,
        output: PathBuf,
    },
    ProposeUnderlay {
        source_lock: PathBuf,
        edit: PathBuf,
        output: PathBuf,
    },
}
fn main() -> Result<()> {
    let args = Args::parse();
    let mut editor = Editor::open(Profile::read(&args.profile)?)?;
    let (operation, key, count) = match args.operation {
        Operation::ExportUnderlay { asset_id, output } => (
            "export-underlay",
            "underlay_spans",
            editor.export_underlay(&asset_id, &output)?,
        ),
        Operation::ProposeUnderlay {
            source_lock,
            edit,
            output,
        } => (
            "propose-underlay",
            "underlay_spans",
            editor.propose_underlay(&source_lock, &edit, &output)?,
        ),
        Operation::Export { asset_id, output } => {
            ("export", "notes", editor.export(&asset_id, &output)?)
        }
        Operation::Propose {
            source_lock,
            edit,
            output,
        } => (
            "propose",
            "changed_notes",
            editor.propose(&source_lock, &edit, &output)?,
        ),
    };
    let mut summary = serde_json::json!({"operation":operation,"media_resolved":false,
        "native_validation":"not-run","published":false,"git_changed":false,"human_approval":false});
    summary[key] = count.into();
    println!("{summary}");
    Ok(())
}

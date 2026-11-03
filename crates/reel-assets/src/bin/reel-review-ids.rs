use reel_assets::{Result, review_ids};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};

fn run(args: &[String]) -> Result<()> {
    if matches!(
        args.first().map(String::as_str),
        Some("inspect" | "allocate")
    ) {
        let output = Path::new(args.last().ok_or("Missing report")?);
        if std::fs::symlink_metadata(output).is_ok() || !output.parent().is_some_and(Path::is_dir) {
            return Err("New report with existing parent required".into());
        }
    }
    let report = match args.first().map(String::as_str) {
        Some("adopt") if args.len() == 5 => {
            let report = review_ids::adopt(Path::new(&args[1]), &args[2], args[3].parse()?, Path::new(&args[4]))?;
            println!("Adopted {} existing assignments; allocated zero new IDs", report.registry.by_source.len());
            return Ok(());
        }
        Some("inspect") if args.len() == 3 => review_ids::inspect(Path::new(&args[1]))?,
        Some("allocate") if args.len() == 4 => {
            let mut bytes = Vec::new();
            let file = std::fs::File::open(&args[2])?;
            if !file.metadata()?.is_file() { return Err("Regular request required".into()); }
            file.take(131073).read_to_end(&mut bytes)?;
            if bytes.len() > 131072 { return Err("Request too large".into()); }
            let request = serde_json::from_slice(&bytes)?;
            review_ids::allocate(Path::new(&args[1]), &request)?
        }
        _ => return Err("Usage: adopt SOURCE SHA256 BYTES NEW_STATE | inspect STATE NEW_REPORT | allocate STATE REQUEST NEW_REPORT".into()),
    };
    let output = Path::new(args.last().ok_or("Missing report")?);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&serde_json::to_vec(&report)?)?;
    file.sync_all()?;
    println!(
        "Verified {} assignments; allocated {} new IDs",
        report.registry.by_source.len(),
        report.new_assignments
    );
    Ok(())
}

fn main() {
    if run(&std::env::args().skip(1).collect::<Vec<_>>()).is_err() {
        eprintln!("Review ID operation refused; inspect explicit inputs/state and retry safely");
        std::process::exit(1);
    }
}

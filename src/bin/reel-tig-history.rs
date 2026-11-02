use std::{io::Write, path::Path, process::ExitCode};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).take(4).collect();
    if args.len() != 3 {
        return Err("usage: reel-tig-history check|publish|propose PROFILE REQUEST".into());
    }
    let result = reel::tig_history::execute(
        args[0].to_str().ok_or("invalid action")?,
        Path::new(&args[1]),
        Path::new(&args[2]),
    )?;
    writeln!(std::io::stdout().lock(), "{result}").map_err(|_| "cannot write history result")?;
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

use reel_workflow::{Error, ErrorKind, Result};
use std::path::Path;

fn report_projection(board: reel_workflow::Artifact) {
    println!(
        "{}",
        serde_json::json!({"ok":true,"board":board,"media_verified":false,"approvals_verified":false,"production_adopted":false})
    );
}

fn run(args: &[String]) -> Result<()> {
    if args == ["capabilities"] {
        println!(
            "{}",
            serde_json::json!({"film_metadata_projection":true,"native_film_metadata_projection":cfg!(windows),"read_only_board":cfg!(windows),"lazy_previews":cfg!(windows),"journal_observations":cfg!(windows),"native_custody_verification":false,"job_execution":false,"selection":false})
        );
        return Ok(());
    }
    if let [command, request, pin, repository, output] = args
        && command == "project-film"
    {
        let board = reel_workflow_ui::film::export(
            Path::new(request),
            pin,
            Path::new(repository),
            Path::new(output),
        )?;
        report_projection(board);
        return Ok(());
    }
    #[cfg(windows)]
    if let [command, request, pin, packet, profile, output] = args
        && command == "project-native-film"
    {
        use reel_workflow_native::{NativeInputs, film_metadata, read_control};
        use reel_workflow_ui::film;

        let board = film::export_with(Path::new(request), pin, Path::new(output), |request| {
            let packet = read_control(Path::new(packet))?;
            let profile = read_control(Path::new(profile))?;
            let mut inputs = NativeInputs::open(&packet, &profile)?;
            let mut reader = film_metadata::reader(&request.inspection, &mut inputs)?;
            film::project(request, &mut reader)
        })?;
        report_projection(board);
        return Ok(());
    }
    #[cfg(windows)]
    if let [command, path, pin, options @ ..] = args {
        let source = reel_workflow_ui::host::load(std::path::Path::new(path), pin)?;
        if command == "check" && options.is_empty() {
            println!(
                "{}",
                serde_json::json!({"ok":true,"metadata_checked":true,"preview_bytes_read":false})
            );
            return Ok(());
        }
        if command == "serve" {
            let mut settings = reel_workflow_ui::host::Options {
                port: 0,
                idle_seconds: 900,
                max_seconds: 21600,
                open: false,
            };
            let mut i = 0;
            while i < options.len() {
                match options[i].as_str() {
                    "--open" => settings.open = true,
                    "--port" | "--idle-seconds" | "--max-seconds" => {
                        let key = &options[i];
                        i += 1;
                        let value = options.get(i).ok_or_else(|| Error {
                            kind: ErrorKind::Invalid,
                            code: "board-option-value".into(),
                        })?;
                        match key.as_str() {
                            "--port" => {
                                settings.port = value.parse().map_err(|_| Error {
                                    kind: ErrorKind::Invalid,
                                    code: "board-port".into(),
                                })?
                            }
                            "--idle-seconds" => {
                                settings.idle_seconds = value.parse().map_err(|_| Error {
                                    kind: ErrorKind::Invalid,
                                    code: "board-deadline".into(),
                                })?
                            }
                            _ => {
                                settings.max_seconds = value.parse().map_err(|_| Error {
                                    kind: ErrorKind::Invalid,
                                    code: "board-deadline".into(),
                                })?
                            }
                        }
                    }
                    _ => {
                        return Err(Error {
                            kind: ErrorKind::Invalid,
                            code: "board-options".into(),
                        });
                    }
                }
                i += 1;
            }
            return reel_workflow_ui::host::serve(source, settings);
        }
    }
    Err(Error {
        kind: ErrorKind::Unsupported,
        code: "board-command-or-qualified-scope".into(),
    })
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "reel-workflow-ui capabilities\nreel-workflow-ui project-film REQUEST SHA256 REPOSITORY NEW_BOARD\nreel-workflow-ui project-native-film REQUEST SHA256 PACKET NATIVE_PROFILE NEW_BOARD (Windows)\nreel-workflow-ui check BOARD SHA256\nreel-workflow-ui serve BOARD SHA256 [--open] [--port N] [--idle-seconds N] [--max-seconds N]\nWindows-owned read-only view. No job execution, selection, hydration or retirement."
        );
        return std::process::ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", serde_json::json!({"ok":false,"error":error}));
            std::process::ExitCode::FAILURE
        }
    }
}

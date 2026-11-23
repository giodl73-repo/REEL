use reel_workflow::{Error, ErrorKind, Result};
fn run(args: &[String]) -> Result<serde_json::Value> {
    match args {
        [command] if command == "capabilities" => Ok(
            serde_json::json!({"http_transport":true,"owned_cli":cfg!(windows),"workflow_proxy":true,"github_issue_submission":cfg!(windows),"native_admission":false,"native_retention":false,"task_completion":false,"speech_adapter":false}),
        ),
        [command, repository, draft, pin] if command == "prepare-github-issue" => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(draft)
                .and_then(|file| {
                    file.take(reel_workflow::MAX_CONTROL_BYTES as u64 + 1)
                        .read_to_end(&mut bytes)
                })
                .map_err(|_| Error {
                    kind: ErrorKind::Denied,
                    code: "github-issue-draft-read".into(),
                })?;
            let spec = reel_workflow_http::github_issue::Spec::from_pinned_draft(
                repository.clone(),
                &bytes,
                pin,
            )?;
            Ok(serde_json::json!({"kind": "github-issue", "spec": spec}))
        }
        #[cfg(windows)]
        [command, request, pin, apply] if command == "post" && apply == "--apply" => {
            reel_workflow_http::call::execute(std::path::Path::new(request), pin, false)
        }
        #[cfg(windows)]
        [command, request, pin] if command == "resume" => {
            reel_workflow_http::call::execute(std::path::Path::new(request), pin, true)
        }
        _ => Err(Error {
            kind: ErrorKind::Unsupported,
            code: "http-command-or-qualified-scope".into(),
        }),
    }
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "reel-workflow-http capabilities\nreel-workflow-http prepare-github-issue OWNER/REPO DRAFT DRAFT_SHA256\nreel-workflow-http post REQUEST REQUEST_SHA256 --apply\nreel-workflow-http resume REQUEST REQUEST_SHA256\nOwned CLI: Windows only. Resume reconciles evidence and never sends HTTP.\nNo native admission, retained completion, selection or scratch retirement."
        );
        return std::process::ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(value) => {
            println!("{value}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", serde_json::json!({"ok":false,"error":error}));
            std::process::ExitCode::FAILURE
        }
    }
}

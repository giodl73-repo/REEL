//! Explicit project restoration. No auto-install, provider discovery or cleanup.
#![forbid(unsafe_code)]
use std::{
    env,
    fs::File,
    io::{Read, Write},
    process::ExitCode,
};
use tig_core::Digest;

// Never echo provider errors: they can contain paths, identifiers or credentials.
// Only exact, reviewed static refusals acquire a public diagnostic.
fn diagnostic(reason: &str) -> (&'static str, &'static str) {
    match reason {
        "invalid project flags" | "invalid project arguments" => (
            "arguments-invalid",
            "Check flag combinations; put flags before PROFILE MANIFEST.",
        ),
        "invalid plan" | "plan required" => (
            "plan-required",
            "Supply the exact SHA256 from a fresh preview with --expect-plan.",
        ),
        "cannot read explicit profile" => (
            "profile-unreadable",
            "Check the supplied local profile exists and is readable.",
        ),
        "invalid profile JSON" | "invalid project profile" => (
            "profile-invalid",
            "Check the bounded profile JSON, schema and supported fields.",
        ),
        "invalid manifest"
        | "invalid manifest path"
        | "invalid manifest JSON"
        | "invalid project manifest"
        | "invalid manifest scope" => (
            "manifest-invalid",
            "Check the manifest path, JSON and admitted scopes at the pinned commit.",
        ),
        "expected plan differs" => (
            "plan-changed",
            "Preview the intended source and profile again; review the new plan before apply.",
        ),
        "destination alias or parent collision" => (
            "destination-collision",
            "Give entries distinct portable destinations, including file-parent paths.",
        ),
        "destination outside declared prefix" | "invalid declared destination" => (
            "destination-invalid",
            "Use portable relative destinations inside a declared manifest prefix.",
        ),
        "requested entry budget exceeded"
        | "destination byte budget exceeded"
        | "source verification budget insufficient for complete operation" => (
            "budget-insufficient",
            "Review requested entries and explicit byte/read limits before retrying.",
        ),
        "requested asset unavailable" | "requested asset selection unavailable" => (
            "selection-unavailable",
            "Check the requested ID and selector in the exact committed source scopes.",
        ),
        "cannot encode result" | "cannot write result" => (
            "report-unavailable",
            "Check the output sink; copies may already exist, so use verified retry.",
        ),
        _ => (
            "operation-refused",
            "Check explicit source, profile, stores and evidence; no automatic retry or cleanup occurred.",
        ),
    }
}

fn run() -> Result<bool, String> {
    let mut args: Vec<_> = env::args_os().skip(1).take(11).collect();
    let mut apply = false;
    let mut expected = None;
    let mut progress = None;
    let mut inspect = None;
    let mut inspect_partials = false;
    while let Some(first) = args.first() {
        if first == "--apply" && !apply {
            apply = true;
            args.remove(0);
        } else if first == "--expect-plan" && expected.is_none() && args.len() > 1 {
            expected = Some(
                Digest::parse(args[1].to_str().ok_or("invalid plan")?)
                    .map_err(|_| "invalid plan")?,
            );
            args.drain(..2);
        } else if first == "--progress" && progress.is_none() && args.len() > 1 {
            progress = Some(std::path::PathBuf::from(&args[1]));
            args.drain(..2);
        } else if first == "--inspect-progress" && inspect.is_none() && args.len() > 1 {
            inspect = Some(std::path::PathBuf::from(&args[1]));
            args.drain(..2);
        } else if first == "--inspect-partials" && !inspect_partials {
            inspect_partials = true;
            args.remove(0);
        } else if first.to_string_lossy().starts_with("--") {
            return Err("invalid project flags".into());
        } else {
            break;
        }
    }
    if args.len() != 2
        || (progress.is_some() && (!apply || inspect.is_some() || expected.is_none()))
        || (inspect.is_some() && (apply || expected.is_none()))
        || (inspect_partials
            && (apply || progress.is_some() || inspect.is_some() || expected.is_none()))
    {
        return Err("invalid project arguments".into());
    }
    let mut profile = Vec::new();
    File::open(&args[0])
        .map_err(|_| "cannot read explicit profile")?
        .take(65537)
        .read_to_end(&mut profile)
        .map_err(|_| "cannot read explicit profile")?;
    let plan = tig_git::project::prepare(&profile, args[1].to_str().ok_or("invalid manifest")?)?;
    let (success, raw) = if inspect_partials {
        let summary = plan.inspect_partials(expected.as_ref().ok_or("plan required")?)?;
        (
            true,
            serde_json::to_vec(&summary).map_err(|_| "cannot encode result")?,
        )
    } else if let Some(path) = inspect {
        let summary = plan.inspect_progress(expected.as_ref().ok_or("plan required")?, &path)?;
        (
            true,
            serde_json::to_vec(&summary).map_err(|_| "cannot encode result")?,
        )
    } else {
        let report = if let Some(path) = progress {
            plan.run_logged(expected.as_ref().ok_or("plan required")?, &path)?
        } else {
            plan.run(apply, expected.as_ref())?
        };
        (
            report.failure.is_none(),
            serde_json::to_vec(&report).map_err(|_| "cannot encode result")?,
        )
    };
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&raw)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write result")?;
    Ok(success)
}
fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(2),
        Err(reason) => {
            let (code, hint) = diagnostic(&reason);
            eprintln!("project restore refused; verify explicit source, profile and plan");
            eprintln!("code={code}; {hint}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::diagnostic;

    #[test]
    fn unreviewed_errors_never_reach_operator_output() {
        for reason in [
            "C:/private/project/voice.wav token=SECRET",
            "expected plan differs\ncredential=SECRET",
            "destination alias or parent collision: private-ID",
            "\u{1b}[31mSECRET\u{1b}[0m",
        ] {
            let (code, hint) = diagnostic(reason);
            assert_eq!(code, "operation-refused");
            assert!(!hint.contains("SECRET"));
            assert!(!hint.contains("private"));
        }
        assert_eq!(diagnostic("expected plan differs").0, "plan-changed");
        assert_eq!(
            diagnostic("destination alias or parent collision").0,
            "destination-collision"
        );
    }
}

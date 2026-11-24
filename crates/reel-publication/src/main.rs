//! Publication metadata checks and explicitly requested private comic operations.
use reel_publication::{MAX_CONTROL_BYTES, Publication};
use std::{fs::File, io::Read};

fn control<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, &'static str> {
    let file = File::open(path).map_err(|_| "manifest-open")?;
    let mut bytes = Vec::new();
    file.take(MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "manifest-read")?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return Err("manifest-bytes");
    }
    serde_json::from_slice(&bytes).map_err(|_| "manifest-json")
}
fn check(path: &str) -> Result<(), &'static str> {
    let publication: Publication = control(path)?;
    reel_publication::check_publication(&publication).map_err(|e| e.code)
}
fn check_issue(path: &str) -> Result<(), &'static str> {
    let raw: reel_publication::comic_issue_source::Plan = control(path)?;
    raw.check().map_err(|_| "issue-plan-contract")
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args.as_slice(),[arg] if arg == "-h" || arg == "--help") {
        println!(
            "reel-publication check MANIFEST\nreel-publication comic-render REQUEST NEW_DIRECTORY\nreel-publication comic-check REQUEST PACKET\nreel-publication comic-restore PACKET NEW_DIRECTORY RECEIPT_SHA256\nreel-publication comic-issue-check PLAN\nreel-publication comic-issue-assemble REQUEST NEW_DIRECTORY\nMetadata checks are read-only. Comic production/check/restore are explicit private operations, without native retention, selection or release."
        );
        println!(
            "reel-publication bbj-prepare SELECTION INPUT_ROOT PUBLICATION LANGUAGE NEW_DIRECTORY\nreel-publication layered-compose REQUEST INPUT_BINDINGS NEW_DIRECTORY\nreel-publication layered-compose-book PREPARED_BOOK NEW_DIRECTORY\nBBJ preparation reads named controls; layered composition verifies SVG sources and writes an editable source bundle without rasterization or native retention. Book composition reuses exact selected reading PNGs after SVG comparison."
        );
        #[cfg(feature = "comic")]
        println!(
            "reel-publication check-book-delta BASELINE BASELINE_SHA256 CANDIDATE CANDIDATE_SHA256 PAGE_ID"
        );
        return std::process::ExitCode::SUCCESS;
    }
    #[cfg(feature = "comic")]
    if let [
        command,
        baseline,
        baseline_sha,
        candidate,
        candidate_sha,
        target,
    ] = args.as_slice()
        && command == "check-book-delta"
    {
        use std::path::Path;
        return match reel_publication::book_delta::check(
            Path::new(baseline),
            baseline_sha,
            Path::new(candidate),
            candidate_sha,
            target,
        ) {
            Ok(unchanged) => {
                println!(
                    "{}",
                    serde_json::json!({"ok":true,"target_page":target,"unchanged_pages":unchanged})
                );
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"error":{"code":"book-delta-refused","detail":error.to_string()}})
                );
                std::process::ExitCode::FAILURE
            }
        };
    }
    let result = match args.as_slice() {
        #[cfg(feature = "comic")]
        [command, prepared, output] if command == "layered-compose-book" => {
            reel_publication::layered::compose_book(
                std::path::Path::new(prepared),
                std::path::Path::new(output),
            )
            .map_err(|_| "layered-compose-book-refused-preserve-partial")
        }
        #[cfg(feature = "comic")]
        [command, selection, root, publication, language, output] if command == "bbj-prepare" => {
            reel_publication::layered::prepare_bbj(
                std::path::Path::new(selection),
                std::path::Path::new(root),
                publication,
                language,
                std::path::Path::new(output),
            )
            .map_err(|_| "bbj-prepare-refused-preserve-partial")
        }
        #[cfg(feature = "comic")]
        [command, request, bindings, output] if command == "layered-compose" => {
            reel_publication::layered::compose(
                std::path::Path::new(request),
                std::path::Path::new(bindings),
                std::path::Path::new(output),
            )
            .map_err(|_| "layered-compose-refused-preserve-partial")
        }
        [command, path] if command == "check" => check(path),
        [command, path] if command == "comic-issue-check" => check_issue(path),
        #[cfg(feature = "comic")]
        [command, request, output] if command == "comic-issue-assemble" => {
            reel_publication::comic_issue::assemble(
                std::path::Path::new(request),
                std::path::Path::new(output),
            )
            .map(|_| ())
            .map_err(|_| "comic-issue-assembly-refused-preserve-partial")
        }
        #[cfg(feature = "comic")]
        [command, request, output] if command == "comic-render" => reel_publication::comic::render(
            std::path::Path::new(request),
            std::path::Path::new(output),
        )
        .map(|_| ())
        .map_err(|_| "comic-render-refused-preserve-partial"),
        #[cfg(feature = "comic")]
        [command, request, packet] if command == "comic-check" => reel_publication::comic::check(
            std::path::Path::new(request),
            std::path::Path::new(packet),
        )
        .map(|_| ())
        .map_err(|_| "comic-check-refused"),
        #[cfg(feature = "comic")]
        [command, packet, output, pin] if command == "comic-restore" => {
            reel_publication::comic::restore_sources(
                std::path::Path::new(packet),
                std::path::Path::new(output),
                pin,
            )
            .map_err(|_| "comic-restore-refused-preserve-partial")
        }
        [command, ..] if command.starts_with("comic-") => {
            Err("unsupported-command-or-comic-feature-disabled")
        }
        _ => Err("command-arguments"),
    };
    match result {
        Ok(()) => {
            let metadata = matches!(args.first(),Some(command) if command=="check" || command=="comic-issue-check" || command=="bbj-prepare");
            println!(
                "{}",
                serde_json::json!({"ok":true,"scope":if metadata {"publication-metadata-only"} else {"explicit-private-comic-operation"},
                "payloads_verified":!metadata,"rendered":matches!(args.first(),Some(command) if command=="comic-render" || command=="comic-check" || command=="comic-issue-assemble"),
                "outputs_written":matches!(args.first(),Some(command) if command=="comic-render" || command=="comic-restore" || command=="comic-issue-assemble" || command=="layered-compose" || command=="bbj-prepare" || command=="layered-compose-book"),
                "selected":false,"print_ready":false,"retained_owner_acknowledged":false})
            );
            std::process::ExitCode::SUCCESS
        }
        Err(code) => {
            println!("{}", serde_json::json!({"ok":false,"error":{"code":code}}));
            std::process::ExitCode::FAILURE
        }
    }
}

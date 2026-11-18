//! Publication metadata checks and explicitly requested private comic operations.
use reel_publication::{MAX_CONTROL_BYTES, Publication};
use std::{fs::File, io::Read};

fn check(path: &str) -> Result<(), &'static str> {
    let file = File::open(path).map_err(|_| "manifest-open")?;
    let mut bytes = Vec::new();
    file.take(MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "manifest-read")?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return Err("manifest-bytes");
    }
    let publication: Publication = serde_json::from_slice(&bytes).map_err(|_| "manifest-json")?;
    reel_publication::check_publication(&publication).map_err(|e| e.code)
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args.as_slice(),[arg] if arg == "-h" || arg == "--help") {
        println!(
            "reel-publication check MANIFEST\nreel-publication comic-render REQUEST NEW_DIRECTORY\nreel-publication comic-check REQUEST PACKET\nreel-publication comic-restore PACKET NEW_DIRECTORY RECEIPT_SHA256\nMetadata check is read-only. Comic commands perform explicit private production/check/restore, without native retention, selection or release."
        );
        return std::process::ExitCode::SUCCESS;
    }
    let result = match args.as_slice() {
        [command, path] if command == "check" => check(path),
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
            let metadata = matches!(args.first(),Some(command) if command=="check");
            println!(
                "{}",
                serde_json::json!({"ok":true,"scope":if metadata {"publication-metadata-only"} else {"explicit-private-comic-operation"},
                "payloads_verified":!metadata,"rendered":matches!(args.first(),Some(command) if command=="comic-render" || command=="comic-check"),
                "outputs_written":matches!(args.first(),Some(command) if command=="comic-render" || command=="comic-restore"),
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

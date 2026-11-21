//! Git external-command front door. No cache effects or provider startup on help.
use std::{
    env,
    ffi::OsStr,
    process::{Command, ExitCode},
};

const COMMANDS: &[&str] = &[
    "catalogue",
    "graph",
    "history",
    "lfs",
    "lifecycle",
    "metadata",
    "project",
    "propose",
    "publication",
    "query",
    "record",
    "resolve",
    "restore",
    "retirement",
    "sample",
    "source",
    "storage",
    "trigger",
];
fn help() {
    println!(
        "git-asset — asset relationships and lifecycle over git-annex\n\nUsage: git asset <command> [arguments]\n       git-asset <command> [arguments]\n\nCommands: {}\n\nUse 'git asset describe' for machine-readable discovery.\nGit versions authored decisions; git-annex manages file bytes and native transfers.\nThe current native adapter supports explicit guarded local reads; native publication\nand the complete retained lifecycle are still being qualified.\nBuild/install the workspace service binaries alongside this front door.",
        COMMANDS.join(", ")
    );
}
fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1);
    let Some(command) = arguments.next() else {
        help();
        return ExitCode::SUCCESS;
    };
    match command.to_str() {
        Some("help" | "--help" | "-h") => {
            help();
            ExitCode::SUCCESS
        }
        Some("version" | "--version" | "-V") => {
            println!("git-asset {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("describe") => {
            println!(
                "{}",
                serde_json::json!({
                    "schema":"git-asset.interface.experimental-1",
                    "product":"git-asset", "version":env!("CARGO_PKG_VERSION"),
                    "role":"asset relationship and lifecycle coordination over git-annex",
                    "backend":"git-annex", "experimental":true, "commands":COMMANDS,
                    "native_publication_qualified":false, "automatic_cache_operations":false
                })
            );
            ExitCode::SUCCESS
        }
        Some(service) if COMMANDS.contains(&service) => {
            let Some(directory) = env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_owned()))
            else {
                eprintln!("git-asset: cannot locate this installation");
                return ExitCode::from(2);
            };
            let executable =
                directory.join(format!("git-asset-{service}{}", env::consts::EXE_SUFFIX));
            match Command::new(executable).args(arguments).status() {
                Ok(status) => ExitCode::from(
                    status
                        .code()
                        .and_then(|n| u8::try_from(n).ok())
                        .unwrap_or(1),
                ),
                Err(_) => {
                    eprintln!(
                        "git-asset: service unavailable; build/install the workspace binaries together"
                    );
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            if command != OsStr::new("") {
                eprintln!("git-asset: unknown command; run git asset help");
            }
            ExitCode::from(2)
        }
    }
}

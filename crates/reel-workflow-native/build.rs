use std::{env, fs, path::PathBuf};

fn main() {
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../vendor/reel-workflow-lifecycle/src/tig_job_runner");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // The qualified module's `mod tests;` resolves beside a #[path] module.
    // Copy both exact source files into build output in that layout; do not
    // patch the pinned module or add unlisted source to its vendor directory.
    for (from, to) in [("slots.rs", "slots.rs"), ("slots/tests.rs", "tests.rs")] {
        let path = source.join(from);
        println!("cargo:rerun-if-changed={}", path.display());
        fs::copy(path, output.join(to)).unwrap();
    }
    fs::write(
        output.join("qualified_module.rs"),
        format!(
            "#[allow(dead_code)]\n#[path = {:?}]\npub(crate) mod slots;\n",
            output.join("slots.rs").to_str().unwrap()
        ),
    )
    .unwrap();
}

use reel_assets::protocol;
use std::{io, path::PathBuf};
fn main() {
    let result = (|| -> reel_assets::Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.len() != 2 || args[0] != "--root" {
            return Err("Usage: caminos-lfs --root <repository>".into());
        }
        let root = PathBuf::from(&args[1]).canonicalize()?;
        protocol(&root, io::stdin().lock(), io::stdout().lock())
    })();
    if let Err(error) = result {
        eprintln!("caminos-lfs: {error}");
        std::process::exit(1);
    }
}

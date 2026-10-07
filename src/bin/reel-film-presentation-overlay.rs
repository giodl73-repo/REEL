use anyhow::{Result, bail};
use std::path::Path;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let result = (|| -> Result<()> {
        if let [_, action, manifest, root_flag, root, output_flag, out] = args.as_slice() {
            if root_flag != "--root" || output_flag != "--output-dir" {
                bail!("invalid flags");
            }
            match action.as_str() {
                "build" => {
                    reel::film_presentation_overlay::build(
                        Path::new(manifest),
                        Path::new(root),
                        Path::new(out),
                    )?;
                }
                "check" => reel::film_presentation_overlay::check(
                    Path::new(manifest),
                    Path::new(root),
                    Path::new(out),
                )?,
                _ => bail!("expected build or check"),
            }
            println!("film presentation overlay {action} passed");
            Ok(())
        } else {
            bail!(
                "usage: reel-film-presentation-overlay <build|check> <manifest> --root <root> --output-dir <dir>"
            );
        }
    })();
    if let Err(e) = result {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}

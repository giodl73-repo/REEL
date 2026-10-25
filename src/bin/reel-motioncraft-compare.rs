use anyhow::{Result, bail};
use std::{env, path::Path};
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, request, output] if command == "render" => {
            reel::motioncraft_comparison::render(Path::new(request), Path::new(output))?;
        }
        [command, root] if command == "check" => {
            reel::motioncraft_comparison::check(Path::new(root))?;
        }
        _ => bail!(
            "usage: reel-motioncraft-compare render <request.json> <new-output> | check <package>"
        ),
    }
    Ok(())
}

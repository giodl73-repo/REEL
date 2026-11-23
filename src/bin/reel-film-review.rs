use anyhow::{Result, bail};
use reel::film_presentation_overlay::review;
use std::path::Path;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "reel-film-review <build|check> REQUEST SHA256 --root ROOT --output-dir OUT\nPrivate IDs over an existing scene-delivery review movie. No selection, issue submission or native retention."
        );
        return;
    }
    if args == ["capabilities"] {
        println!(
            "{}",
            serde_json::json!({"burned_cel_ids":true,"source":"scene-delivery-review","native_retention":false,"issue_submission":false,"episode_assembly":false,"creative_approval":false})
        );
        return;
    }
    let result = (|| -> Result<()> {
        let [action, request, pin, root_flag, root, out_flag, out] = args.as_slice() else {
            bail!(
                "usage: reel-film-review <build|check> REQUEST SHA256 --root ROOT --output-dir OUT"
            )
        };
        if root_flag != "--root" || out_flag != "--output-dir" {
            bail!("invalid review flags");
        }
        let request = review::request_reference(Path::new(request), pin)?;
        let files = review::Local {
            request: &request,
            root: Path::new(root),
        };
        let receipt = match action.as_str() {
            "build" => review::build_with_inputs(&request, &files, Path::new(out))?,
            "check" => review::check_with_inputs(&request, &files, Path::new(out))?,
            _ => bail!("expected build or check"),
        };
        println!(
            "{}",
            serde_json::json!({"ok":true,"frames":receipt.frame_count,"visual_reviewed":false,"native_retention_verified":false,"publication":"not-authorized"})
        );
        Ok(())
    })();
    if result.is_err() {
        eprintln!("film review refused; preserve any original output for diagnosis");
        std::process::exit(1);
    }
}

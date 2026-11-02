use std::io::{self, Read};
fn main() {
    let mut text = String::new();
    if let Err(error) = io::stdin()
        .take((reel_assembly_workbench::MAX_REQUEST_BYTES + 1) as u64)
        .read_to_string(&mut text)
    {
        eprintln!("UTF-8 input: {error}");
        std::process::exit(2);
    }
    if text.len() > reel_assembly_workbench::MAX_REQUEST_BYTES {
        eprintln!("Native request transport exceeds 2 MiB; nothing was truncated.");
        std::process::exit(2);
    }
    let request: reel_assembly_workbench::Request = match serde_json::from_str(&text) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("Native request JSON: {error}");
            std::process::exit(2);
        }
    };
    println!(
        "{}",
        reel_assembly_workbench::inspect_assembly(
            &request.graph_text,
            &request.target,
            &request.kind,
            &request.pointer_text
        )
    );
}

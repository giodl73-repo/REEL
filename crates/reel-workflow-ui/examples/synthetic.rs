#[path = "../tests/support/mod.rs"]
mod sample;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [command, path] = args.as_slice() else {
        panic!("synthetic create|advance|retain NEW_OR_EXISTING_SAMPLE_DIRECTORY")
    };
    let root = std::path::Path::new(path);
    assert!(root.is_absolute());
    match command.as_str() {
        "create" => {
            sample::create(root);
        }
        "advance" => sample::advance(root, false),
        "retain" => sample::advance(root, true),
        _ => panic!("unknown sample command"),
    };
    let pin = reel_workflow::digest(&std::fs::read(root.join("board.json")).unwrap());
    println!(
        "{}",
        serde_json::json!({"board":root.join("board.json"),"sha256":pin,"synthetic":true})
    );
}

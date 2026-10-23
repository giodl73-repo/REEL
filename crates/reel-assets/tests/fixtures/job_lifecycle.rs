// Synthetic executable compiled only inside a temporary test directory.
use std::{env, fs, process::Command, thread, time::Duration};
fn main() {
    let args: Vec<String> = env::args().collect();
    match args[1].as_str() {
        "copy" => { fs::copy(&args[2], &args[3]).unwrap(); }
        "mutate" => {
            fs::write(&args[2], b"changed").unwrap();
            fs::copy(&args[2], &args[3]).unwrap();
        }
        "fail" => {
            fs::write(&args[3], b"candidate").unwrap();
            fs::write("intermediate.bin", vec![0; 1024 * 1024]).unwrap();
            eprintln!("synthetic failure detail");
            std::process::exit(23);
        }
        "oversize" => {
            fs::write(&args[3], vec![0; 4096]).unwrap();
            std::process::exit(23);
        }
        "hold-copy" => {
            fs::write("started.pid", std::process::id().to_string()).unwrap();
            thread::sleep(Duration::from_secs(2));
            fs::copy(&args[2], &args[3]).unwrap();
        }
        "workspace-window" => {
            fs::write("transient.bin", vec![7; 20 * 1024 * 1024]).unwrap();
            thread::sleep(Duration::from_secs(2));
            fs::copy(&args[2], &args[3]).unwrap();
        }
        "unlink-originals" => {
            for p in &args[4..] { fs::remove_file(p).unwrap(); }
            fs::copy(&args[2], &args[3]).unwrap();
        }
        "tree" => {
            fs::write("parent.pid", std::process::id().to_string()).unwrap();
            let mut child = Command::new(env::current_exe().unwrap()).arg("sleep").spawn().unwrap();
            let _ = child.wait();
        }
        "sleep" => {
            fs::write("child.pid", std::process::id().to_string()).unwrap();
            loop { thread::sleep(Duration::from_secs(1)); }
        }
        _ => panic!("unknown synthetic mode"),
    }
}

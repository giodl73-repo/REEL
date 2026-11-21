use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
};
use tig_git::{
    cache::{CacheBackend, Profile},
    protocol::{self, AgentError},
};
fn main() {
    let result = (|| {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.len() != 2 || args[0] != "--profile" {
            return Err(AgentError::InvalidProfile);
        }
        let path = PathBuf::from(&args[1]);
        if !path.is_absolute() {
            return Err(AgentError::InvalidProfile);
        }
        let mut bytes = Vec::new();
        File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
        let mut backend = CacheBackend::new(Profile::from_json(&bytes)?)?;
        protocol::serve(io::stdin().lock(), io::stdout().lock(), &mut backend)
    })();
    if let Err(error) = result {
        eprintln!("tig-lfs: {error}");
        std::process::exit(1);
    }
}

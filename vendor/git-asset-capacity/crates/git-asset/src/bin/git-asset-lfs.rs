use git_asset::{
    cache::{CacheBackend, Profile},
    protocol::{self, AgentError},
};
use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
};
#[cfg(feature = "upload")]
#[path = "git-asset-lfs/upload.rs"]
mod upload;
fn main() {
    let result = (|| {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        #[cfg(feature = "upload")]
        if args.first().is_some_and(|a| a == "upload") {
            return upload::cli(&args[1..]);
        }
        #[cfg(feature = "upload")]
        if args.len() == 2 && args[0] == "--upload-profile" {
            let path = PathBuf::from(&args[1]);
            let profile = git_asset::upload::Profile::from_json(&upload::read(&path)?)
                .map_err(|_| AgentError::InvalidProfile)?;
            let mut backend =
                git_asset::upload::Backend::new(profile).map_err(|_| AgentError::InvalidProfile)?;
            return protocol::serve_upload(io::stdin().lock(), io::stdout().lock(), &mut backend);
        }
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
        eprintln!("git-asset-lfs: {error}");
        std::process::exit(1);
    }
}

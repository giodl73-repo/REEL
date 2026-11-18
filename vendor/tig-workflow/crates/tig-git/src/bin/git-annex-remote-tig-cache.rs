use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
};
use tig_git::annex::legacy::{Backend, Profile, serve};

fn main() {
    if run().is_err() {
        // Protocol errors never reveal source paths or underlying OS messages.
        eprintln!("tig-cache: unavailable");
        std::process::exit(1);
    }
}
fn run() -> io::Result<()> {
    let path = PathBuf::from(
        std::env::var_os("TIG_ANNEX_CACHE_PROFILE")
            .ok_or_else(|| io::Error::other("missing trusted enrollment"))?,
    );
    if !path.is_absolute() {
        return Err(io::Error::other("invalid enrollment"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(io::Error::other("profile budget"));
    }
    let value = tig_core::identity::parse_json(&bytes).map_err(io::Error::other)?;
    let profile: Profile = serde_json::from_value(value).map_err(io::Error::other)?;
    let mut backend = Backend::new(profile)?;
    serve(io::stdin().lock(), io::stdout().lock(), &mut backend)
}

use tig_core::Digest;
use tig_store::Key;
pub fn fresh_operation(prefix: &str) -> Result<Key, String> {
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).map_err(|_| "cannot allocate a fresh probe identity")?;
    Key::parse(&format!("{prefix}-{}", Digest::blob(&nonce).as_str()))
        .map_err(|_| "cannot encode fresh probe identity".into())
}

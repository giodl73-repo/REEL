use tig_core::{Digest, Object};
const CURRENT: &str = "https://git-lfs.github.com/spec/v1";
const LEGACY: &str = "https://hawser.github.com/spec/v1";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pointer {
    version: &'static str,
    object: Object,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PointerError {
    Budget,
    Malformed,
    Unsupported,
}
impl Pointer {
    pub fn new(object: Object) -> Self {
        Self {
            version: CURRENT,
            object,
        }
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, PointerError> {
        if bytes.len() > 1024 {
            return Err(PointerError::Budget);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| PointerError::Malformed)?;
        let lines: Vec<_> = text.split('\n').collect();
        if lines.len() != 4 || !lines[3].is_empty() || text.contains('\r') {
            return Err(PointerError::Malformed);
        }
        let version = match lines[0].strip_prefix("version ") {
            Some(CURRENT) => CURRENT,
            Some(LEGACY) => LEGACY,
            _ => return Err(PointerError::Unsupported),
        };
        let oid = lines[1]
            .strip_prefix("oid sha256:")
            .ok_or(PointerError::Unsupported)?;
        let sha256 = Digest::parse(oid).map_err(|_| PointerError::Malformed)?;
        let size = lines[2]
            .strip_prefix("size ")
            .ok_or(PointerError::Malformed)?;
        let count = size.parse::<u64>().map_err(|_| PointerError::Malformed)?;
        if count.to_string() != size {
            return Err(PointerError::Malformed);
        }
        Ok(Self {
            version,
            object: Object {
                sha256,
                bytes: count,
            },
        })
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        format!(
            "version {}\noid sha256:{}\nsize {}\n",
            self.version,
            self.object.sha256.as_str(),
            self.object.bytes
        )
        .into_bytes()
    }
}

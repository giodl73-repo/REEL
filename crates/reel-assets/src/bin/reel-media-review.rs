//! Freeze explicitly selected local media for the bounded native review host.
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    label: String,
    language: String,
    kind: String,
    source: Source,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    title: String,
    notice: String,
    items: Vec<Item>,
}
fn export(input: &Path, output: &Path) -> Result<serde_json::Value> {
    let mut raw = Vec::new();
    fs::File::open(input)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > 1024 * 1024 {
        return Err("Review request exceeds budget".into());
    }
    let request: Request = serde_json::from_slice(&raw)?;
    if request.schema != "reel.media-review.v1"
        || request.items.is_empty()
        || request.items.len() > 16
        || request.title.len() > 512
        || request.notice.len() > 4096
    {
        return Err("Invalid review scope".into());
    }
    if !output.is_absolute() || output.exists() || !output.parent().is_some_and(Path::is_dir) {
        return Err("New absolute output directory required".into());
    }
    let mut ids = BTreeSet::new();
    let mut total = 0u64;
    let mut loaded = Vec::new();
    for item in request.items {
        if item.id.is_empty()
            || item.id.len() > 128
            || !ids.insert(item.id.clone())
            || item.label.len() > 512
            || item.language.len() > 32
            || !item.source.path.is_absolute()
        {
            return Err("Invalid review item".into());
        }
        let extension = item
            .source
            .path
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("Missing media extension")?;
        if !matches!(
            (item.kind.as_str(), extension),
            ("video", "mp4") | ("audio", "wav" | "mp3" | "m4a") | ("image", "png" | "jpg" | "jpeg")
        ) {
            return Err("Unsupported review media kind/extension".into());
        }
        let metadata = fs::symlink_metadata(&item.source.path)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != item.source.bytes
            || metadata.len() > 128 * 1024 * 1024
        {
            return Err("Invalid review source".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Review source cannot be a reparse point".into());
            }
        }
        total = total
            .checked_add(metadata.len())
            .ok_or("Media budget overflow")?;
        if total > 256 * 1024 * 1024 {
            return Err("Review media exceeds budget".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(&item.source.path)?
            .take(item.source.bytes + 1)
            .read_to_end(&mut bytes)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if bytes.len() as u64 != item.source.bytes || hash != item.source.sha256 {
            return Err("Review source hash mismatch".into());
        }
        let route = format!("media/{hash}.{extension}");
        loaded.push((json!({"id":item.id,"label":item.label,"language":item.language,"kind":item.kind,"src":route,"sha256":hash,"bytes":bytes.len()}),bytes));
    }
    fs::create_dir(output)?;
    fs::create_dir(output.join("media"))?;
    for (item, bytes) in &loaded {
        let path = output.join(item["src"].as_str().unwrap());
        if !path.exists() {
            fs::write(path, bytes)?;
        }
    }
    fs::write(
        output.join("index.html"),
        include_bytes!("../media_review.html"),
    )?;
    fs::write(
        output.join("editor.js"),
        include_bytes!("../media_review.js"),
    )?;
    fs::write(
        output.join("session.json"),
        serde_json::to_vec(
            &json!({"schema":"reel.media-review-session.v1","title":request.title,"notice":request.notice,"items":loaded.iter().map(|(i,_)|i).collect::<Vec<_>>() }),
        )?,
    )?;
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec(
            &json!({"schema":"reel.media-review-export.v1","request_sha256":format!("{:x}",Sha256::digest(&raw)),"items":loaded.len(),"media_bytes":total,"human_approval":false,"public_release":false}),
        )?,
    )?;
    Ok(json!({"items":loaded.len(),"media_bytes":total,"human_approval":false}))
}
fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let result = if args.len() == 2 {
        export(Path::new(&args[0]), Path::new(&args[1]))
    } else {
        Err("Usage: reel-media-review REQUEST NEW_ABSOLUTE_OUTPUT".into())
    };
    match result {
        Ok(v) => println!("{v}"),
        Err(e) => {
            eprintln!("reel-media-review: {e}");
            std::process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_media_export_and_changed_source_refusal() {
        let d = tempfile::tempdir().unwrap();
        let media = d.path().join("test.mp4");
        fs::write(&media, b"fixture").unwrap();
        let request = d.path().join("request.json");
        let hash = format!("{:x}", Sha256::digest(b"fixture"));
        fs::write(&request,serde_json::to_vec(&json!({"schema":"reel.media-review.v1","title":"Test","notice":"Private","items":[{"id":"one","label":"First","language":"es","kind":"video","source":{"path":media,"sha256":hash,"bytes":7}}]})).unwrap()).unwrap();
        let out = d.path().join("export");
        export(&request, &out).unwrap();
        let session: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("session.json")).unwrap()).unwrap();
        assert!(
            session["items"][0]["src"]
                .as_str()
                .unwrap()
                .ends_with(".mp4")
        );
        assert!(
            !fs::read_to_string(out.join("session.json"))
                .unwrap()
                .contains("test.mp4")
        );
        assert!(export(&request, &out).is_err());
        fs::write(&media, b"changed").unwrap();
        let refused = d.path().join("refused");
        assert!(export(&request, &refused).is_err());
        assert!(!refused.exists());
    }
}

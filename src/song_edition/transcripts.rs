//! Read existing bounded ZIP32 transcript evidence in memory; never extract files.
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn u16le(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes(b[n..n + 2].try_into().unwrap())
}
fn u32le(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(b[n..n + 4].try_into().unwrap())
}
fn crc32(raw: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in raw {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ ((0u32.wrapping_sub(crc & 1)) & 0xedb88320)
        }
    }
    !crc
}
fn archive(raw: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    if raw.len() < 22 || raw.len() > 16 * 1024 * 1024 {
        bail!("transcript ZIP byte budget")
    }
    let end = (raw.len().saturating_sub(65_557)..=raw.len() - 22)
        .rev()
        .find(|i| {
            &raw[*i..*i + 4] == b"PK\x05\x06"
                && *i + 22 + usize::from(u16le(raw, *i + 20)) == raw.len()
        })
        .ok_or_else(|| anyhow::anyhow!("ZIP32 end missing"))?;
    let entries = usize::from(u16le(raw, end + 10));
    let directory = u32le(raw, end + 16) as usize;
    if u16le(raw, end + 4) != 0
        || u16le(raw, end + 6) != 0
        || u16le(raw, end + 8) as usize != entries
        || entries == 0
        || entries > 64
        || directory.checked_add(u32le(raw, end + 12) as usize) != Some(end)
    {
        bail!("ZIP32 directory budget")
    }
    let mut at = directory;
    let mut total = 0usize;
    let mut files = BTreeMap::new();
    for _ in 0..entries {
        let head = raw
            .get(at..at + 46)
            .ok_or_else(|| anyhow::anyhow!("ZIP directory truncated"))?;
        if &head[..4] != b"PK\x01\x02" {
            bail!("ZIP central signature")
        }
        let flags = u16le(head, 8);
        let method = u16le(head, 10);
        let crc = u32le(head, 16);
        let compressed = u32le(head, 20) as usize;
        let size = u32le(head, 24) as usize;
        let name_len = usize::from(u16le(head, 28));
        let extra = usize::from(u16le(head, 30));
        let comment = usize::from(u16le(head, 32));
        let local = u32le(head, 42) as usize;
        total = total
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("transcript ZIP overflow"))?;
        if flags & !0x080e != 0
            || !matches!(method, 0 | 8)
            || total > 16 * 1024 * 1024
            || name_len == 0
            || name_len > 1024
            || u16le(head, 34) != 0
        {
            bail!("ZIP compression/entry budget")
        }
        let name_raw = raw
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| anyhow::anyhow!("ZIP name truncated"))?;
        let name = std::str::from_utf8(name_raw)?;
        if name.contains('\\')
            || name.starts_with('/')
            || name.split('/').any(|p| p == ".." || p == ".")
            || name.chars().any(char::is_control)
        {
            bail!("invalid transcript member path")
        }
        at = at
            .checked_add(46 + name_len + extra + comment)
            .ok_or_else(|| anyhow::anyhow!("ZIP central size"))?;
        if at > end {
            bail!("ZIP central entry overflow")
        }
        if local >= directory {
            bail!("ZIP local offset outside payload area")
        }
        let local_head = raw
            .get(local..local + 30)
            .ok_or_else(|| anyhow::anyhow!("ZIP local header"))?;
        if &local_head[..4] != b"PK\x03\x04"
            || u16le(local_head, 6) != flags
            || u16le(local_head, 8) != method
        {
            bail!("ZIP local header differs")
        }
        let local_name = usize::from(u16le(local_head, 26));
        let local_extra = usize::from(u16le(local_head, 28));
        if local_name != name_len || raw.get(local + 30..local + 30 + local_name) != Some(name_raw)
        {
            bail!("ZIP local name differs")
        }
        let start = local
            .checked_add(30 + local_name + local_extra)
            .ok_or_else(|| anyhow::anyhow!("ZIP local overflow"))?;
        let stop = start
            .checked_add(compressed)
            .filter(|n| *n <= directory)
            .ok_or_else(|| anyhow::anyhow!("ZIP payload bounds"))?;
        let payload = raw
            .get(start..stop)
            .ok_or_else(|| anyhow::anyhow!("ZIP payload missing"))?;
        if flags & 8 == 0
            && (u32le(local_head, 14) != crc
                || u32le(local_head, 18) as usize != compressed
                || u32le(local_head, 22) as usize != size)
        {
            bail!("ZIP local sizes/CRC differ")
        }
        let bytes = if method == 0 {
            payload.to_vec()
        } else {
            let mut decoder = flate2::Decompress::new(false);
            let mut bytes = vec![0u8; size + 1];
            let status =
                decoder.decompress(payload, &mut bytes, flate2::FlushDecompress::Finish)?;
            if status != flate2::Status::StreamEnd
                || decoder.total_in() != compressed as u64
                || decoder.total_out() != size as u64
            {
                bail!("ZIP deflate stream differs")
            };
            bytes.truncate(size);
            bytes
        };
        if bytes.len() != size
            || crc32(&bytes) != crc
            || files.insert(name.to_owned(), bytes).is_some()
        {
            bail!("ZIP member CRC, length or duplicate name")
        }
    }
    if at != end {
        bail!("ZIP directory trailing data")
    }
    Ok(files)
}

pub(super) fn context(raw: &[u8], passes: &[Value], prefix: &str) -> Result<Value> {
    if prefix.len() > 1024 || (!prefix.is_empty() && !prefix.ends_with('/')) {
        bail!("transcript prefix")
    }
    let files = archive(raw)?;
    let mut conditions = Vec::new();
    for pass in passes {
        let name = pass["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("original pass name"))?;
        let bytes = files
            .get(&format!("{prefix}{name}.json"))
            .ok_or_else(|| anyhow::anyhow!("original transcript member missing"))?;
        let transcript = super::packet::parse(bytes)?;
        if transcript["canonical_prompt"] != false {
            bail!("original transcript is not explicitly unprompted")
        }
        let chunks = transcript["chunks"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("original transcript chunks missing"))?;
        let mut observed = Vec::new();
        for chunk in chunks {
            let text = chunk["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("transcript text"))?;
            let span = &chunk["timestamp"];
            if span.as_array().is_none_or(|s| s.len() != 2) {
                bail!("transcript timestamp")
            }
            for word in super::lyrics::words(text) {
                observed.push(json!({"word":word,"span":span}));
            }
        }
        let original = pass["tokens"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("original transcript tokens"))?;
        if observed.len() != original.len()
            || observed
                .iter()
                .zip(original)
                .any(|(a, b)| a["word"] != b["word"] || a["span"] != b["span"])
        {
            bail!("analysis words/spans differ from original raw transcript")
        }
        let mut metadata = transcript;
        let object = metadata.as_object_mut().unwrap();
        object.remove("chunks");
        object.remove("text");
        conditions.push(json!({"name":name,"metadata":metadata}));
    }
    Ok(
        json!({"kind":"original-pinned-transcript-archive","conditions":conditions,"new_recognition":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn zip(name: &str, data: &[u8], deflate: bool) -> Vec<u8> {
        let payload = if deflate {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        } else {
            data.to_vec()
        };
        let crc = crc32(data);
        let method: u16 = if deflate { 8 } else { 0 };
        let mut out = b"PK\x03\x04".to_vec();
        out.extend(20u16.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(method.to_le_bytes());
        out.extend([0; 4]);
        out.extend(crc.to_le_bytes());
        out.extend((payload.len() as u32).to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name.as_bytes());
        out.extend(&payload);
        let directory = out.len();
        out.extend(b"PK\x01\x02");
        out.extend([20, 0, 20, 0, 0, 0]);
        out.extend(method.to_le_bytes());
        out.extend([0; 4]);
        out.extend(crc.to_le_bytes());
        out.extend((payload.len() as u32).to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((name.len() as u16).to_le_bytes());
        out.extend([0; 16]);
        out.extend(name.as_bytes());
        let size = out.len() - directory;
        out.extend(b"PK\x05\x06");
        out.extend([0; 4]);
        out.extend([1, 0, 1, 0]);
        out.extend((size as u32).to_le_bytes());
        out.extend((directory as u32).to_le_bytes());
        out.extend([0; 2]);
        out
    }
    #[test]
    fn stored_and_deflated_original_transcripts_bind_exact_words_and_clocks() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        let transcript = json!({"canonical_prompt":false,"source_clock_offset_seconds":0,"chunks":[{"text":"Álpha beta","timestamp":[0.0,1.0]}]});
        let pass = json!({"name":"mix","tokens":[{"word":"alpha","span":[0.0,1.0]},{"word":"beta","span":[0.0,1.0]}]});
        for compressed in [false, true] {
            let raw = zip(
                "mix.json",
                &serde_json::to_vec(&transcript).unwrap(),
                compressed,
            );
            assert_eq!(
                context(&raw, std::slice::from_ref(&pass), "").unwrap()["new_recognition"],
                false
            );
            let mut bad = pass.clone();
            bad["tokens"][0]["word"] = json!("different");
            assert!(context(&raw, &[bad], "").is_err());
            let mut bad = raw;
            bad[30 + 8] ^= 1;
            assert!(context(&bad, std::slice::from_ref(&pass), "").is_err());
        }
    }
    #[test]
    fn prompt_conditioned_or_escaping_members_refuse() {
        let raw = zip(
            "mix.json",
            br#"{"canonical_prompt":true,"chunks":[]}"#,
            false,
        );
        assert!(context(&raw, &[json!({"name":"mix","tokens":[]})], "").is_err());
        assert!(archive(&zip("../mix.json", b"{}", false)).is_err());
    }
}

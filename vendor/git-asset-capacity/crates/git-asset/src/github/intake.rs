//! Strict one-request loopback transport. TLS ingress/OS isolation is external.
use super::*;
use git_asset_store::placement::Cursor;
use std::{
    io::{self, Read, Write},
    net::TcpStream,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    schema: String,
    commit: String,
    cursor: Option<Cursor>,
}
struct Input {
    bearer: String,
    commit: GitId,
    cursor: Option<Cursor>,
}
fn read(stream: &mut TcpStream) -> Result<Input> {
    if !stream
        .peer_addr()
        .map_err(|_| Error::Unavailable)?
        .ip()
        .is_loopback()
    {
        return Err(Error::Trust);
    }
    let start = Instant::now();
    let limit = Duration::from_secs(5);
    let mut bytes = Vec::new();
    let mut byte = [0; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 24576 {
            return Err(Error::Budget);
        }
        let remaining = limit.checked_sub(start.elapsed()).ok_or(Error::Budget)?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| Error::Unavailable)?;
        stream
            .read_exact(&mut byte)
            .map_err(|_| Error::Unavailable)?;
        bytes.push(byte[0]);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::Invalid)?;
    let mut lines = text.split("\r\n");
    if lines.next() != Some("POST /v1/reconcile HTTP/1.1") {
        return Err(Error::Invalid);
    }
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(Error::Invalid)?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.bytes().any(|b| b < 32 && b != b'\t' || b == 127)
        {
            return Err(Error::Invalid);
        }
        if headers
            .insert(name.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err(Error::Invalid);
        }
    }
    if !headers.contains_key("host")
        || headers.contains_key("transfer-encoding")
        || headers.contains_key("expect")
        || headers
            .get("content-encoding")
            .is_some_and(|s| s != "identity")
        || headers
            .get("content-type")
            .is_none_or(|s| s.split(';').next() != Some("application/json"))
    {
        return Err(Error::Invalid);
    }
    let length = headers.get("content-length").ok_or(Error::Invalid)?;
    let count = length.parse::<usize>().map_err(|_| Error::Invalid)?;
    if count.to_string() != *length {
        return Err(Error::Invalid);
    }
    if count > 2048 {
        return Err(Error::Budget);
    }
    let bearer = headers
        .get("authorization")
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(Error::Claims)?;
    if bearer.is_empty() || bearer.len() > MAX_TOKEN || bearer.bytes().any(|b| b <= 32 || b >= 127)
    {
        return Err(Error::Claims);
    }
    let mut body = vec![0; count];
    let mut position = 0;
    while position < body.len() {
        let remaining = limit.checked_sub(start.elapsed()).ok_or(Error::Budget)?;
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| Error::Unavailable)?;
        let n = stream
            .read(&mut body[position..])
            .map_err(|_| Error::Unavailable)?;
        if n == 0 {
            return Err(Error::Invalid);
        }
        position += n;
    }
    let body: Body = serde_json::from_value(json(&body)?).map_err(|_| Error::Invalid)?;
    if body.schema != "tig.github-request.design-1" {
        return Err(Error::Invalid);
    }
    Ok(Input {
        bearer: bearer.into(),
        commit: GitId::parse(&body.commit).map_err(|_| Error::Invalid)?,
        cursor: body.cursor,
    })
}
/// Dispatch is called only after exact signed intake succeeds. Callers open the
/// private source/provider profiles inside that callback, never from request data.
pub fn handle(
    mut stream: TcpStream,
    trust: &Trust,
    keys: &KeySnapshot,
    mut now: impl FnMut() -> Result<u64>,
    dispatch: impl FnOnce(Verified, Option<Cursor>) -> Result<serde_json::Value>,
) -> io::Result<()> {
    let admitted = read(&mut stream).and_then(|input| {
        let verified = verify(trust, keys, &input.bearer, &input.commit, now()?)?;
        Ok((verified, input.cursor))
    });
    let (authenticated, result) = match admitted {
        Ok((proof, cursor)) => (true, dispatch(proof, cursor)),
        Err(e) => (false, Err(e)),
    };
    let (status, value) = match result {
        Ok(value) => (
            200,
            serde_json::json!({"schema":"tig.github-response.design-1","authenticated":true,"ok":true,"result":value}),
        ),
        Err(error) => {
            let (status, code) = match error {
                Error::Invalid => (400, "invalid_request"),
                Error::Budget => (413, "budget"),
                Error::Unavailable => (503, "unavailable"),
                Error::Disabled => (403, "disabled"),
                Error::Stale => (409, "stale"),
                _ if authenticated => (403, "authority"),
                _ => (401, "unauthorized"),
            };
            (
                status,
                serde_json::json!({"schema":"tig.github-response.design-1","authenticated":authenticated,"ok":false,"code":code}),
            )
        }
    };
    let body = identity::canonical_json(&value).map_err(|_| io::Error::other("response"))?;
    if body.len() > 1024 * 1024 {
        return Err(io::Error::other("response budget"));
    }
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "HTTP/1.1 {status} git-asset\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::fixture::Fixture;
    use std::{
        net::TcpListener,
        sync::atomic::{AtomicUsize, Ordering},
        thread,
    };
    #[test]
    fn malformed_or_unauthenticated_http_never_dispatches_private_source_or_provider_work() {
        let f = Fixture::new();
        let token = f.token(&f.claims);
        let valid =
            serde_json::json!({"schema":"tig.github-request.design-1","commit":f.commit.as_str()})
                .to_string();
        for variant in 0..6 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (incoming, _) = listener.accept().unwrap();
            let calls = AtomicUsize::new(0);
            let mut body = valid.clone();
            let mut headers = String::new();
            let mut bearer = token.clone();
            let mut length = body.len();
            match variant {
                0 => headers = format!("Content-Length: {length}\r\n"),
                1 => headers = "Transfer-Encoding: chunked\r\n".into(),
                2 => headers = format!("Authorization: Bearer {token}\r\n"),
                3 => {
                    body=serde_json::json!({"schema":"tig.github-request.design-1","commit":f.commit.as_str(),"policy":{"allow_eviction":true}}).to_string();
                    length = body.len();
                }
                4 => bearer = "not-a-signed-token".into(),
                _ => {
                    length = 2049;
                    body.clear();
                }
            }
            thread::scope(|scope| {
                let task = scope.spawn(|| {
                    handle(
                        incoming,
                        &f.trust,
                        &f.keys,
                        || Ok(1001),
                        |_, _| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Ok(serde_json::json!({"unexpected":true}))
                        },
                    )
                });
                write!(client,"POST /v1/reconcile HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAuthorization: Bearer {bearer}\r\nContent-Length: {length}\r\n{headers}\r\n{body}").unwrap();
                let mut response = String::new();
                let read = client.read_to_string(&mut response);
                task.join().unwrap().unwrap();
                assert!(!response.starts_with("HTTP/1.1 200"));
                assert!(!response.contains(&token));
                if let Err(error) = read {
                    // Refusing an ambiguous header before consuming its body
                    // may reset the connection on Windows. It never dispatches.
                    assert!(variant <= 2 && error.kind() == io::ErrorKind::ConnectionReset);
                    return;
                }
                let value: serde_json::Value =
                    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
                assert_eq!(value["authenticated"], false);
                assert_eq!(value["ok"], false);
            });
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }
}

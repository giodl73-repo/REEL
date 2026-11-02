//! Existing CDP websocket only; does not launch, discover or admit a browser.
use anyhow::{Result, bail};
use std::{
    net::TcpStream,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig, stream::MaybeTlsStream};

/// CDP is a wire protocol, not Tig identity metadata: real events carry f64
/// timestamps. Retain duplicate-key rejection without weakening the asset reader.
pub(crate) fn parse(raw: &str) -> Result<serde_json::Value> {
    use serde::Deserialize;
    struct Wire(serde_json::Value);
    impl<'de> Deserialize<'de> for Wire {
        fn deserialize<D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> std::result::Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Wire;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    write!(f, "bounded duplicate-free CDP JSON")
                }
                fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Wire, E> {
                    Ok(Wire(v.into()))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Wire, E> {
                    Ok(Wire(v.into()))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Wire, E> {
                    Ok(Wire(v.into()))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Wire, E> {
                    Ok(Wire(serde_json::Value::Number(
                        serde_json::Number::from_f64(v)
                            .ok_or_else(|| E::custom("nonfinite CDP number"))?,
                    )))
                }
                fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Wire, E> {
                    Ok(Wire(v.into()))
                }
                fn visit_string<E: serde::de::Error>(
                    self,
                    v: String,
                ) -> std::result::Result<Wire, E> {
                    Ok(Wire(v.into()))
                }
                fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Wire, E> {
                    Ok(Wire(serde_json::Value::Null))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> std::result::Result<Wire, A::Error> {
                    let mut values = Vec::new();
                    while let Some(value) = seq.next_element::<Wire>()? {
                        values.push(value.0);
                    }
                    Ok(Wire(values.into()))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> std::result::Result<Wire, A::Error> {
                    use serde::de::Error;
                    let mut values = serde_json::Map::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if values.contains_key(&key) {
                            return Err(A::Error::custom("duplicate CDP key"));
                        }
                        values.insert(key, map.next_value::<Wire>()?.0);
                    }
                    Ok(Wire(values.into()))
                }
            }
            deserializer.deserialize_any(Visitor)
        }
    }
    Ok(serde_json::from_str::<Wire>(raw)?.0)
}

/// Trusted bounded transport. Implementations must enforce the supplied deadline
/// and receive size while reading, before allocating an unbounded message.
pub trait Transport {
    fn send(&mut self, text: &str, remaining: Duration) -> Result<()>;
    fn receive(&mut self, max_bytes: usize, remaining: Duration) -> Result<String>;
}
pub struct WebSocketTransport {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    max_bytes: usize,
}
impl WebSocketTransport {
    /// An already-connected local socket, owned by an admitted runtime. Configure
    /// before any protocol reads; no URL, network discovery or PATH fallback.
    pub fn new(mut socket: WebSocket<MaybeTlsStream<TcpStream>>, max_bytes: usize) -> Result<Self> {
        if !(1024..=128 * 1024 * 1024).contains(&max_bytes)
            || !matches!(socket.get_ref(), MaybeTlsStream::Plain(_))
        {
            bail!("unsupported browser transport admission");
        }
        if let MaybeTlsStream::Plain(stream) = socket.get_ref()
            && !stream.peer_addr()?.ip().is_loopback()
        {
            bail!("browser control socket is not local");
        }
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream.set_nonblocking(true)?;
        }
        socket.set_config(|config| {
            *config = WebSocketConfig::default()
                .read_buffer_size(8192)
                .write_buffer_size(0)
                .max_write_buffer_size(max_bytes + 8192)
                .max_message_size(Some(max_bytes))
                .max_frame_size(Some(max_bytes));
        });
        Ok(Self { socket, max_bytes })
    }
    fn deadline(&self, remaining: Duration) -> Result<Instant> {
        if remaining.is_zero() {
            bail!("browser transport deadline elapsed");
        }
        Instant::now()
            .checked_add(remaining)
            .ok_or_else(|| anyhow::anyhow!("browser deadline overflow"))
    }
}
impl Transport for WebSocketTransport {
    fn send(&mut self, text: &str, remaining: Duration) -> Result<()> {
        if text.len() > self.max_bytes {
            bail!("browser transport command exceeds bound");
        }
        let deadline = self.deadline(remaining)?;
        match self.socket.send(Message::Text(text.to_owned().into())) {
            Ok(()) => return Ok(()),
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(e) => return Err(e.into()),
        }
        // WouldBlock leaves the frame buffered in tungstenite; flush that frame
        // rather than retrying send (which would duplicate the command).
        loop {
            pause(deadline)?;
            match self.socket.flush() {
                Ok(()) => return Ok(()),
                Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e.into()),
            }
        }
    }
    fn receive(&mut self, max_bytes: usize, remaining: Duration) -> Result<String> {
        if max_bytes != self.max_bytes {
            bail!("browser transport bounds differ");
        }
        let deadline = self.deadline(remaining)?;
        // Ping, binary and close frames fail rather than looping without charging
        // the session message budget or extending its deadline.
        loop {
            if Instant::now() >= deadline {
                bail!("browser transport deadline elapsed");
            }
            match self.socket.read() {
                Ok(Message::Text(text)) if text.len() <= max_bytes => return Ok(text.to_string()),
                Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    pause(deadline)?
                }
                Err(e) => return Err(e.into()),
                _ => bail!("unexpected browser control message"),
            }
        }
    }
}
fn pause(deadline: Instant) -> Result<()> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|v| !v.is_zero())
        .ok_or_else(|| anyhow::anyhow!("browser transport deadline elapsed"))?;
    std::thread::sleep(remaining.min(Duration::from_millis(1)));
    Ok(())
}

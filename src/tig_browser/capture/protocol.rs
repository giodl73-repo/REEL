//! Existing CDP websocket only; does not launch, discover or admit a browser.
use anyhow::{Result, bail};
use std::{
    net::TcpStream,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig, stream::MaybeTlsStream};

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

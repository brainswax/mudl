//! IRC transport — [`GameTransport`] mapping plus protocol-specific raw lines.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use crate::transport::GameTransport;

use super::message::format_outgoing;

/// IRC-specific outgoing action (includes raw protocol lines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingIrc {
    Privmsg { target: String, text: String },
    Notice { target: String, text: String },
    Join { channel: String },
    Part { channel: String, message: Option<String> },
    Raw(String),
}

impl From<crate::transport::OutgoingAction> for OutgoingIrc {
    fn from(action: crate::transport::OutgoingAction) -> Self {
        match action {
            crate::transport::OutgoingAction::Direct { recipient, text } => {
                OutgoingIrc::Privmsg {
                    target: recipient,
                    text,
                }
            }
            crate::transport::OutgoingAction::Notice { recipient, text } => OutgoingIrc::Notice {
                target: recipient,
                text,
            },
            crate::transport::OutgoingAction::Join { presence } => OutgoingIrc::Join {
                channel: presence,
            },
            crate::transport::OutgoingAction::Leave { presence, message } => OutgoingIrc::Part {
                channel: presence,
                message,
            },
        }
    }
}

/// IRC extension of [`GameTransport`] for registration and capability negotiation.
#[async_trait]
pub trait IrcTransport: GameTransport {
    async fn send_raw(&self, line: &str);
}

/// Tokio TCP IRC client that reads lines and forwards them to a handler.
#[derive(Debug, Default)]
pub struct TcpIrcClient;

impl TcpIrcClient {
    pub fn new() -> Self {
        Self
    }

    pub async fn connect_and_run<H, Fut>(
        server: &str,
        port: u16,
        nick: &str,
        realname: &str,
        channels: &[String],
        mut handler: H,
    ) -> anyhow::Result<()>
    where
        H: FnMut(super::message::IrcMessage) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpStream;

        let addr = format!("{server}:{port}");
        let mut stream = TcpStream::connect(&addr).await?;
        let (reader, mut writer) = stream.split();
        let mut lines = BufReader::new(reader).lines();

        writer
            .write_all(format_outgoing("NICK", &[nick], None).as_bytes())
            .await?;
        writer
            .write_all(
                format_outgoing("USER", &[nick, "0", "*"], Some(realname)).as_bytes(),
            )
            .await?;

        for channel in channels {
            writer
                .write_all(format_outgoing("JOIN", &[channel], None).as_bytes())
                .await?;
        }

        while let Some(line) = lines.next_line().await? {
            let msg = super::message::parse_irc_line(&line);
            if let super::message::IrcMessage::Ping { token } = &msg {
                writer
                    .write_all(format_outgoing("PONG", &[token], None).as_bytes())
                    .await?;
            }
            handler(msg).await?;
        }
        Ok(())
    }
}

/// Outbound IRC stream (plain TCP or TLS) wrapped for [`IrcTransport`].
#[derive(Clone)]
pub struct StreamTransport {
    writer: Arc<tokio::sync::Mutex<Box<dyn tokio::io::AsyncWrite + Send + Unpin>>>,
}

impl StreamTransport {
    pub fn new(writer: Box<dyn tokio::io::AsyncWrite + Send + Unpin>) -> Self {
        Self {
            writer: Arc::new(tokio::sync::Mutex::new(writer)),
        }
    }
}

#[async_trait]
impl GameTransport for StreamTransport {
    async fn send_direct(&self, recipient: &str, text: &str) {
        let line = format_outgoing("PRIVMSG", &[recipient], Some(text));
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(line.as_bytes()).await;
    }

    async fn send_notice(&self, recipient: &str, text: &str) {
        let line = format_outgoing("NOTICE", &[recipient], Some(text));
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(line.as_bytes()).await;
    }

    async fn join(&self, presence: &str) {
        let line = format_outgoing("JOIN", &[presence], None);
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(line.as_bytes()).await;
    }

    async fn leave(&self, presence: &str, message: Option<&str>) {
        let line = format_outgoing("PART", &[presence], message);
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(line.as_bytes()).await;
    }
}

#[async_trait]
impl IrcTransport for StreamTransport {
    async fn send_raw(&self, line: &str) {
        let payload = if line.ends_with("\r\n") {
            line.to_string()
        } else {
            format!("{line}\r\n")
        };
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(payload.as_bytes()).await;
    }
}

/// Backward-compatible alias for [`StreamTransport`].
pub type TcpTransport = StreamTransport;
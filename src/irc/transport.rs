//! IRC transport abstraction — real TCP client and in-memory mock for tests.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use super::message::format_outgoing;

/// Outgoing IRC action recorded by [`MockTransport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingIrc {
    Privmsg { target: String, text: String },
    Notice { target: String, text: String },
    Join { channel: String },
    Part { channel: String, message: Option<String> },
    Raw(String),
}

/// Transport surface used by [`super::bot::IrcBot`].
#[async_trait]
pub trait IrcTransport: Send + Sync {
    async fn send_privmsg(&self, target: &str, text: &str);
    async fn send_notice(&self, target: &str, text: &str);
    async fn join(&self, channel: &str);
    async fn part(&self, channel: &str, message: Option<&str>);
    async fn send_raw(&self, line: &str);
}

/// In-memory transport that records all outgoing messages for assertions.
#[derive(Debug, Default, Clone)]
pub struct MockTransport {
    log: Arc<Mutex<Vec<OutgoingIrc>>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn recorded(&self) -> Vec<OutgoingIrc> {
        self.log.lock().expect("mock transport lock").clone()
    }

    pub fn clear(&self) {
        self.log.lock().expect("mock transport lock").clear();
    }

    pub fn privmsgs_to(&self, target: &str) -> Vec<String> {
        self.recorded()
            .into_iter()
            .filter_map(|entry| match entry {
                OutgoingIrc::Privmsg { target: t, text } if t == target => Some(text),
                _ => None,
            })
            .collect()
    }

    pub fn channel_messages(&self, channel: &str) -> Vec<String> {
        self.privmsgs_to(channel)
    }
}

#[async_trait]
impl IrcTransport for MockTransport {
    async fn send_privmsg(&self, target: &str, text: &str) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingIrc::Privmsg {
                target: target.to_string(),
                text: text.to_string(),
            });
    }

    async fn send_notice(&self, target: &str, text: &str) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingIrc::Notice {
                target: target.to_string(),
                text: text.to_string(),
            });
    }

    async fn join(&self, channel: &str) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingIrc::Join {
                channel: channel.to_string(),
            });
    }

    async fn part(&self, channel: &str, message: Option<&str>) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingIrc::Part {
                channel: channel.to_string(),
                message: message.map(str::to_string),
            });
    }

    async fn send_raw(&self, line: &str) {
        self.log
            .lock()
            .expect("mock transport lock")
            .push(OutgoingIrc::Raw(line.to_string()));
    }
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
impl IrcTransport for StreamTransport {
    async fn send_privmsg(&self, target: &str, text: &str) {
        let line = format_outgoing("PRIVMSG", &[target], Some(text));
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(line.as_bytes()).await;
    }

    async fn send_notice(&self, target: &str, text: &str) {
        let line = format_outgoing("NOTICE", &[target], Some(text));
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(line.as_bytes()).await;
    }

    async fn join(&self, channel: &str) {
        let line = format_outgoing("JOIN", &[channel], None);
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(line.as_bytes()).await;
    }

    async fn part(&self, channel: &str, message: Option<&str>) {
        let line = format_outgoing("PART", &[channel], message);
        let mut writer = self.writer.lock().await;
        let _ = writer.write_all(line.as_bytes()).await;
    }

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
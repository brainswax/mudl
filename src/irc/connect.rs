//! TLS and plain TCP connections to IRCv3 servers.

use std::sync::{Arc, Once};
use std::time::Duration;

use anyhow::Context;
use rustls::pki_types::ServerName;
use rustls::RootCertStore;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tracing::{debug, info};

use super::config::IrcConfig;
use super::transport::StreamTransport;

static INSTALL_CRYPTO_PROVIDER: Once = Once::new();

/// Install rustls's ring backend once per process (required since rustls 0.23).
fn ensure_crypto_provider() {
    INSTALL_CRYPTO_PROVIDER.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("failed to install rustls ring crypto provider");
    });
}

/// Live IRC connection: line reader plus a shared outbound transport.
pub struct IrcConnection {
    lines: tokio::io::Lines<BufReader<Box<dyn tokio::io::AsyncRead + Send + Unpin>>>,
    pub transport: StreamTransport,
    read_timeout: Duration,
}

impl IrcConnection {
    pub async fn next_line(&mut self) -> anyhow::Result<Option<String>> {
        match tokio::time::timeout(self.read_timeout, self.lines.next_line()).await {
            Ok(Ok(line)) => {
                if let Some(ref text) = line {
                    debug!(line = %sanitize_wire_line(text), "IRC <<");
                }
                Ok(line)
            }
            Ok(Err(err)) => Err(err).context("IRC connection read failed"),
            Err(_) => anyhow::bail!(
                "IRC read timed out after {}s — no data from server. \
                 Check nick availability, IRC_IRCV3 setting, and network reachability.",
                self.read_timeout.as_secs()
            ),
        }
    }

    pub fn read_timeout(&self) -> Duration {
        self.read_timeout
    }
}

/// Connect to the configured IRC server (TLS by default).
pub async fn connect(config: &IrcConfig) -> anyhow::Result<IrcConnection> {
    let addr = format!("{}:{}", config.server, config.port);
    let connect_timeout = Duration::from_secs(config.connect_timeout_secs);
    info!(
        addr = %addr,
        tls = config.tls,
        timeout_secs = config.connect_timeout_secs,
        "connecting to IRC server"
    );

    let connection = if config.tls {
        connect_tls(&addr, &config.server, connect_timeout).await?
    } else {
        connect_plain(&addr, connect_timeout).await?
    };

    info!(addr = %addr, "IRC transport ready — send registration (CAP/NICK/USER)");
    Ok(IrcConnection {
        lines: connection.lines,
        transport: connection.transport,
        read_timeout: Duration::from_secs(config.read_timeout_secs),
    })
}

struct RawConnection {
    lines: tokio::io::Lines<BufReader<Box<dyn tokio::io::AsyncRead + Send + Unpin>>>,
    transport: StreamTransport,
}

async fn connect_plain(addr: &str, connect_timeout: Duration) -> anyhow::Result<RawConnection> {
    let stream = tokio::time::timeout(connect_timeout, TcpStream::connect(addr))
        .await
        .map_err(|_| {
            anyhow::anyhow!("TCP connect to {addr} timed out after {secs}s", secs = connect_timeout.as_secs())
        })?
        .with_context(|| format!("failed to connect to IRC server at {addr}"))?;
    info!(addr = %addr, "TCP connected (plaintext)");
    let (reader, writer) = stream.into_split();
    let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = Box::new(reader);
    Ok(RawConnection {
        lines: BufReader::new(reader).lines(),
        transport: StreamTransport::new(Box::new(writer)),
    })
}

async fn connect_tls(
    addr: &str,
    server_name: &str,
    connect_timeout: Duration,
) -> anyhow::Result<RawConnection> {
    ensure_crypto_provider();

    let stream = tokio::time::timeout(connect_timeout, TcpStream::connect(addr))
        .await
        .map_err(|_| {
            anyhow::anyhow!("TCP connect to {addr} timed out after {secs}s", secs = connect_timeout.as_secs())
        })?
        .with_context(|| format!("failed to connect to IRC server at {addr}"))?;
    info!(addr = %addr, "TCP connected, starting TLS handshake");

    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(tls_config));
    let sni = ServerName::try_from(server_name.to_string())
        .map_err(|_| anyhow::anyhow!("invalid TLS server name: {server_name}"))?;

    let tls_stream = tokio::time::timeout(connect_timeout, connector.connect(sni, stream))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "TLS handshake with {server_name} timed out after {secs}s",
                secs = connect_timeout.as_secs()
            )
        })?
        .context("TLS handshake with IRC server failed")?;

    info!(server = %server_name, "TLS handshake complete");

    let (reader, writer) = tokio::io::split(tls_stream);
    let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = Box::new(reader);
    Ok(RawConnection {
        lines: BufReader::new(reader).lines(),
        transport: StreamTransport::new(Box::new(writer)),
    })
}

/// Redact secrets from wire lines before logging.
fn sanitize_wire_line(line: &str) -> String {
    let upper = line.to_ascii_uppercase();
    if upper.contains("IDENTIFY") || upper.contains("REGISTER") || upper.contains("PASS") {
        return "<redacted>".to_string();
    }
    if line.len() > 200 {
        return format!("{}…", &line[..200]);
    }
    line.to_string()
}

/// Log an outbound registration/command line (redacted).
pub fn log_outbound_command(command: &str) {
    let trimmed = command.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return;
    }
    debug!(line = %sanitize_wire_line(trimmed), "IRC >>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_provider_allows_tls_client_config_build() {
        ensure_crypto_provider();
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let _config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
    }

    #[test]
    fn sanitize_wire_line_redacts_secrets() {
        assert_eq!(
            sanitize_wire_line("PRIVMSG NickServ :IDENTIFY alice sekrit"),
            "<redacted>"
        );
        assert!(sanitize_wire_line(":server 001 mudl :Welcome").contains("Welcome"));
    }
}
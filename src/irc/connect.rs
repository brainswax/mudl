//! TLS and plain TCP connections to IRCv3 servers.

use std::sync::{Arc, Once};

use anyhow::Context;
use rustls::pki_types::ServerName;
use rustls::RootCertStore;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

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
}

impl IrcConnection {
    pub async fn next_line(&mut self) -> anyhow::Result<Option<String>> {
        self.lines
            .next_line()
            .await
            .context("IRC connection read failed")
    }
}

/// Connect to the configured IRC server (TLS by default).
pub async fn connect(config: &IrcConfig) -> anyhow::Result<IrcConnection> {
    let addr = format!("{}:{}", config.server, config.port);
    if config.tls {
        connect_tls(&addr, &config.server).await
    } else {
        connect_plain(&addr).await
    }
}

async fn connect_plain(addr: &str) -> anyhow::Result<IrcConnection> {
    let stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("failed to connect to IRC server at {addr}"))?;
    let (reader, writer) = stream.into_split();
    let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = Box::new(reader);
    Ok(IrcConnection {
        lines: BufReader::new(reader).lines(),
        transport: StreamTransport::new(Box::new(writer)),
    })
}

async fn connect_tls(addr: &str, server_name: &str) -> anyhow::Result<IrcConnection> {
    ensure_crypto_provider();

    let stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("failed to connect to IRC server at {addr}"))?;

    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(tls_config));
    let server_name = ServerName::try_from(server_name.to_string())
        .map_err(|_| anyhow::anyhow!("invalid TLS server name: {server_name}"))?;

    let tls_stream = connector
        .connect(server_name, stream)
        .await
        .context("TLS handshake with IRC server failed")?;

    let (reader, writer) = tokio::io::split(tls_stream);
    let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = Box::new(reader);
    Ok(IrcConnection {
        lines: BufReader::new(reader).lines(),
        transport: StreamTransport::new(Box::new(writer)),
    })
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
}
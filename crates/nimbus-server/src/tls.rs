//! TLS termination for the main HTTP listener (LR8).
//!
//! The server terminates TLS itself when `ServeOptions::with_tls` provides
//! a certificate/key pair: a tokio-rustls acceptor wraps each accepted
//! connection and hands it to hyper with upgrade support, so HTTPS and
//! `wss://` work identically to the plain-TCP path. The MongoDB and
//! DynamoDB sibling listeners stay plain TCP behind a TLS-terminating
//! proxy — see `docs/private/decisions/adapter-listener-tls.md`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HyperConnBuilder;
use hyper_util::service::TowerToHyperService;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;

/// Certificate/key pair for terminating TLS on the main HTTP listener.
/// Both paths must point at PEM files; loading fails with an error naming
/// the offending path.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

impl TlsConfig {
    pub fn new(cert_path: impl Into<PathBuf>, key_path: impl Into<PathBuf>) -> Self {
        Self {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
        }
    }
}

/// Load and validate the PEM pair into a rustls server config. Called at
/// startup so a bad certificate fails the boot, not the first connection.
pub(crate) fn load_rustls_server_config(config: &TlsConfig) -> io::Result<Arc<ServerConfig>> {
    let certs = load_pem_certs(&config.cert_path)?;
    let key = load_pem_private_key(&config.key_path)?;
    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "TLS certificate {} and key {} do not form a valid identity: {error}",
                    config.cert_path.display(),
                    config.key_path.display()
                ),
            )
        })?;
    Ok(Arc::new(server_config))
}

fn load_pem_certs(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    let certs = CertificateDer::pem_file_iter(path)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to read TLS certificate {}: {error}", path.display()),
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "TLS certificate {} is not valid PEM: {error}",
                    path.display()
                ),
            )
        })?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "TLS certificate {} contains no certificates",
                path.display()
            ),
        ));
    }
    Ok(certs)
}

fn load_pem_private_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_file(path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "TLS key {} is missing or not a valid PEM private key: {error}",
                path.display()
            ),
        )
    })
}

/// TLS accept loop for the main listener: stop accepting on shutdown,
/// serve each connection with upgrade support (WebSocket). Handshake or
/// per-connection errors are logged and never tear down the listener.
pub(crate) async fn serve_tls(
    listener: TcpListener,
    router: Router,
    server_config: Arc<ServerConfig>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> io::Result<()> {
    let acceptor = TlsAcceptor::from(server_config);
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return Ok(());
                }
                continue;
            }
        };
        let (stream, peer_addr) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "TLS listener accept failed");
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let service = TowerToHyperService::new(router.clone());
        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(stream).await {
                Ok(tls_stream) => tls_stream,
                Err(error) => {
                    tracing::debug!(%error, %peer_addr, "TLS handshake failed");
                    return;
                }
            };
            if let Err(error) = HyperConnBuilder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(TokioIo::new(tls_stream), service)
                .await
            {
                tracing::debug!(%error, %peer_addr, "TLS connection ended with error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Self-signed localhost fixture pair (test-only; see the README beside
    // the fixtures for the regeneration command).
    const CERT: &str = "tests/fixtures/tls/localhost-cert.pem";
    const KEY: &str = "tests/fixtures/tls/localhost-key.pem";

    fn fixture(path: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }

    #[test]
    fn loads_the_checked_in_localhost_fixture_pair() {
        let config = TlsConfig::new(fixture(CERT), fixture(KEY));
        load_rustls_server_config(&config).expect("fixture pair should load");
    }

    #[test]
    fn missing_or_invalid_inputs_fail_with_the_offending_path() {
        let missing = TlsConfig::new(fixture("tests/fixtures/tls/nope.pem"), fixture(KEY));
        let error = load_rustls_server_config(&missing).expect_err("missing cert must fail");
        assert!(error.to_string().contains("nope.pem"), "{error}");

        let swapped = TlsConfig::new(fixture(KEY), fixture(CERT));
        let error = load_rustls_server_config(&swapped).expect_err("swapped pair must fail");
        assert!(
            error.to_string().contains("localhost"),
            "error should name a path, got: {error}"
        );
    }
}

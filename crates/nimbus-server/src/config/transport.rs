use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::watch;

use crate::system::VersionCheck;
use crate::system::version_check::VersionCheckConfig;

#[derive(Clone, Default)]
pub(crate) struct TransportConfig {
    listen_addr: Option<SocketAddr>,
    server_shutdown: Option<watch::Sender<bool>>,
    cors_allowed_origins: Vec<String>,
    version_check: Option<Arc<VersionCheck>>,
}

impl TransportConfig {
    pub(crate) fn with_cors_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_allowed_origins = origins;
        self
    }

    pub(crate) fn with_listen_addr(mut self, listen_addr: SocketAddr) -> Self {
        self.listen_addr = Some(listen_addr);
        self
    }

    pub(crate) fn with_server_shutdown(mut self, server_shutdown: watch::Sender<bool>) -> Self {
        self.server_shutdown = Some(server_shutdown);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_version_check(mut self, version_check: Arc<VersionCheck>) -> Self {
        self.version_check = Some(version_check);
        self
    }

    pub(crate) fn ensure_version_check(mut self) -> Self {
        if self.version_check.is_none() {
            self.version_check = Some(build_version_check());
        }
        self
    }

    pub(crate) fn listen_addr(&self) -> Option<SocketAddr> {
        self.listen_addr
    }

    pub(crate) fn server_shutdown(&self) -> Option<&watch::Sender<bool>> {
        self.server_shutdown.as_ref()
    }

    pub(crate) fn cors_allowed_origins(&self) -> &[String] {
        &self.cors_allowed_origins
    }

    pub(crate) fn version_check(&self) -> Arc<VersionCheck> {
        self.version_check
            .as_ref()
            .expect("transport config should carry a version check before AppState is built")
            .clone()
    }
}

fn build_version_check() -> Arc<VersionCheck> {
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .unwrap_or_else(|_| semver::Version::new(0, 0, 0));
    let config = VersionCheckConfig::from_env(&current);
    VersionCheck::new(current, config)
}

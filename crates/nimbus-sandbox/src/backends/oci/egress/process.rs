//! Process-owned PEP lifecycle authority.

use std::sync::Arc;

use nimbus_proxy::EgressEngine;

use super::RegisteredArtifacts;

/// The one OCI-process lifecycle map for workload PEPs.
///
/// Backend facades combine this opaque shared engine with their own decision
/// log and trust-anchor roots. Provider effects and artifact publication stay
/// in [`super::EgressProxyRegistry`].
#[derive(Clone)]
pub(crate) struct EgressProxyProcess {
    engine: Arc<EgressEngine<RegisteredArtifacts>>,
}

impl EgressProxyProcess {
    pub(crate) fn new() -> Self {
        Self {
            engine: Arc::new(EgressEngine::new()),
        }
    }

    pub(super) fn engine(&self) -> Arc<EgressEngine<RegisteredArtifacts>> {
        Arc::clone(&self.engine)
    }
}

//! Crash-safe container egress-policy reload composition.

use nimbus_egress::EgressPolicy;

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::{ContainerSandboxBackend, ContainerStartMode, runner};

impl ContainerSandboxBackend {
    #[cfg(test)]
    pub(super) fn with_post_egress_reload_ack_observer(
        mut self,
        observer: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.post_egress_reload_ack_observer = Some(std::sync::Arc::new(observer));
        self
    }

    pub fn reload_egress_policy(&self, id: &SandboxId, egress: EgressPolicy) -> Result<()> {
        let requested = egress
            .compile()
            .map_err(|message| SandboxError::InvalidSpec { message })?;
        let Some(manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };
        if manifest.start_mode != ContainerStartMode::Execute {
            return Err(SandboxError::InvalidSpec {
                message: "container egress live reload requires execute-mode sandbox".to_owned(),
            });
        }
        self.ensure_startup_reconciliation_ready()?;
        let (_lifecycle, mut manifest) =
            runner::lock_current_execute_lifecycle_for_backend(self, &manifest)?;
        if let Some(phase) = runner::execute_handoff_phase(&manifest)? {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container egress live reload for {id} requires published lifecycle \
                     ownership; runner handoff remains in {phase:?}"
                ),
            });
        }
        if manifest.launch_reservation_claim.is_some() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container egress live reload for {id} requires published lifecycle \
                     ownership; the launch reservation remains before provider effects"
                ),
            });
        }

        if let Some(pending) = manifest.egress_policy_reload.pending_attempt()? {
            let durable = manifest.spec.egress.compile().map_err(|message| {
                SandboxError::OperationFailed {
                    message: format!(
                        "durable egress policy for applying reload {pending:?} is invalid: \
                         {message}"
                    ),
                }
            })?;
            self.ensure_egress_proxy_running(&manifest)?;
            let receipt = self.egress_proxies.reconcile_reload(
                &manifest.spec.tenant_id,
                id,
                durable,
                pending,
            )?;
            self.observe_egress_reload_acknowledgement();
            manifest.egress_policy_reload.complete(receipt)?;
            self.write_manifest(&manifest)?;
            if manifest.spec.egress == *requested.policy() {
                return Ok(());
            }
        }

        if manifest.spec.egress == *requested.policy() {
            self.ensure_egress_proxy_running(&manifest)?;
            return Ok(());
        }

        let attempt = manifest.egress_policy_reload.begin()?;
        manifest.spec.egress = requested.policy().clone();
        // Desired bytes plus both generations become durable before the PEP
        // effect. Any failure after this publication is an applying attempt,
        // never an unrecorded acknowledged policy.
        self.write_manifest(&manifest)?;
        self.ensure_egress_proxy_running(&manifest)?;
        let receipt = self.egress_proxies.reconcile_reload(
            &manifest.spec.tenant_id,
            id,
            requested,
            attempt,
        )?;
        self.observe_egress_reload_acknowledgement();
        manifest.egress_policy_reload.complete(receipt)?;
        self.write_manifest(&manifest)
    }

    fn observe_egress_reload_acknowledgement(&self) {
        #[cfg(test)]
        if let Some(observer) = self.post_egress_reload_ack_observer.as_ref() {
            observer();
        }
    }
}

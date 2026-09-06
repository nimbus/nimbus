//! Crash-safe container egress-policy reload composition.

use nimbus_egress::EgressPolicy;

use crate::backends::oci::egress::EgressReloadAttachmentState;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::{ContainerCreatorHandoffState, ContainerSandboxBackend, ContainerStartMode, runner};

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
            .compile_for_supervisor_proxy()
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
        if manifest.launch_reservation_claim.is_some()
            && !matches!(
                manifest.creator_handoff,
                ContainerCreatorHandoffState::RuntimeObserved { .. }
            )
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container egress live reload for {id} requires published lifecycle \
                     ownership; the launch reservation remains without an authenticated runtime \
                     effect receipt"
                ),
            });
        }

        if let Some(pending) = manifest.egress_policy_reload.pending_attempt()? {
            let durable = manifest
                .spec
                .egress
                .compile_for_supervisor_proxy()
                .map_err(|message| SandboxError::OperationFailed {
                    message: format!(
                        "durable egress policy for applying reload {pending:?} is invalid: \
                             {message}"
                    ),
                })?;
            self.ensure_reload_registration(&manifest, &durable)?;
            let receipt = self.egress_proxies.reconcile_authenticated_reload(
                &manifest.spec.tenant_id,
                id,
                manifest.egress_proxy.as_ref(),
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
            self.ensure_reload_registration(&manifest, &requested)?;
            if let Some(attempt) = manifest.egress_policy_reload.active_attempt()? {
                self.egress_proxies.reconcile_authenticated_reload(
                    &manifest.spec.tenant_id,
                    id,
                    manifest.egress_proxy.as_ref(),
                    requested,
                    attempt,
                )?;
            }
            return Ok(());
        }

        let attempt = manifest.egress_policy_reload.begin()?;
        manifest.spec.egress = requested.policy().clone();
        // Desired bytes plus both generations become durable before the PEP
        // effect. Any failure after this publication is an applying attempt,
        // never an unrecorded acknowledged policy.
        self.write_manifest(&manifest)?;
        self.ensure_reload_registration(&manifest, &requested)?;
        let receipt = self.egress_proxies.reconcile_authenticated_reload(
            &manifest.spec.tenant_id,
            id,
            manifest.egress_proxy.as_ref(),
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

    fn ensure_reload_registration(
        &self,
        manifest: &super::ContainerSandboxManifest,
        durable: &nimbus_egress::CompiledEgressPolicy,
    ) -> Result<()> {
        match self.egress_proxies.authenticated_reload_attachment(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            durable,
        )? {
            EgressReloadAttachmentState::Authenticated => return Ok(()),
            EgressReloadAttachmentState::MissingRegistration => {
                self.ensure_egress_proxy_running(manifest)?;
            }
        }
        match self.egress_proxies.authenticated_reload_attachment(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            durable,
        )? {
            EgressReloadAttachmentState::Authenticated => Ok(()),
            EgressReloadAttachmentState::MissingRegistration => {
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "egress proxy for sandbox {} remained absent after reconstruction",
                        manifest.handle.id
                    ),
                })
            }
        }
    }

    pub(super) fn replay_stable_egress_reload_attempt(
        &self,
        manifest: &super::ContainerSandboxManifest,
    ) -> Result<()> {
        if manifest.egress_policy_reload.is_applying() {
            return Ok(());
        }
        let Some(attempt) = manifest.egress_policy_reload.active_attempt()? else {
            return Ok(());
        };
        let durable = manifest
            .spec
            .egress
            .compile_for_supervisor_proxy()
            .map_err(|message| SandboxError::OperationFailed {
                message: format!(
                    "stable durable egress policy for reload replay {attempt:?} is invalid: \
                     {message}"
                ),
            })?;
        self.egress_proxies.reconcile_authenticated_reload(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            durable,
            attempt,
        )?;
        Ok(())
    }
}

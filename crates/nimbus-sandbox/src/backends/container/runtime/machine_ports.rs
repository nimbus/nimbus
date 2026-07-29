//! Container-owned lifecycle for provider-managed machine port proxies.

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard};

use nimbus_core::TenantId;
use nimbus_network::PortLeaseRequest;

#[cfg(test)]
use crate::backends::oci::network::panicking_machine_port_proxy_for_test;
use crate::backends::oci::network::{
    MachinePortPreparationReleaseAuthority, MachinePortProxyCleanupDisposition,
    MachinePortProxyCleanupState, MachinePortProxyEntry, MachinePortProxyKey,
    MachinePortProxyLeaseAuthority, MachinePortProxyRegistration, OciMachinePortForwarderConfig,
    machine_port_proxy_routes, prepare_machine_port_proxies_with_release_authority,
    start_machine_port_proxies_with_recovery, unexpose_machine_ports,
};
use crate::backends::oci::port_lifecycle::OciPortLeaseCoordinator;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

use super::{ContainerSandboxBackend, ContainerSandboxManifest};

fn partial_start_cleanup_disposition(
    release_authority: MachinePortPreparationReleaseAuthority<'_>,
) -> MachinePortProxyCleanupDisposition {
    match release_authority {
        MachinePortPreparationReleaseAuthority::FreshLaunch(_) => {
            MachinePortProxyCleanupDisposition::Release
        }
        MachinePortPreparationReleaseAuthority::Retain => {
            MachinePortProxyCleanupDisposition::Restart
        }
    }
}

pub(super) struct MachinePortProxyCleanup {
    key: MachinePortProxyKey,
    state: Arc<Mutex<MachinePortProxyCleanupState>>,
}

struct MachinePortProxyLifecycleHooks<BeforeActivation, AfterActivation, AfterValidation, Publish> {
    before_activation: BeforeActivation,
    after_activation: AfterActivation,
    after_active_validation: AfterValidation,
    publish: Publish,
}

struct MachinePortProxyCleanupRequest<'a> {
    tenant_id: &'a TenantId,
    id: &'a SandboxId,
    expected_port_bindings: &'a [SandboxPortBinding],
    expected_port_leases: &'a [PortLeaseRequest],
    disposition: MachinePortProxyCleanupDisposition,
    port_lease_coordinator: OciPortLeaseCoordinator,
}

#[cfg(test)]
fn fresh_machine_port_release_authority(
    manifest: &ContainerSandboxManifest,
) -> Result<MachinePortPreparationReleaseAuthority<'_>> {
    manifest
        .launch_reservation_claim
        .as_ref()
        .map(MachinePortPreparationReleaseAuthority::FreshLaunch)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "initial machine-port launch for {} lacks reservation compensation authority",
                manifest.handle.id
            ),
        })
}

impl ContainerSandboxBackend {
    #[cfg(test)]
    pub(super) fn ensure_machine_port_proxies_running(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.ensure_machine_port_proxies_running_with_lifecycle_observers(
            id,
            assigned_ips,
            manifest,
            fresh_machine_port_release_authority(manifest)?,
            MachinePortProxyLifecycleHooks {
                before_activation: || Ok(()),
                after_activation: || Ok(()),
                after_active_validation: || {},
                publish: || Ok(()),
            },
        )
    }

    #[cfg(test)]
    pub(super) fn ensure_machine_port_proxies_running_for_restart(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.ensure_machine_port_proxies_running_with_lifecycle_observers(
            id,
            assigned_ips,
            manifest,
            MachinePortPreparationReleaseAuthority::Retain,
            MachinePortProxyLifecycleHooks {
                before_activation: || Ok(()),
                after_activation: || Ok(()),
                after_active_validation: || {},
                publish: || Ok(()),
            },
        )
    }

    pub(super) fn ensure_machine_port_proxies_running_with_publication(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
        release_authority: MachinePortPreparationReleaseAuthority<'_>,
        publish: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.ensure_machine_port_proxies_running_with_lifecycle_observers(
            id,
            assigned_ips,
            manifest,
            release_authority,
            MachinePortProxyLifecycleHooks {
                before_activation: || Ok(()),
                after_activation: || Ok(()),
                after_active_validation: || {},
                publish,
            },
        )
    }

    fn ensure_machine_port_proxies_running_with_lifecycle_observers<
        BeforeActivation,
        AfterActivation,
        AfterValidation,
        Publish,
    >(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
        release_authority: MachinePortPreparationReleaseAuthority<'_>,
        hooks: MachinePortProxyLifecycleHooks<
            BeforeActivation,
            AfterActivation,
            AfterValidation,
            Publish,
        >,
    ) -> Result<()>
    where
        BeforeActivation: FnOnce() -> Result<()>,
        AfterActivation: FnOnce() -> Result<()>,
        AfterValidation: FnOnce(),
        Publish: FnOnce() -> Result<()>,
    {
        let MachinePortProxyLifecycleHooks {
            before_activation,
            after_activation,
            after_active_validation,
            publish,
        } = hooks;
        if id != &manifest.handle.id {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port proxy caller sandbox {id} does not match manifest sandbox {}",
                    manifest.handle.id
                ),
            });
        }
        if manifest.handle.tenant_id != manifest.spec.tenant_id {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port proxy manifest handle tenant {} does not match spec tenant {} \
                     for sandbox {id}",
                    manifest.handle.tenant_id, manifest.spec.tenant_id
                ),
            });
        }
        let routes = machine_port_proxy_routes(assigned_ips, &manifest.spec.port_bindings)?;
        let mut after_active_validation = Some(after_active_validation);
        let mut publish = Some(publish);
        let manager = self.port_lease_coordinator_for_manifest(manifest)?;
        let mut proxies =
            self.machine_port_proxies
                .lock()
                .map_err(|_| SandboxError::OperationFailed {
                    message: "container machine port proxy registry lock is poisoned".to_owned(),
                })?;
        let key = (manifest.spec.tenant_id.clone(), id.clone());
        if let Some(entry) = proxies.get_mut(&key) {
            let MachinePortProxyEntry::Running(registration) = entry else {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container machine port proxy cleanup is still in progress for tenant {} \
                         sandbox {id}; replacement publication is fenced",
                        manifest.spec.tenant_id
                    ),
                });
            };
            if registration.port_bindings != manifest.spec.port_bindings
                || registration.port_leases != manifest.port_leases
                || registration.routes != routes
                || registration.proxies.len() != routes.len()
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container machine port proxy registry for tenant {} sandbox {id} does \
                         not match the manifest's exact listener generation",
                        manifest.spec.tenant_id
                    ),
                });
            }
            let live_authority = match registration.lease_authority.as_ref() {
                Some(MachinePortProxyLeaseAuthority::Live(authority)) => authority,
                Some(MachinePortProxyLeaseAuthority::Recovered(_)) | None => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "container machine port proxy registry for tenant {} sandbox {id} \
                             lacks its exact live process lifetime",
                            manifest.spec.tenant_id
                        ),
                    });
                }
            };
            manager.require_active_machine_bindings_with_lifetimes(
                &manifest.spec.tenant_id,
                &manifest.handle.id,
                &manifest.spec.port_bindings,
                &manifest.port_leases,
                live_authority,
            )?;
            after_active_validation
                .take()
                .expect("validation observer runs exactly once")();
            if registration
                .proxies
                .iter()
                .any(|proxy| !proxy.provider_is_running())
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container machine port proxy provider worker exited for tenant {} \
                         sandbox {id}; publication remains fenced until exact cleanup and \
                         reconciliation",
                        manifest.spec.tenant_id
                    ),
                });
            }
            registration.publication_may_exist = true;
            return publish
                .take()
                .expect("publication runs exactly once for a validated provider")(
            );
        }
        let prepared = prepare_machine_port_proxies_with_release_authority(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            assigned_ips,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            &manager,
            release_authority,
        )?;
        let activation_result = before_activation()
            .and_then(|()| {
                manager.activate_machine_bindings_with_lifetimes(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.spec.port_bindings,
                    &manifest.port_leases,
                    prepared.bind_authority(),
                )
            })
            .and_then(|bindings| {
                after_activation()?;
                Ok(bindings)
            });
        let expected_bindings = match activation_result {
            Ok(bindings) => bindings,
            Err(activation_error) => {
                // Provider absence must precede durable claim compensation.
                // `prepared` owns every inert socket and no accept worker or
                // external publication has started yet.
                let (prepared_proxies, bind_authority) = prepared.into_parts();
                drop(prepared_proxies);
                let compensation_result = match manager
                    .abandon_machine_bind_claims_with_lifetimes_without_effect(
                        &manifest.port_leases,
                        &bind_authority,
                    ) {
                    Ok(()) => Ok(()),
                    Err(abandon_error) => {
                        // A store acknowledgement can be lost after the atomic
                        // activation commit. Inspect the exact durable binding
                        // set rather than treating the error as proof that the
                        // claims remain Reserved.
                        match manager.require_active_machine_bindings(
                            &manifest.spec.tenant_id,
                            &manifest.handle.id,
                            &manifest.spec.port_bindings,
                            &manifest.port_leases,
                        ) {
                            Ok(active_bindings) => manager
                                .prepare_machine_bindings_for_rebind_with_lifetimes(
                                    &manifest.port_leases,
                                    &active_bindings,
                                    &bind_authority,
                                )
                                .map_err(|rebind_error| {
                                    format!(
                                        "bind-claim abandonment failed: {abandon_error}; exact \
                                         Active compensation also failed: {rebind_error}"
                                    )
                                }),
                            Err(inspect_error) => Err(format!(
                                "bind-claim abandonment failed: {abandon_error}; exact Active \
                                 inspection also failed: {inspect_error}"
                            )),
                        }
                    }
                };
                return Err(match compensation_result {
                    Ok(()) => activation_error,
                    Err(compensation_error) => SandboxError::OperationFailed {
                        message: format!(
                            "{activation_error}; machine activation compensation also failed: \
                             {compensation_error}"
                        ),
                    },
                });
            }
        };
        after_active_validation
            .take()
            .expect("validation observer runs exactly once")();
        let (running, bind_authority) = match start_machine_port_proxies_with_recovery(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            &manager,
            prepared,
        ) {
            Ok(running) => running.into_parts(),
            Err(failure) => {
                let (start_error, running, bind_authority) = failure.into_parts();
                let disposition = partial_start_cleanup_disposition(release_authority);
                let state = Arc::new(Mutex::new(MachinePortProxyCleanupState {
                    disposition,
                    port_lease_coordinator: manager.clone(),
                    registration: MachinePortProxyRegistration {
                        port_bindings: manifest.spec.port_bindings.clone(),
                        port_leases: manifest.port_leases.clone(),
                        routes,
                        proxies: running,
                        lease_authority: Some(MachinePortProxyLeaseAuthority::Live(bind_authority)),
                        publication_may_exist: false,
                    },
                    expected_bindings,
                    withdraw_complete: disposition == MachinePortProxyCleanupDisposition::Restart,
                    provider_stopped: false,
                    publication_withdrawn: vec![true; manifest.spec.port_bindings.len()],
                    durable_transition_complete: false,
                }));
                proxies.insert(
                    key.clone(),
                    MachinePortProxyEntry::Stopping(Arc::clone(&state)),
                );
                drop(proxies);
                let cleanup = MachinePortProxyCleanup { key, state };
                let cleanup_result = self
                    .stop_machine_port_proxy_provider(&cleanup)
                    .and_then(|()| self.complete_machine_port_proxy_cleanup(&cleanup));
                return Err(match cleanup_result {
                    Ok(()) => start_error,
                    Err(cleanup_error) => SandboxError::OperationFailed {
                        message: format!(
                            "{start_error}; partial machine proxy start cleanup also failed: \
                             {cleanup_error}"
                        ),
                    },
                });
            }
        };
        proxies.insert(
            key,
            MachinePortProxyEntry::Running(MachinePortProxyRegistration {
                port_bindings: manifest.spec.port_bindings.clone(),
                port_leases: manifest.port_leases.clone(),
                routes,
                proxies: running,
                lease_authority: Some(MachinePortProxyLeaseAuthority::Live(bind_authority)),
                publication_may_exist: true,
            }),
        );
        publish
            .take()
            .expect("publication runs exactly once for a started provider")()
    }

    #[cfg(test)]
    pub(super) fn withdraw_and_stop_machine_port_proxies(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        expected_port_bindings: &[SandboxPortBinding],
        expected_port_leases: &[PortLeaseRequest],
    ) -> Result<()> {
        let manager = self.port_lease_coordinator();
        let cleanup = self.begin_machine_port_proxy_cleanup(
            MachinePortProxyCleanupRequest {
                tenant_id,
                id,
                expected_port_bindings,
                expected_port_leases,
                disposition: MachinePortProxyCleanupDisposition::Release,
                port_lease_coordinator: manager,
            },
            || {},
        )?;
        if let Some(cleanup) = cleanup {
            self.confirm_machine_port_proxy_publication_absent(&cleanup)?;
            self.complete_machine_port_proxy_cleanup(&cleanup)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn stop_machine_port_proxies(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        expected_port_bindings: &[SandboxPortBinding],
        expected_port_leases: &[PortLeaseRequest],
    ) -> Result<()> {
        let manager = self.port_lease_coordinator();
        let cleanup = self.begin_machine_port_proxy_cleanup(
            MachinePortProxyCleanupRequest {
                tenant_id,
                id,
                expected_port_bindings,
                expected_port_leases,
                disposition: MachinePortProxyCleanupDisposition::Restart,
                port_lease_coordinator: manager,
            },
            || {},
        )?;
        if let Some(cleanup) = cleanup {
            self.confirm_machine_port_proxy_publication_absent(&cleanup)?;
            self.complete_machine_port_proxy_cleanup(&cleanup)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn begin_machine_port_proxy_restart(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        expected_port_bindings: &[SandboxPortBinding],
        expected_port_leases: &[PortLeaseRequest],
    ) -> Result<Option<MachinePortProxyCleanup>> {
        let manager = self.port_lease_coordinator();
        self.begin_machine_port_proxy_cleanup(
            MachinePortProxyCleanupRequest {
                tenant_id,
                id,
                expected_port_bindings,
                expected_port_leases,
                disposition: MachinePortProxyCleanupDisposition::Restart,
                port_lease_coordinator: manager,
            },
            || {},
        )
    }

    pub(super) fn begin_machine_port_proxy_restart_for_manifest(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<Option<MachinePortProxyCleanup>> {
        let manager = self.port_lease_coordinator_for_manifest(manifest)?;
        self.begin_machine_port_proxy_cleanup(
            MachinePortProxyCleanupRequest {
                tenant_id: &manifest.spec.tenant_id,
                id: &manifest.handle.id,
                expected_port_bindings: &manifest.spec.port_bindings,
                expected_port_leases: &manifest.port_leases,
                disposition: MachinePortProxyCleanupDisposition::Restart,
                port_lease_coordinator: manager,
            },
            || {},
        )
    }

    #[cfg(test)]
    pub(super) fn begin_machine_port_proxy_release(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        expected_port_bindings: &[SandboxPortBinding],
        expected_port_leases: &[PortLeaseRequest],
    ) -> Result<Option<MachinePortProxyCleanup>> {
        let manager = self.port_lease_coordinator();
        self.begin_machine_port_proxy_cleanup(
            MachinePortProxyCleanupRequest {
                tenant_id,
                id,
                expected_port_bindings,
                expected_port_leases,
                disposition: MachinePortProxyCleanupDisposition::Release,
                port_lease_coordinator: manager,
            },
            || {},
        )
    }

    pub(super) fn begin_machine_port_proxy_release_for_manifest(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<Option<MachinePortProxyCleanup>> {
        let manager = self.port_lease_coordinator_for_manifest(manifest)?;
        self.begin_machine_port_proxy_cleanup(
            MachinePortProxyCleanupRequest {
                tenant_id: &manifest.spec.tenant_id,
                id: &manifest.handle.id,
                expected_port_bindings: &manifest.spec.port_bindings,
                expected_port_leases: &manifest.port_leases,
                disposition: MachinePortProxyCleanupDisposition::Release,
                port_lease_coordinator: manager,
            },
            || {},
        )
    }

    fn begin_machine_port_proxy_cleanup(
        &self,
        request: MachinePortProxyCleanupRequest<'_>,
        before_registry_lock: impl FnOnce(),
    ) -> Result<Option<MachinePortProxyCleanup>> {
        let MachinePortProxyCleanupRequest {
            tenant_id,
            id,
            expected_port_bindings,
            expected_port_leases,
            disposition,
            port_lease_coordinator,
        } = request;
        if expected_port_bindings.len() != expected_port_leases.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot stop machine port proxies for tenant {tenant_id} sandbox {id}: \
                     {} expected bindings do not match {} durable leases",
                    expected_port_bindings.len(),
                    expected_port_leases.len()
                ),
            });
        }
        let key = (tenant_id.clone(), id.clone());
        before_registry_lock();
        let state = {
            let mut registrations = self.lock_machine_port_proxy_registry()?;
            let recovered = match registrations.get(&key) {
                None => {
                    if expected_port_leases.is_empty()
                        || port_lease_coordinator.machine_bindings_are_terminal_without_effect(
                            tenant_id,
                            id,
                            expected_port_bindings,
                            expected_port_leases,
                        )?
                    {
                        return Ok(None);
                    }
                    if disposition == MachinePortProxyCleanupDisposition::Restart
                        && port_lease_coordinator.classify_machine_cleanup_batch(
                            tenant_id,
                            id,
                            expected_port_bindings,
                            expected_port_leases,
                        )? == crate::backends::oci::port_lifecycle::LaunchPortBatchState::RestartRetained
                    {
                        return Ok(None);
                    }
                    Some(
                        port_lease_coordinator.recover_machine_bindings_after_owner_death(
                            tenant_id,
                            id,
                            expected_port_bindings,
                            expected_port_leases,
                        )?,
                    )
                }
                Some(MachinePortProxyEntry::Running(registration)) => {
                    if registration.port_bindings != expected_port_bindings
                        || registration.port_leases != expected_port_leases
                    {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "cannot stop machine port proxies for tenant {tenant_id} sandbox \
                                 {id}: process-local provider evidence does not match the expected \
                                 listener generation"
                            ),
                        });
                    }
                    None
                }
                Some(MachinePortProxyEntry::Stopping(state)) => {
                    let state = Arc::clone(state);
                    drop(registrations);
                    self.validate_machine_port_proxy_cleanup(
                        &state,
                        tenant_id,
                        id,
                        expected_port_bindings,
                        expected_port_leases,
                        disposition,
                    )?;
                    let cleanup = MachinePortProxyCleanup { key, state };
                    self.stop_machine_port_proxy_provider(&cleanup)?;
                    return Ok(Some(cleanup));
                }
            };

            let state = if let Some((expected_bindings, recoveries)) = recovered {
                Arc::new(Mutex::new(MachinePortProxyCleanupState {
                    disposition,
                    port_lease_coordinator,
                    registration: MachinePortProxyRegistration {
                        port_bindings: expected_port_bindings.to_vec(),
                        port_leases: expected_port_leases.to_vec(),
                        routes: Vec::new(),
                        proxies: Vec::new(),
                        lease_authority: Some(MachinePortProxyLeaseAuthority::Recovered(
                            recoveries,
                        )),
                        publication_may_exist: true,
                    },
                    expected_bindings,
                    // CleanupPending already fences new use. Process death
                    // proves the local proxy listener absent, but never proves
                    // the provider-managed external publication absent.
                    withdraw_complete: true,
                    provider_stopped: true,
                    publication_withdrawn: vec![false; expected_port_bindings.len()],
                    durable_transition_complete: false,
                }))
            } else {
                let expected_bindings = if expected_port_leases.is_empty() {
                    Vec::new()
                } else {
                    match disposition {
                        MachinePortProxyCleanupDisposition::Restart => port_lease_coordinator
                            .require_active_machine_bindings(
                                tenant_id,
                                id,
                                expected_port_bindings,
                                expected_port_leases,
                            )?,
                        MachinePortProxyCleanupDisposition::Release => port_lease_coordinator
                            .require_releasable_machine_bindings(
                                tenant_id,
                                id,
                                expected_port_bindings,
                                expected_port_leases,
                            )?,
                    }
                };
                let Some(MachinePortProxyEntry::Running(registration)) = registrations.remove(&key)
                else {
                    unreachable!("running registration was validated under the same lock");
                };
                let publication_withdrawn =
                    vec![!registration.publication_may_exist; registration.port_bindings.len()];
                Arc::new(Mutex::new(MachinePortProxyCleanupState {
                    disposition,
                    port_lease_coordinator,
                    registration,
                    expected_bindings,
                    withdraw_complete: disposition == MachinePortProxyCleanupDisposition::Restart,
                    provider_stopped: false,
                    publication_withdrawn,
                    durable_transition_complete: false,
                }))
            };
            registrations.insert(
                key.clone(),
                MachinePortProxyEntry::Stopping(Arc::clone(&state)),
            );
            state
        };
        let cleanup = MachinePortProxyCleanup { key, state };
        self.stop_machine_port_proxy_provider(&cleanup)?;
        Ok(Some(cleanup))
    }

    fn validate_machine_port_proxy_cleanup(
        &self,
        state: &Arc<Mutex<MachinePortProxyCleanupState>>,
        tenant_id: &TenantId,
        id: &SandboxId,
        expected_port_bindings: &[SandboxPortBinding],
        expected_port_leases: &[PortLeaseRequest],
        disposition: MachinePortProxyCleanupDisposition,
    ) -> Result<()> {
        let state = state.lock().map_err(|_| SandboxError::OperationFailed {
            message: "container machine port proxy cleanup lock is poisoned".to_owned(),
        })?;
        if state.disposition != disposition
            || state.registration.port_bindings != expected_port_bindings
            || state.registration.port_leases != expected_port_leases
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port proxy cleanup for tenant {tenant_id} sandbox {id} is already \
                     executing a different exact listener generation or disposition"
                ),
            });
        }
        Ok(())
    }

    fn lock_machine_port_proxy_registry(
        &self,
    ) -> Result<MutexGuard<'_, crate::backends::oci::network::MachinePortProxyEntries>> {
        self.machine_port_proxies.lock()
    }

    fn lock_machine_port_proxy_cleanup<'a>(
        &self,
        cleanup: &'a MachinePortProxyCleanup,
    ) -> Result<MutexGuard<'a, MachinePortProxyCleanupState>> {
        cleanup
            .state
            .lock()
            .map_err(|_| SandboxError::OperationFailed {
                message: "container machine port proxy cleanup lock is poisoned".to_owned(),
            })
    }

    fn stop_machine_port_proxy_provider(&self, cleanup: &MachinePortProxyCleanup) -> Result<()> {
        let mut state = self.lock_machine_port_proxy_cleanup(cleanup)?;
        if state.disposition == MachinePortProxyCleanupDisposition::Release
            && !state.withdraw_complete
        {
            if !state.registration.port_leases.is_empty() {
                let port_lease_coordinator = state.port_lease_coordinator.clone();
                port_lease_coordinator.withdraw_bindings(
                    &cleanup.key.0,
                    &cleanup.key.1,
                    &state.registration.port_bindings,
                    &state.registration.port_leases,
                )?;
            }
            state.withdraw_complete = true;
        }
        if state.provider_stopped {
            return Ok(());
        }
        let mut errors = Vec::new();
        for proxy in &mut state.registration.proxies {
            if let Err(error) = proxy.shutdown() {
                errors.push(error.to_string());
            }
        }
        if !errors.is_empty() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port proxy shutdown for tenant {} sandbox {} was unconfirmed: {}",
                    cleanup.key.0,
                    cleanup.key.1,
                    errors.join("; ")
                ),
            });
        }
        state.provider_stopped = true;
        Ok(())
    }

    pub(super) fn unexpose_machine_port_proxy_publications(
        &self,
        cleanup: &MachinePortProxyCleanup,
        forwarder: &OciMachinePortForwarderConfig,
    ) -> Result<()> {
        let mut state = self.lock_machine_port_proxy_cleanup(cleanup)?;
        let mut errors = Vec::new();
        for index in 0..state.registration.port_bindings.len() {
            if state.publication_withdrawn[index] {
                continue;
            }
            let binding = state.registration.port_bindings[index].clone();
            match unexpose_machine_ports(forwarder, std::slice::from_ref(&binding)) {
                Ok(()) => state.publication_withdrawn[index] = true,
                Err(error) => errors.push(format!(
                    "{}:{} withdrawal was unconfirmed: {error}",
                    binding.host_address, binding.host_port
                )),
            }
        }
        if !errors.is_empty() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port publication withdrawal for tenant {} sandbox {} was \
                     incomplete: {}",
                    cleanup.key.0,
                    cleanup.key.1,
                    errors.join("; ")
                ),
            });
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn confirm_machine_port_proxy_publication_absent(
        &self,
        cleanup: &MachinePortProxyCleanup,
    ) -> Result<()> {
        let mut state = self.lock_machine_port_proxy_cleanup(cleanup)?;
        state.publication_withdrawn.fill(true);
        Ok(())
    }

    pub(super) fn complete_machine_port_proxy_cleanup(
        &self,
        cleanup: &MachinePortProxyCleanup,
    ) -> Result<()> {
        {
            let mut state = self.lock_machine_port_proxy_cleanup(cleanup)?;
            if !state.provider_stopped {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "machine port proxy cleanup for tenant {} sandbox {} cannot complete \
                         before provider stop acknowledgement",
                        cleanup.key.0, cleanup.key.1
                    ),
                });
            }
            if state
                .publication_withdrawn
                .iter()
                .any(|withdrawn| !withdrawn)
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "machine port proxy cleanup for tenant {} sandbox {} cannot complete \
                         before every external publication is withdrawn",
                        cleanup.key.0, cleanup.key.1
                    ),
                });
            }
            if !state.durable_transition_complete {
                let port_lease_coordinator = state.port_lease_coordinator.clone();
                if !state.registration.port_leases.is_empty() {
                    let authority =
                        state.registration.lease_authority.as_ref().ok_or_else(|| {
                            SandboxError::OperationFailed {
                                message: format!(
                                    "machine port proxy cleanup for tenant {} sandbox {} lost its \
                                     exact lease-lifetime authority",
                                    cleanup.key.0, cleanup.key.1
                                ),
                            }
                        })?;
                    match (state.disposition, authority) {
                        (
                            MachinePortProxyCleanupDisposition::Restart,
                            MachinePortProxyLeaseAuthority::Live(lifetimes),
                        ) => {
                            port_lease_coordinator
                                .prepare_machine_bindings_for_rebind_with_lifetimes(
                                    &state.registration.port_leases,
                                    &state.expected_bindings,
                                    lifetimes,
                                )?;
                        }
                        (
                            MachinePortProxyCleanupDisposition::Release,
                            MachinePortProxyLeaseAuthority::Live(lifetimes),
                        ) => {
                            port_lease_coordinator
                                .release_machine_bindings_after_confirmed_stop_with_lifetimes(
                                    &state.registration.port_leases,
                                    &state.expected_bindings,
                                    lifetimes,
                                )?;
                        }
                        (
                            MachinePortProxyCleanupDisposition::Restart,
                            MachinePortProxyLeaseAuthority::Recovered(recoveries),
                        ) => {
                            port_lease_coordinator.prepare_recovered_machine_bindings_for_rebind(
                                &state.registration.port_leases,
                                &state.expected_bindings,
                                recoveries,
                            )?;
                        }
                        (
                            MachinePortProxyCleanupDisposition::Release,
                            MachinePortProxyLeaseAuthority::Recovered(recoveries),
                        ) => {
                            port_lease_coordinator.release_recovered_machine_bindings(
                                &state.registration.port_leases,
                                recoveries,
                            )?;
                        }
                    }
                }
                state.durable_transition_complete = true;
            }
        }

        let mut registrations = self.lock_machine_port_proxy_registry()?;
        let Some(entry) = registrations.get(&cleanup.key) else {
            return Ok(());
        };
        let MachinePortProxyEntry::Stopping(current) = entry else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port proxy cleanup for tenant {} sandbox {} no longer owns the \
                     registry tombstone",
                    cleanup.key.0, cleanup.key.1
                ),
            });
        };
        if !Arc::ptr_eq(current, &cleanup.state) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port proxy cleanup for tenant {} sandbox {} does not match the \
                     current registry tombstone",
                    cleanup.key.0, cleanup.key.1
                ),
            });
        }
        registrations.remove(&cleanup.key);
        Ok(())
    }
}

#[cfg(test)]
impl ContainerSandboxBackend {
    pub(super) fn ensure_machine_port_proxies_running_with_activation_failure(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
        activation_failure: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.ensure_machine_port_proxies_running_with_lifecycle_observers(
            id,
            assigned_ips,
            manifest,
            fresh_machine_port_release_authority(manifest)?,
            MachinePortProxyLifecycleHooks {
                before_activation: activation_failure,
                after_activation: || Ok(()),
                after_active_validation: || {},
                publish: || Ok(()),
            },
        )
    }

    pub(super) fn ensure_machine_port_proxies_running_with_activation_ack_loss(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
        activation_ack_loss: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.ensure_machine_port_proxies_running_with_lifecycle_observers(
            id,
            assigned_ips,
            manifest,
            fresh_machine_port_release_authority(manifest)?,
            MachinePortProxyLifecycleHooks {
                before_activation: || Ok(()),
                after_activation: activation_ack_loss,
                after_active_validation: || {},
                publish: || Ok(()),
            },
        )
    }

    pub(super) fn ensure_machine_port_proxies_running_at_validation_barrier(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
        after_active_validation: impl FnOnce(),
    ) -> Result<()> {
        self.ensure_machine_port_proxies_running_with_lifecycle_observers(
            id,
            assigned_ips,
            manifest,
            fresh_machine_port_release_authority(manifest)?,
            MachinePortProxyLifecycleHooks {
                before_activation: || Ok(()),
                after_activation: || Ok(()),
                after_active_validation,
                publish: || Ok(()),
            },
        )
    }

    pub(super) fn ensure_machine_port_proxies_running_at_publication_barrier(
        &self,
        id: &SandboxId,
        assigned_ips: &[Ipv4Addr],
        manifest: &ContainerSandboxManifest,
        publish: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.ensure_machine_port_proxies_running_with_lifecycle_observers(
            id,
            assigned_ips,
            manifest,
            fresh_machine_port_release_authority(manifest)?,
            MachinePortProxyLifecycleHooks {
                before_activation: || Ok(()),
                after_activation: || Ok(()),
                after_active_validation: || {},
                publish,
            },
        )
    }

    #[cfg(test)]
    pub(super) fn inject_partial_machine_proxy_start_shutdown_diagnostic(
        &self,
        manifest: &ContainerSandboxManifest,
        release_authority: MachinePortPreparationReleaseAuthority<'_>,
    ) -> Result<()> {
        let manager = self.port_lease_coordinator_for_manifest(manifest)?;
        let expected_bindings = manager.require_active_machine_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )?;
        let key = (manifest.spec.tenant_id.clone(), manifest.handle.id.clone());
        let mut registrations = self.lock_machine_port_proxy_registry()?;
        let Some(MachinePortProxyEntry::Running(mut registration)) = registrations.remove(&key)
        else {
            return Err(SandboxError::OperationFailed {
                message: "partial-start test requires one running machine proxy registration"
                    .to_owned(),
            });
        };
        for proxy in &mut registration.proxies {
            proxy.shutdown()?;
        }
        registration.proxies = vec![panicking_machine_port_proxy_for_test(
            (Ipv4Addr::LOCALHOST, 0).into(),
        )];
        registration.publication_may_exist = false;
        let disposition = partial_start_cleanup_disposition(release_authority);
        let state = Arc::new(Mutex::new(MachinePortProxyCleanupState {
            disposition,
            port_lease_coordinator: manager,
            registration,
            expected_bindings,
            withdraw_complete: disposition == MachinePortProxyCleanupDisposition::Restart,
            provider_stopped: false,
            publication_withdrawn: vec![true; manifest.spec.port_bindings.len()],
            durable_transition_complete: false,
        }));
        registrations.insert(
            key.clone(),
            MachinePortProxyEntry::Stopping(Arc::clone(&state)),
        );
        drop(registrations);
        self.stop_machine_port_proxy_provider(&MachinePortProxyCleanup { key, state })
    }

    pub(super) fn withdraw_and_stop_machine_port_proxies_at_lock_barrier(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        expected_port_bindings: &[SandboxPortBinding],
        expected_port_leases: &[PortLeaseRequest],
        before_registry_lock: impl FnOnce(),
    ) -> Result<()> {
        let manager = self.port_lease_coordinator();
        let cleanup = self.begin_machine_port_proxy_cleanup(
            MachinePortProxyCleanupRequest {
                tenant_id,
                id,
                expected_port_bindings,
                expected_port_leases,
                disposition: MachinePortProxyCleanupDisposition::Release,
                port_lease_coordinator: manager,
            },
            before_registry_lock,
        )?;
        if let Some(cleanup) = cleanup {
            self.confirm_machine_port_proxy_publication_absent(&cleanup)?;
            self.complete_machine_port_proxy_cleanup(&cleanup)?;
        }
        Ok(())
    }
}

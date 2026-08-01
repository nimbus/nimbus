use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU16;
use std::sync::{Arc, Mutex};

use nimbus::{
    CommitErrorClass, Error, SandboxBackend, SandboxBackendKind, SandboxError, SandboxHandle,
    SandboxId, SandboxOciImageSource, SandboxRootSpec, SandboxSpec,
};
use nimbus_machine::api::{
    MachineApiServiceSandboxStartResponse, MachineApiServiceSandboxStopResponse,
};
use nimbus_network::{
    LocalPortLeaseAuthority, NetworkPlanId, NetworkProviderHandle, PortBindClaim,
    PortBindingProvenance, PortBoundEndpoint, PortLeaseBinding, PortLeaseLifetimeGuard,
    PortLeasePhase, PortLeaseRequest, PortProtocol,
};
use nimbus_sandbox::{MachinePortForwardReceipt, SandboxFuture};
use ulid::Ulid;

use super::client::MachineApiClient;
use super::network_composition::HostMachineNetworkAuthority;
use super::publication_authority::{
    MachinePublicationIntent, MachinePublicationIntentPhase, MachinePublicationIntentStore,
    authenticate_exact_durable_plan, machine_host_bind_target, port_authority_error,
    recover_dead_batch,
};

#[derive(Clone)]
pub(crate) struct ForwardedMachineApiSandboxBackend {
    client: MachineApiClient,
    // Production construction always retains the process-composition token.
    // Only the test-only primitive constructor deliberately leaves this empty.
    _parent_network: Option<HostMachineNetworkAuthority>,
    port_leases: LocalPortLeaseAuthority,
    publication_intents: MachinePublicationIntentStore,
    live: Arc<Mutex<BTreeMap<NetworkPlanId, LiveMachinePublication>>>,
}

impl ForwardedMachineApiSandboxBackend {
    pub(crate) fn new(
        client: MachineApiClient,
        network: &HostMachineNetworkAuthority,
    ) -> Result<Self, Error> {
        Ok(Self {
            client,
            _parent_network: Some(network.clone()),
            port_leases: network.port_leases(),
            publication_intents: network.machine_publications()?,
            live: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        client: MachineApiClient,
        port_leases: LocalPortLeaseAuthority,
    ) -> Result<Self, Error> {
        Ok(Self {
            client,
            _parent_network: None,
            publication_intents: MachinePublicationIntentStore::open(port_leases.state_root())?,
            port_leases,
            live: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    fn start_sync(&self, spec: SandboxSpec) -> Result<SandboxHandle, Error> {
        let service_name = spec.service_name().ok_or_else(|| {
            Error::InvalidInput(format!(
                "forwarded machine API backend requires service-owned sandbox metadata for {}",
                spec.display_name()
            ))
        })?;
        let authority = self.client.forwarder_authority()?.clone();
        let mut intent = self.publication_intents.stage_service_attempt(
            &spec.tenant_id,
            service_name,
            &authority,
            &spec.port_bindings,
        )?;

        if intent.phase == MachinePublicationIntentPhase::Committed {
            return Err(Error::conflict(format!(
                "tenant {} service {} already has a committed machine publication attempt {}; \
                 reconcile or stop that exact sandbox before retrying",
                intent.tenant_id, intent.service_name, intent.sandbox_id
            )));
        }
        if self.live_contains(&intent.plan_id)? {
            return Err(Error::conflict(format!(
                "machine publication plan {} already has a live parent coordinator",
                intent.plan_id
            )));
        }

        let durable = self
            .port_leases
            .list_plan(&intent.plan_id)
            .map_err(port_authority_error)?;
        if !durable.is_empty() {
            self.release_staged_attempt_after_owner_death(&intent, &durable)?;
            self.publication_intents.mark_terminal(&intent.plan_id)?;
            intent = self.publication_intents.stage_service_attempt(
                &spec.tenant_id,
                service_name,
                &authority,
                &spec.port_bindings,
            )?;
        }

        let claims = match publication_claims(&intent) {
            Ok(claims) => claims,
            Err(error) => {
                let _ = self.publication_intents.mark_terminal(&intent.plan_id);
                return Err(error);
            }
        };
        let reservation = if claims.is_empty() {
            None
        } else {
            match self
                .port_leases
                .reserve_and_claim_provider_managed_batch_with_lifetimes(&claims)
            {
                Ok(reservation) => Some(reservation),
                Err(error) => {
                    if self
                        .port_leases
                        .list_plan(&intent.plan_id)
                        .is_ok_and(|records| records.is_empty())
                    {
                        let _ = self.publication_intents.mark_terminal(&intent.plan_id);
                    }
                    return Err(port_authority_error(error));
                }
            }
        };
        let lifetimes = reservation
            .map(|reservation| reservation.into_parts().1)
            .unwrap_or_default();
        self.insert_live(LiveMachinePublication {
            intent: intent.clone(),
            claims: claims.clone(),
            lifetimes,
            bindings: None,
        })?;

        if let Err(error) = self
            .publication_intents
            .commit_before_machine_api(&intent.plan_id)
        {
            self.compensate_uncommitted_live_attempt(&intent.plan_id);
            return Err(error);
        }

        let response = match &spec.root {
            SandboxRootSpec::Rootfs(_) => {
                return Err(Error::InvalidInput(format!(
                    "forwarded machine API backend requires an OCI image root for service \
                     sandbox {}; rootfs starts are not supported through this backing-plane API",
                    spec.display_name()
                )));
            }
            SandboxRootSpec::OciImage(image) => match &image.source {
                SandboxOciImageSource::Reference(_) => self
                    .client
                    .start_service_sandbox_from_image(intent.sandbox_id.clone(), spec)?,
                SandboxOciImageSource::Build(_) => self
                    .client
                    .start_service_sandbox_from_build(intent.sandbox_id.clone(), spec)?,
            },
        };
        self.activate_exact_response(&intent.plan_id, &response)?;
        Ok(response.handle)
    }

    fn stop_sync(&self, sandbox_id: &SandboxId) -> Result<(), Error> {
        let plan_id = parse_machine_publication_sandbox_id(sandbox_id)?;
        let intent = self
            .publication_intents
            .load_plan(&plan_id)?
            .ok_or_else(|| Error::NotFound(format!("machine publication plan {plan_id}")))?;
        self.client
            .forwarder_authority()?
            .authenticate(&intent.forwarder_authority)
            .map_err(|error| Error::PreconditionFailed(error.to_string()))?;

        if intent.phase == MachinePublicationIntentPhase::Terminal {
            return Ok(());
        }
        if let Some(mut live) = self.remove_live(&plan_id)? {
            let requests = live.requests();
            if !requests.is_empty()
                && let Err(error) = self
                    .port_leases
                    .withdraw_provider_managed_batch_with_lifetimes(&requests, &live.lifetimes)
            {
                self.reinsert_live(live);
                return Err(port_authority_error(error));
            }
            let response = match self.client.stop_service_sandbox(
                &intent.tenant_id,
                &intent.sandbox_id,
                &intent.bindings,
            ) {
                Ok(response) => response,
                Err(error) => {
                    self.reinsert_live(live);
                    return Err(error);
                }
            };
            if let Err(error) = self.release_live_after_exact_stop(&mut live, &response) {
                self.reinsert_live(live);
                return Err(error);
            }
            self.publication_intents.mark_terminal(&plan_id)?;
            return Ok(());
        }

        self.stop_after_fresh_recovery(&intent)
    }

    fn activate_exact_response(
        &self,
        plan_id: &NetworkPlanId,
        response: &MachineApiServiceSandboxStartResponse,
    ) -> Result<(), Error> {
        let mut live = self.remove_live(plan_id)?.ok_or_else(|| {
            Error::PreconditionFailed(format!(
                "machine publication plan {plan_id} lost its live parent coordinator"
            ))
        })?;
        let activation = live
            .claims
            .iter()
            .zip(&response.publication_evidence)
            .map(|((request, claim), receipt)| {
                Ok((
                    request.clone(),
                    claim.clone(),
                    binding_from_receipt(request, receipt)?,
                ))
            })
            .collect::<Result<Vec<_>, Error>>();
        let activation = match activation {
            Ok(activation) => activation,
            Err(error) => {
                self.reinsert_live(live);
                return Err(error);
            }
        };
        let result = if activation.is_empty() {
            Ok(Vec::new())
        } else {
            self.port_leases
                .adopt_claimed_and_activate_batch_with_lifetimes(&activation, None, &live.lifetimes)
                .map_err(port_authority_error)
        };
        match result {
            Ok(_) => {
                live.bindings = Some(
                    activation
                        .iter()
                        .map(|(request, _, binding)| (request.clone(), binding.clone()))
                        .collect(),
                );
                self.reinsert_live(live);
                Ok(())
            }
            Err(error) => {
                self.reinsert_live(live);
                Err(error)
            }
        }
    }

    fn release_live_after_exact_stop(
        &self,
        live: &mut LiveMachinePublication,
        _response: &MachineApiServiceSandboxStopResponse,
    ) -> Result<(), Error> {
        if let Some(bindings) = &live.bindings {
            if !bindings.is_empty() {
                self.port_leases
                    .release_provider_managed_batch_after_confirmed_stop_with_lifetimes(
                        bindings,
                        &live.lifetimes,
                    )
                    .map_err(port_authority_error)?;
            }
        } else if !live.claims.is_empty() {
            self.port_leases
                .release_provider_managed_claim_batch_after_confirmed_absence_with_lifetimes(
                    &live.claims,
                    &live.lifetimes,
                )
                .map_err(port_authority_error)?;
        }
        Ok(())
    }

    fn stop_after_fresh_recovery(&self, intent: &MachinePublicationIntent) -> Result<(), Error> {
        let records = self
            .port_leases
            .list_plan(&intent.plan_id)
            .map_err(port_authority_error)?;
        if records.is_empty() && intent.bindings.is_empty() {
            self.client.stop_service_sandbox(
                &intent.tenant_id,
                &intent.sandbox_id,
                &intent.bindings,
            )?;
            self.publication_intents.mark_terminal(&intent.plan_id)?;
            return Ok(());
        }
        if !records.is_empty()
            && records
                .iter()
                .all(|record| record.phase() == PortLeasePhase::Released)
        {
            self.publication_intents.mark_terminal(&intent.plan_id)?;
            return Ok(());
        }
        let expected = publication_claims(intent)?
            .into_iter()
            .map(|(request, _)| request)
            .collect::<Vec<_>>();
        authenticate_exact_durable_plan(&expected, &records)?;
        let recoveries = recover_dead_batch(&self.port_leases, &expected)?;
        self.port_leases
            .mark_cleanup_pending_batch_after_owner_death(&expected, &recoveries)
            .map_err(port_authority_error)?;
        self.client.stop_service_sandbox(
            &intent.tenant_id,
            &intent.sandbox_id,
            &intent.bindings,
        )?;
        self.port_leases
            .release_provider_managed_batch_after_confirmed_stop(&expected, &recoveries)
            .map_err(port_authority_error)?;
        self.publication_intents.mark_terminal(&intent.plan_id)?;
        Ok(())
    }

    fn release_staged_attempt_after_owner_death(
        &self,
        intent: &MachinePublicationIntent,
        records: &[nimbus_network::PortLeaseRecord],
    ) -> Result<(), Error> {
        let expected = publication_claims(intent)?
            .into_iter()
            .map(|(request, _)| request)
            .collect::<Vec<_>>();
        authenticate_exact_durable_plan(&expected, records)?;
        if records.iter().all(|record| record.phase().is_terminal()) {
            return Ok(());
        }
        let recoveries = recover_dead_batch(&self.port_leases, &expected)?;
        self.port_leases
            .mark_cleanup_pending_batch_after_owner_death(&expected, &recoveries)
            .map_err(port_authority_error)?;
        self.port_leases
            .release_provider_managed_batch_after_confirmed_stop(&expected, &recoveries)
            .map_err(port_authority_error)?;
        Ok(())
    }

    fn compensate_uncommitted_live_attempt(&self, plan_id: &NetworkPlanId) {
        let Ok(Some(live)) = self.remove_live(plan_id) else {
            return;
        };
        let released = live.claims.is_empty()
            || self
                .port_leases
                .release_provider_managed_claim_batch_after_confirmed_absence_with_lifetimes(
                    &live.claims,
                    &live.lifetimes,
                )
                .is_ok();
        if released {
            let _ = self.publication_intents.mark_terminal(plan_id);
        } else {
            self.reinsert_live(live);
        }
    }

    fn live_contains(&self, plan_id: &NetworkPlanId) -> Result<bool, Error> {
        Ok(self
            .live
            .lock()
            .map_err(|_| poisoned_live_state())?
            .contains_key(plan_id))
    }

    fn insert_live(&self, live: LiveMachinePublication) -> Result<(), Error> {
        use std::collections::btree_map::Entry;

        let plan_id = live.intent.plan_id.clone();
        match self
            .live
            .lock()
            .map_err(|_| poisoned_live_state())?
            .entry(plan_id.clone())
        {
            Entry::Vacant(entry) => {
                entry.insert(live);
                Ok(())
            }
            Entry::Occupied(_) => Err(Error::conflict(format!(
                "machine publication plan {plan_id} already has a live parent coordinator"
            ))),
        }
    }

    fn remove_live(
        &self,
        plan_id: &NetworkPlanId,
    ) -> Result<Option<LiveMachinePublication>, Error> {
        Ok(self
            .live
            .lock()
            .map_err(|_| poisoned_live_state())?
            .remove(plan_id))
    }

    fn reinsert_live(&self, live: LiveMachinePublication) {
        let plan_id = live.intent.plan_id.clone();
        self.live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(plan_id, live);
    }
}

impl fmt::Debug for ForwardedMachineApiSandboxBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForwardedMachineApiSandboxBackend")
            .field("client", &self.client)
            .field("parent_state_root", &self.port_leases.state_root())
            .finish_non_exhaustive()
    }
}

struct LiveMachinePublication {
    intent: MachinePublicationIntent,
    claims: Vec<(PortLeaseRequest, PortBindClaim)>,
    lifetimes: Vec<PortLeaseLifetimeGuard>,
    bindings: Option<Vec<(PortLeaseRequest, PortLeaseBinding)>>,
}

impl LiveMachinePublication {
    fn requests(&self) -> Vec<PortLeaseRequest> {
        self.claims
            .iter()
            .map(|(request, _)| request.clone())
            .collect()
    }
}

impl SandboxBackend for ForwardedMachineApiSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        if spec.service_name().is_none() {
            let message = format!(
                "forwarded machine API backend requires service-owned sandbox metadata for {}; standalone sandboxes are not supported through this backing-plane API",
                spec.display_name()
            );
            return Box::pin(async move { Err(SandboxError::InvalidSpec { message }) });
        }

        match &spec.root {
            SandboxRootSpec::Rootfs(_) => {
                let message = format!(
                    "forwarded machine API backend requires an OCI image root for service sandbox {}; rootfs starts are not supported through this backing-plane API",
                    spec.display_name()
                );
                Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
            }
            SandboxRootSpec::OciImage(image) => match &image.source {
                SandboxOciImageSource::Reference(_) => {
                    let backend = self.clone();
                    spawn_machine_api_operation("image-start", move || backend.start_sync(spec))
                }
                SandboxOciImageSource::Build(_) => {
                    let backend = self.clone();
                    spawn_machine_api_operation("build-start", move || backend.start_sync(spec))
                }
            },
        }
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<nimbus_sandbox::SandboxInspection>> {
        let sandbox_id = id.clone();
        let client = self.client.clone();
        spawn_machine_api_operation("inspect", move || {
            client.inspect_service_sandbox(&sandbox_id)
        })
    }

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
        let sandbox_id = id.clone();
        let backend = self.clone();
        spawn_machine_api_operation("stop", move || backend.stop_sync(&sandbox_id))
    }
}

fn spawn_machine_api_operation<T, F>(operation: &'static str, callback: F) -> SandboxFuture<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, Error> + Send + 'static,
{
    Box::pin(async move {
        tokio::task::spawn_blocking(callback)
            .await
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("forwarded machine API {operation} task failed to join: {error}"),
            })?
            .map_err(machine_client_error_to_sandbox_error)
    })
}

fn publication_claims(
    intent: &MachinePublicationIntent,
) -> Result<Vec<(PortLeaseRequest, PortBindClaim)>, Error> {
    intent
        .requests()?
        .into_iter()
        .zip(&intent.bindings)
        .map(|(request, binding)| {
            let attempt = NetworkProviderHandle::new(
                intent
                    .forwarder_authority
                    .provider_instance()
                    .provider_id()
                    .clone(),
                format!(
                    "machine-publication:{}:{}:{}",
                    intent.plan_id,
                    binding.name,
                    Ulid::new()
                ),
            )
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to create provider-scoped bind claim for machine publication {}: \
                     {error}",
                    intent.plan_id
                ))
            })?;
            Ok((request, PortBindClaim::new(attempt)))
        })
        .collect()
}

fn binding_from_receipt(
    request: &PortLeaseRequest,
    receipt: &MachinePortForwardReceipt,
) -> Result<PortLeaseBinding, Error> {
    let port = NonZeroU16::new(receipt.binding.host_port).ok_or_else(|| {
        Error::PreconditionFailed("machine publication receipt reported host port zero".to_owned())
    })?;
    let endpoint = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        nimbus_network::PortBindRealm::Host,
        machine_host_bind_target(receipt.binding.host_address)?,
        port,
    )
    .map_err(|error| {
        Error::PreconditionFailed(format!(
            "machine publication receipt has invalid bound endpoint: {error}"
        ))
    })?;
    let binding = PortLeaseBinding::new(
        endpoint,
        PortBindingProvenance::NimbusOwned,
        receipt.provider_instance.clone(),
    );
    if receipt.binding.name.is_empty() || request.tenant_id() != Some(&receipt.tenant_id) {
        return Err(Error::PreconditionFailed(
            "machine publication receipt has invalid listener or tenant identity".to_owned(),
        ));
    }
    Ok(binding)
}

fn parse_machine_publication_sandbox_id(sandbox_id: &SandboxId) -> Result<NetworkPlanId, Error> {
    sandbox_id
        .as_str()
        .strip_prefix("machine-api:")
        .ok_or_else(|| {
            Error::InvalidInput(
                "forwarded machine sandbox identity lacks the machine-api plan domain".to_owned(),
            )
        })?
        .parse()
        .map_err(|error| {
            Error::InvalidInput(format!(
                "forwarded machine sandbox identity has an invalid network plan: {error}"
            ))
        })
}

fn poisoned_live_state() -> Error {
    Error::Internal("parent machine publication runtime state was poisoned".to_owned())
}

fn machine_client_error_to_sandbox_error(error: Error) -> SandboxError {
    let rendered = error.to_string();
    if let Some(class) = error.commit_class() {
        return match class {
            CommitErrorClass::Conflict | CommitErrorClass::OutOfRetention => {
                SandboxError::OperationFailed { message: rendered }
            }
            CommitErrorClass::Overloaded
            | CommitErrorClass::CommitterFull
            | CommitErrorClass::RejectedBeforeExecution
            | CommitErrorClass::RateLimited => {
                SandboxError::BackendUnavailable { message: rendered }
            }
            CommitErrorClass::CapExceeded => SandboxError::InvalidSpec { message: rendered },
        };
    }

    match error {
        Error::InvalidInput(_)
        | Error::MissingIndex { .. }
        | Error::SchemaValidation(_)
        | Error::SchemaNotFound(_)
        | Error::HistoricalRead { .. }
        | Error::Serialization(_) => SandboxError::InvalidSpec { message: rendered },
        Error::ResourceExhausted(_)
        | Error::PermissionDenied(_)
        | Error::Storage { .. }
        | Error::Transport(_) => SandboxError::BackendUnavailable { message: rendered },
        Error::Internal(message)
            if message.contains("failed to connect to machine API socket")
                || message.contains("failed to read machine API response")
                || message.contains("machine API response from")
                || message.contains("machine API request") =>
        {
            SandboxError::BackendUnavailable { message: rendered }
        }
        Error::AlreadyExists(_)
        | Error::PreconditionFailed(_)
        | Error::Cancelled
        | Error::TenantNotFound(_)
        | Error::DocumentNotFound(_)
        | Error::ScheduledJobNotFound(_)
        | Error::NotFound(_)
        | Error::Internal(_) => SandboxError::OperationFailed { message: rendered },
        _ => SandboxError::OperationFailed { message: rendered },
    }
}

#[cfg(test)]
mod tests {
    mod publication_authority;

    use std::collections::BTreeMap;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use nimbus::{
        EndpointProtocol, Error, PublishedEndpoint, SandboxBackend, SandboxBackendKind,
        SandboxError, SandboxHandle, SandboxId, SandboxOwnerSpec, SandboxPortBinding,
        SandboxProcessSpec, SandboxRootSpec, SandboxSpec, SandboxStatus, TenantId,
    };
    use nimbus_machine::MachineForwarderAuthority;
    use nimbus_network::{
        LocalPortLeaseAuthority, NetworkProviderHandle, NetworkProviderId,
        NetworkResourceGeneration,
    };
    use nimbus_sandbox::SandboxFuture;
    use serde_json::json;
    use tempfile::{Builder, TempDir};

    use super::{
        ForwardedMachineApiSandboxBackend, MachineApiClient, machine_client_error_to_sandbox_error,
    };
    use crate::machine::{
        MachineApiListenMode, MachineApiState, bind_direct_listener,
        default_guest_helper_binary_dirs, machine_api_node_workload_facade_from_sandbox_backend,
        serve_machine_api,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_round_trips_image_build_inspect_and_stop_over_machine_api() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let control_data_dir = temp_dir.path().join("control");
        let state_root = machine_api_container_state_root(&control_data_dir);
        let authority = test_forwarder_authority();
        let state = MachineApiState {
            control_data_dir,
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: None,
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_workloads: Some(machine_api_node_workload_facade_from_sandbox_backend(
                std::sync::Arc::new(StubMachineApiSandboxBackend::with_state_root(state_root)),
            )),
            machine_port_forwarder: None,
            forwarder_authority: Some(authority.clone()),
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));

        let backend = ForwardedMachineApiSandboxBackend::new_for_test(
            MachineApiClient::new_for_test(socket_path.clone()).with_forwarder_authority(authority),
            LocalPortLeaseAuthority::open(temp_dir.path().join("parent-network"))
                .expect("parent authority should open"),
        )
        .expect("forwarded backend should compose");
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let image_handle = backend
            .start(without_port_bindings(image_spec(
                &tenant,
                "db",
                "docker://busybox:latest",
            )))
            .await
            .expect("image-backed start should succeed");
        assert_eq!(image_handle.backend, SandboxBackendKind::Container);
        assert_eq!(image_handle.status, SandboxStatus::Ready);
        assert!(image_handle.published_endpoints.is_empty());

        let inspected = backend
            .inspect(&image_handle.id)
            .await
            .expect("inspect should succeed")
            .expect("handle should exist");
        assert_eq!(inspected.handle, image_handle);

        backend
            .stop(&image_handle.id)
            .await
            .expect("stop should succeed");
        assert!(
            backend
                .inspect(&image_handle.id)
                .await
                .expect("inspect after stop should succeed")
                .is_none()
        );

        let build_handle = backend
            .start(without_port_bindings(build_spec(
                &tenant,
                "api",
                "api-image",
                "/Users/jack/src/github.com/nimbus/nimbus/Dockerfile",
                "/Users/jack/src/github.com/nimbus/nimbus",
            )))
            .await
            .expect("build-backed start should succeed");
        assert_eq!(build_handle.name, "api");

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_maps_missing_socket_to_backend_unavailable() {
        let temp_dir = short_socket_tempdir();
        let backend = ForwardedMachineApiSandboxBackend::new_for_test(
            MachineApiClient::new("/tmp/nimbus-missing.sock")
                .with_forwarder_authority(test_forwarder_authority()),
            LocalPortLeaseAuthority::open(temp_dir.path().join("parent-network"))
                .expect("parent authority should open"),
        )
        .expect("forwarded backend should compose");
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let error = backend
            .start(image_spec(&tenant, "db", "docker://busybox:latest"))
            .await
            .expect_err("missing socket should fail");
        assert!(
            matches!(error, SandboxError::BackendUnavailable { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn machine_client_missing_index_error_maps_to_invalid_spec() {
        let error = machine_client_error_to_sandbox_error(Error::MissingIndex {
            fields: vec!["state".to_string(), "rank".to_string()],
        });

        assert!(
            matches!(error, SandboxError::InvalidSpec { .. }),
            "{error:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_rejects_rootfs_specs() {
        let temp_dir = short_socket_tempdir();
        let backend = ForwardedMachineApiSandboxBackend::new_for_test(
            MachineApiClient::new("/tmp/nimbus-unused.sock"),
            LocalPortLeaseAuthority::open(temp_dir.path().join("parent-network"))
                .expect("parent authority should open"),
        )
        .expect("forwarded backend should compose");
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let error = backend
            .start(rootfs_spec(&tenant, "db"))
            .await
            .expect_err("rootfs specs should fail");
        assert!(
            matches!(error, SandboxError::InvalidSpec { .. }),
            "{error:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_rejects_standalone_specs_before_machine_api_io() {
        let temp_dir = short_socket_tempdir();
        let backend = ForwardedMachineApiSandboxBackend::new_for_test(
            MachineApiClient::new("/tmp/nimbus-unused.sock"),
            LocalPortLeaseAuthority::open(temp_dir.path().join("parent-network"))
                .expect("parent authority should open"),
        )
        .expect("forwarded backend should compose");
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let mut spec = image_spec(&tenant, "db", "docker://busybox:latest");
        spec.owner = SandboxOwnerSpec::standalone_named("scratch-db");

        let error = backend
            .start(spec)
            .await
            .expect_err("standalone specs should fail before machine API I/O");

        let SandboxError::InvalidSpec { message } = error else {
            panic!("expected InvalidSpec for standalone spec, got {error:?}");
        };
        assert!(
            message.contains("requires service-owned sandbox metadata"),
            "{message}"
        );
    }

    fn rootfs_spec(tenant: &TenantId, name: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant.clone(),
            SandboxOwnerSpec::service(name),
            SandboxBackendKind::Container,
            SandboxRootSpec::rootfs("/"),
            SandboxProcessSpec::new(["sleep", "60"]),
        )
        .with_port_binding(SandboxPortBinding::new(
            "http",
            EndpointProtocol::Http,
            18080,
            8080,
        ))
    }

    fn image_spec(tenant: &TenantId, name: &str, image_reference: &str) -> SandboxSpec {
        let mut spec = rootfs_spec(tenant, name);
        spec.root = SandboxRootSpec::oci_image_reference(image_reference);
        spec
    }

    fn build_spec(
        tenant: &TenantId,
        name: &str,
        image_name: &str,
        dockerfile_path: impl Into<std::path::PathBuf>,
        context_path: impl Into<std::path::PathBuf>,
    ) -> SandboxSpec {
        let mut spec = rootfs_spec(tenant, name);
        spec.root = SandboxRootSpec::oci_image_build(image_name, dockerfile_path, context_path);
        spec
    }

    fn without_port_bindings(mut spec: SandboxSpec) -> SandboxSpec {
        spec.port_bindings.clear();
        spec
    }

    fn short_socket_tempdir() -> TempDir {
        Builder::new()
            .prefix("nimbus-mac-")
            .tempdir_in("/tmp")
            .expect("short temp dir should exist")
    }

    fn test_forwarder_authority() -> MachineForwarderAuthority {
        MachineForwarderAuthority::new(
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key("machine-backend-test-gvproxy"),
                "machine-backend-test-provider",
            )
            .expect("test provider handle should validate"),
            NetworkResourceGeneration::new(1),
        )
    }

    #[derive(Default)]
    struct StubMachineApiSandboxBackend {
        next_id: AtomicUsize,
        handles: Mutex<BTreeMap<String, SandboxHandle>>,
        state_root: Option<PathBuf>,
    }

    fn machine_api_container_state_root(control_data_dir: &std::path::Path) -> PathBuf {
        control_data_dir
            .join("service-sandboxes")
            .join("container")
            .join("state")
    }

    impl StubMachineApiSandboxBackend {
        fn with_state_root(state_root: PathBuf) -> Self {
            Self {
                state_root: Some(state_root),
                ..Self::default()
            }
        }

        fn start_with_spec(&self, spec: &SandboxSpec) -> SandboxHandle {
            let sequence = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
            let service_name = spec.display_name().to_owned();
            let sandbox_id = SandboxId::new(format!("{service_name}-{sequence}"));
            let endpoints = spec
                .port_bindings
                .iter()
                .map(|binding| {
                    PublishedEndpoint::new(
                        binding.name.clone(),
                        binding.protocol,
                        SocketAddr::new(
                            IpAddr::V4(Ipv4Addr::LOCALHOST),
                            binding.host_socket_addr().port(),
                        ),
                    )
                })
                .collect::<Vec<_>>();
            let handle = SandboxHandle::new(
                spec.tenant_id.clone(),
                sandbox_id.clone(),
                service_name,
                SandboxBackendKind::Container,
                SandboxStatus::Ready,
                endpoints,
            );
            self.handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .insert(sandbox_id.as_str().to_owned(), handle.clone());
            if let Some(state_root) = self.state_root.as_ref() {
                write_stub_container_manifest(state_root, &handle, spec);
            }
            handle
        }

        fn remove_manifest(&self, id: &SandboxId) {
            let Some(state_root) = self.state_root.as_ref() else {
                return;
            };
            let Some(handle) = self
                .handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .get(id.as_str())
                .cloned()
            else {
                return;
            };
            let manifest_path = state_root
                .join("tenants")
                .join(handle.tenant_id.as_str())
                .join("sandboxes")
                .join(id.as_str())
                .join("state")
                .join("containers")
                .join(id.as_str())
                .join("manifest.json");
            let _ = fs::remove_file(manifest_path);
        }
    }

    impl SandboxBackend for StubMachineApiSandboxBackend {
        fn kind(&self) -> SandboxBackendKind {
            SandboxBackendKind::Container
        }

        fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
            let handle = self.start_with_spec(&spec);
            Box::pin(async move { Ok(handle) })
        }

        fn inspect(
            &self,
            id: &SandboxId,
        ) -> SandboxFuture<Option<nimbus_sandbox::SandboxInspection>> {
            let handle = self
                .handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .get(id.as_str())
                .cloned();
            Box::pin(
                async move { Ok(handle.map(nimbus_sandbox::SandboxInspection::provider_reported)) },
            )
        }

        fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
            self.remove_manifest(id);
            self.handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .remove(id.as_str());
            Box::pin(async move { Ok(()) })
        }
    }

    fn write_stub_container_manifest(
        state_root: &std::path::Path,
        handle: &SandboxHandle,
        spec: &SandboxSpec,
    ) {
        let container_dir = state_root
            .join("tenants")
            .join(handle.tenant_id.as_str())
            .join("sandboxes")
            .join(handle.id.as_str())
            .join("state")
            .join("containers")
            .join(handle.id.as_str());
        fs::create_dir_all(&container_dir).expect("stub manifest directory should exist");
        let manifest = json!({
            "handle": handle,
            "spec": spec,
            "conmon_layout": {
                "container_state_dir": container_dir,
                "ctr_log": container_dir.join("ctr.log"),
                "oci_log": container_dir.join("oci.log")
            },
            "last_exit_code": null,
            "shutdown_requested": false,
            "status": handle.status
        });

        fs::write(
            container_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("stub manifest should serialize"),
        )
        .expect("stub manifest should write");
    }
}

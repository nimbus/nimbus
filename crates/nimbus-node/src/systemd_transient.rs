use std::sync::Arc;

use nimbus_core::{Error, Result};
use nimbus_workloads::{WorkloadExecutionReference, WorkloadProvisionDispatchClaim};
use serde::Serialize;

use super::{
    HostBackendObservedState, HostLifecycleBackend, HostLifecycleBackendCapabilities,
    HostLifecycleBackendKind, HostLifecycleFuture, HostLifecycleJournalSelectorEvidence,
    HostLifecyclePlan, HostLifecycleProperty, HostLifecycleRequest, HostLifecycleStatus,
    HostLifecycleStatusReason, HostRestartPolicy, LocalEnforcementBinding, SystemdUnitKind,
    SystemdUnitName, TenantWorkloadLifecycleEvidence, TenantWorkloadPhase, WorkloadExecutionId,
};
use crate::host_lifecycle::{HostActivationFence, HostProviderPlan, HostRestartProviderClaim};

const WORKLOAD_EXECUTION_JOURNAL_FIELD: &str = "NIMBUS_WORKLOAD_EXECUTION_ID";

/// Live `zbus_systemd`-backed `SystemdDbusClient`. Present only on Linux when
/// the `systemd-dbus` feature is enabled; otherwise the backend keeps its
/// fail-closed `UnavailableSystemdDbusClient` default.
#[cfg(all(target_os = "linux", feature = "systemd-dbus"))]
pub mod zbus_client;

pub trait SystemdDbusClient: Send + Sync + 'static {
    fn capabilities(&self) -> SystemdTransientCapabilities;

    fn start_transient_unit<'a>(
        &'a self,
        request: SystemdStartTransientUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse>;

    fn stop_unit<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse>;

    fn inspect_unit<'a>(
        &'a self,
        request: SystemdInspectUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdUnitStatus>;
}

#[derive(Debug, Clone)]
pub struct SystemdTransientUnitBackend<C = UnavailableSystemdDbusClient> {
    client: Arc<C>,
}

impl SystemdTransientUnitBackend<UnavailableSystemdDbusClient> {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::new(UnavailableSystemdDbusClient::new(reason))
    }
}

#[cfg(all(target_os = "linux", feature = "systemd-dbus"))]
impl SystemdTransientUnitBackend<zbus_client::ZbusSystemdClient> {
    /// The default Linux production backend: a live `ZbusSystemdClient` bound
    /// to the system bus.
    ///
    /// `ZbusSystemdClient::new` is async and fallible (it opens a D-Bus
    /// connection and probes capabilities), so this is an explicit factory
    /// rather than a `Default`/type-parameter default — a generic default type
    /// parameter cannot construct the live client by itself. Non-Linux builds
    /// keep the fail-closed `SystemdTransientUnitBackend::unavailable(...)`
    /// path. Returns `Err` if the system bus cannot be opened (callers may fall
    /// back to `unavailable`).
    pub async fn linux_systemd_default() -> Result<Self> {
        let client = zbus_client::ZbusSystemdClient::new(zbus_client::BusKind::System).await?;
        Ok(Self::new(client))
    }
}

impl<C> SystemdTransientUnitBackend<C>
where
    C: SystemdDbusClient,
{
    pub fn new(client: C) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    fn ensure_capable(&self) -> Result<()> {
        self.client.capabilities().ensure_required()
    }

    pub fn backend_capabilities(&self) -> HostLifecycleBackendCapabilities {
        self.client.capabilities().to_backend_capabilities()
    }

    async fn activate_provider_exact(
        &self,
        plan: &HostProviderPlan,
    ) -> Result<HostLifecycleStatus> {
        self.ensure_capable()?;
        let request = SystemdStartTransientUnitRequest::from_provider_plan(plan)?;
        let expected_fence = request.activation_fence().cloned();
        if let Some(expected_fence) = expected_fence.as_ref() {
            let observed = self
                .client
                .inspect_unit(SystemdInspectUnitRequest::for_execution(
                    plan.execution_id().clone(),
                )?)
                .await?;
            if let Some(adopted) = adopt_exact_systemd_observation(expected_fence, &observed)? {
                return Ok(adopted);
            }
        }
        let response = match self.client.start_transient_unit(request.clone()).await {
            Ok(response) => response,
            Err(start_error) => {
                let Some(expected_fence) = expected_fence.as_ref() else {
                    return Err(start_error);
                };
                let observed = self
                    .client
                    .inspect_unit(SystemdInspectUnitRequest::for_execution(
                        plan.execution_id().clone(),
                    )?)
                    .await;
                match observed {
                    Ok(observed) => {
                        if let Some(adopted) =
                            adopt_exact_systemd_observation(expected_fence, &observed)?
                        {
                            return Ok(adopted);
                        }
                        return Err(start_error);
                    }
                    Err(_) => return Err(start_error),
                }
            }
        };
        if response.unit_name() != plan.unit_name() {
            return Err(Error::InvalidInput(format!(
                "systemd StartTransientUnit returned unit {}, but plan requested {}",
                response.unit_name().as_str(),
                plan.unit_name().as_str()
            )));
        }
        let lifecycle_evidence = TenantWorkloadLifecycleEvidence::from_provider_plan(
            plan,
            HostLifecycleStatusReason::Submitted,
        )
        .with_job_path(response.job_path())?
        .with_cgroup_path(request.cgroup_path())?
        .with_journal_selectors(journal_selector_evidence(request.journal_selectors())?);
        Ok(
            HostLifecycleStatus::from_provider_state(plan, HostBackendObservedState::Submitted)
                .with_lifecycle_evidence(lifecycle_evidence),
        )
    }

    async fn inspect_provider(&self, plan: &HostProviderPlan) -> Result<HostLifecycleStatus> {
        self.ensure_capable()?;
        let observed = self
            .client
            .inspect_unit(SystemdInspectUnitRequest::for_execution(
                plan.execution_id().clone(),
            )?)
            .await?;
        if let Some(expected_fence) = plan.activation_fence() {
            if observed.is_absent() {
                return Err(Error::NotFound(format!(
                    "systemd unit {} is absent",
                    plan.unit_name().as_str()
                )));
            }
            ensure_exact_systemd_fence(expected_fence, &observed)?;
        }
        observed.to_host_lifecycle_status()
    }

    async fn inspect_restart_source(
        &self,
        claim: &HostRestartProviderClaim,
    ) -> Result<SystemdUnitStatus> {
        self.ensure_capable()?;
        let observed = self
            .client
            .inspect_unit(SystemdInspectUnitRequest::for_execution(
                claim.source_execution().execution_id().clone(),
            )?)
            .await?;
        if observed.is_absent() {
            claim.require_step(nimbus_workloads::WorkloadRestartStep::QuiesceExecution)?;
            return Ok(observed);
        }
        observed
            .activation_fence()
            .ok_or_else(|| {
                Error::PermissionDenied(format!(
                    "systemd restart source unit {} has no retained activation fence",
                    observed.unit_name().as_str()
                ))
            })?
            .authenticate_restart_source(claim)?;
        Ok(observed)
    }

    async fn inspect_restart_target(
        &self,
        claim: &HostRestartProviderClaim,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecycleStatus> {
        let plan = HostProviderPlan::from_restart(claim, request)?;
        if plan.backend() != HostLifecycleBackendKind::SystemdTransientUnit {
            return Err(Error::InvalidInput(
                "SystemdTransientUnitBackend exact restart inspection requires a systemd_transient_unit request"
                    .to_owned(),
            ));
        }
        self.ensure_capable()?;
        let observed = self
            .client
            .inspect_unit(SystemdInspectUnitRequest::for_execution(
                plan.execution_id().clone(),
            )?)
            .await?;
        if observed.is_absent() {
            return Err(Error::NotFound(format!(
                "systemd unit {} is absent",
                plan.unit_name().as_str()
            )));
        }
        observed
            .activation_fence()
            .ok_or_else(|| {
                Error::PermissionDenied(format!(
                    "systemd restart target unit {} has no retained activation fence",
                    observed.unit_name().as_str()
                ))
            })?
            .authenticate_restart_target(claim)?;
        observed.to_host_lifecycle_status()
    }
}

impl<C> HostLifecycleBackend for SystemdTransientUnitBackend<C>
where
    C: SystemdDbusClient,
{
    fn validate(
        &self,
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecyclePlan> {
        self.ensure_capable()?;
        let plan = HostLifecyclePlan::from_binding(binding, request)?;
        if plan.backend() != HostLifecycleBackendKind::SystemdTransientUnit {
            return Err(Error::InvalidInput(format!(
                "SystemdTransientUnitBackend requires a systemd_transient_unit plan, got {:?}",
                plan.backend()
            )));
        }
        SystemdStartTransientUnitRequest::from_plan(&plan)?;
        Ok(plan)
    }

    fn stop<'a>(
        &'a self,
        execution_id: WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            self.ensure_capable()?;
            let response = self
                .client
                .stop_unit(SystemdStopUnitRequest::for_execution(execution_id)?)
                .await?;
            response.status().to_host_lifecycle_status()
        })
    }

    fn inspect<'a>(
        &'a self,
        execution_id: WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            self.ensure_capable()?;
            let status = self
                .client
                .inspect_unit(SystemdInspectUnitRequest::for_execution(execution_id)?)
                .await?;
            status.to_host_lifecycle_status()
        })
    }

    fn inspect_exact<'a>(
        &'a self,
        plan: HostLifecyclePlan,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            self.inspect_provider(&HostProviderPlan::from_lifecycle(&plan))
                .await
        })
    }

    fn activate_exact<'a>(
        &'a self,
        execution: WorkloadExecutionReference,
        claim: WorkloadProvisionDispatchClaim,
        request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            let plan = HostProviderPlan::from_execution(&execution, &claim, request)?;
            if plan.backend() != HostLifecycleBackendKind::SystemdTransientUnit {
                return Err(Error::InvalidInput(
                    "SystemdTransientUnitBackend exact activation requires a systemd_transient_unit request"
                        .to_owned(),
                ));
            }
            self.activate_provider_exact(&plan).await
        })
    }

    fn inspect_activation<'a>(
        &'a self,
        execution: WorkloadExecutionReference,
        claim: WorkloadProvisionDispatchClaim,
        request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            let plan = HostProviderPlan::from_execution(&execution, &claim, request)?;
            if plan.backend() != HostLifecycleBackendKind::SystemdTransientUnit {
                return Err(Error::InvalidInput(
                    "SystemdTransientUnitBackend exact inspection requires a systemd_transient_unit request"
                        .to_owned(),
                ));
            }
            self.inspect_provider(&plan).await
        })
    }

    fn quiesce_restart_exact<'a>(
        &'a self,
        claim: HostRestartProviderClaim,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            claim.require_execute_authority()?;
            let observed = self.inspect_restart_source(&claim).await?;
            if observed.active_state() == "inactive" {
                return observed.to_host_lifecycle_status();
            }
            let request = SystemdStopUnitRequest::for_execution(
                claim.source_execution().execution_id().clone(),
            )?;
            match self.client.stop_unit(request).await {
                Ok(response) => response.status().to_host_lifecycle_status(),
                Err(stop_error) => match self.inspect_restart_source(&claim).await {
                    Ok(observed) if observed.active_state() == "inactive" => {
                        observed.to_host_lifecycle_status()
                    }
                    _ => Err(stop_error),
                },
            }
        })
    }

    fn inspect_restart_quiescence<'a>(
        &'a self,
        claim: HostRestartProviderClaim,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            self.inspect_restart_source(&claim)
                .await?
                .to_host_lifecycle_status()
        })
    }

    fn activate_restart_exact<'a>(
        &'a self,
        claim: HostRestartProviderClaim,
        request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            claim.require_execute_authority()?;
            let plan = HostProviderPlan::from_restart(&claim, request)?;
            if plan.backend() != HostLifecycleBackendKind::SystemdTransientUnit {
                return Err(Error::InvalidInput(
                    "SystemdTransientUnitBackend exact restart activation requires a systemd_transient_unit request"
                        .to_owned(),
                ));
            }
            self.activate_provider_exact(&plan).await
        })
    }

    fn inspect_restart_activation<'a>(
        &'a self,
        claim: HostRestartProviderClaim,
        request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move { self.inspect_restart_target(&claim, request).await })
    }
}

fn adopt_exact_systemd_observation(
    expected_fence: &HostActivationFence,
    observed: &SystemdUnitStatus,
) -> Result<Option<HostLifecycleStatus>> {
    if observed.is_absent() {
        return Ok(None);
    }
    ensure_exact_systemd_fence(expected_fence, observed)?;
    observed.to_host_lifecycle_status().map(Some)
}

fn ensure_exact_systemd_fence(
    expected_fence: &HostActivationFence,
    observed: &SystemdUnitStatus,
) -> Result<()> {
    if observed.activation_fence() == Some(expected_fence) {
        return Ok(());
    }
    Err(Error::PermissionDenied(format!(
        "systemd unit {} is crossed with the retained activation fence",
        observed.unit_name().as_str()
    )))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdTransientCapabilities {
    dbus_available: bool,
    transient_units: bool,
    service_units: bool,
}

impl SystemdTransientCapabilities {
    pub fn available() -> Self {
        Self {
            dbus_available: true,
            transient_units: true,
            service_units: true,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            dbus_available: false,
            transient_units: false,
            service_units: false,
        }
    }

    pub fn without_dbus(mut self) -> Self {
        self.dbus_available = false;
        self
    }

    pub fn without_transient_units(mut self) -> Self {
        self.transient_units = false;
        self
    }

    pub fn without_service_units(mut self) -> Self {
        self.service_units = false;
        self
    }

    pub fn ensure_required(&self) -> Result<()> {
        if !self.dbus_available {
            return Err(Error::ResourceExhausted(
                "systemd D-Bus is unavailable for transient unit backend".to_string(),
            ));
        }
        if !self.transient_units {
            return Err(Error::ResourceExhausted(
                "systemd transient units are unavailable".to_string(),
            ));
        }
        if !self.service_units {
            return Err(Error::ResourceExhausted(
                "systemd service units are unavailable".to_string(),
            ));
        }
        Ok(())
    }

    pub fn dbus_available(&self) -> bool {
        self.dbus_available
    }

    pub fn transient_units(&self) -> bool {
        self.transient_units
    }

    pub fn service_units(&self) -> bool {
        self.service_units
    }

    pub fn to_backend_capabilities(&self) -> HostLifecycleBackendCapabilities {
        let mut capabilities = HostLifecycleBackendCapabilities::new(
            HostLifecycleBackendKind::SystemdTransientUnit,
            self.dbus_available && self.transient_units && self.service_units,
        )
        .with_feature("dbus", self.dbus_available)
        .with_feature("transient_units", self.transient_units)
        .with_feature("service_units", self.service_units);
        if !self.dbus_available {
            capabilities = capabilities
                .with_failure_reason("systemd D-Bus is unavailable for transient unit backend")
                .expect("static failure reason should be valid");
        }
        if !self.transient_units {
            capabilities = capabilities
                .with_failure_reason("systemd transient units are unavailable")
                .expect("static failure reason should be valid");
        }
        if !self.service_units {
            capabilities = capabilities
                .with_failure_reason("systemd service units are unavailable")
                .expect("static failure reason should be valid");
        }
        capabilities
    }
}

#[derive(Debug, Clone)]
pub struct UnavailableSystemdDbusClient {
    reason: String,
}

impl UnavailableSystemdDbusClient {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl SystemdDbusClient for UnavailableSystemdDbusClient {
    fn capabilities(&self) -> SystemdTransientCapabilities {
        SystemdTransientCapabilities::unavailable()
    }

    fn start_transient_unit<'a>(
        &'a self,
        _request: SystemdStartTransientUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse> {
        Box::pin(async move {
            Err(Error::ResourceExhausted(format!(
                "systemd D-Bus client unavailable: {}",
                self.reason
            )))
        })
    }

    fn stop_unit<'a>(
        &'a self,
        _request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse> {
        Box::pin(async move {
            Err(Error::ResourceExhausted(format!(
                "systemd D-Bus client unavailable: {}",
                self.reason
            )))
        })
    }

    fn inspect_unit<'a>(
        &'a self,
        _request: SystemdInspectUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdUnitStatus> {
        Box::pin(async move {
            Err(Error::ResourceExhausted(format!(
                "systemd D-Bus client unavailable: {}",
                self.reason
            )))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartTransientMode {
    Replace,
    Fail,
}

impl StartTransientMode {
    pub fn as_dbus_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdStartTransientUnitRequest {
    unit_name: SystemdUnitName,
    mode: StartTransientMode,
    properties: Vec<SystemdDbusProperty>,
    execution_id: WorkloadExecutionId,
    cgroup_path: String,
    journal_selectors: Vec<SystemdJournalSelector>,
    activation_fence: Option<HostActivationFence>,
}

impl SystemdStartTransientUnitRequest {
    pub fn from_plan(plan: &HostLifecyclePlan) -> Result<Self> {
        Self::from_provider_plan(&HostProviderPlan::from_lifecycle(plan))
    }

    fn from_provider_plan(plan: &HostProviderPlan) -> Result<Self> {
        if plan.backend() != HostLifecycleBackendKind::SystemdTransientUnit {
            return Err(Error::InvalidInput(format!(
                "cannot build StartTransientUnit request for {:?} backend",
                plan.backend()
            )));
        }
        let activation_fence = plan.activation_fence().cloned();
        let journal_fields = activation_fence.as_ref().map_or_else(
            || vec![execution_journal_field(plan.execution_id())],
            HostActivationFence::journal_fields,
        );
        let mut properties = vec![
            SystemdDbusProperty::Description(format!(
                "Nimbus tenant workload {}",
                plan.execution_id().as_str()
            )),
            SystemdDbusProperty::LogExtraFields(journal_fields.clone()),
            SystemdDbusProperty::ExecStart(SystemdExecStart::from_provider_plan(plan)?),
        ];
        properties.extend(
            plan.properties()
                .properties()
                .iter()
                .map(SystemdDbusProperty::from_host_property),
        );
        Ok(Self {
            unit_name: plan.unit_name().clone(),
            mode: StartTransientMode::Fail,
            properties,
            execution_id: plan.execution_id().clone(),
            cgroup_path: cgroup_path_for_unit(plan.unit_name()),
            journal_selectors: journal_selectors(plan.unit_name(), &journal_fields)?,
            activation_fence,
        })
    }

    /// Build a request directly from a workload id and executable, without a
    /// full `HostLifecyclePlan`. Used by NDB5's live integration tests to
    /// drive `StartTransientUnit` against a real session bus. Uses
    /// `StartTransientMode::Fail` so a stale unit surfaces instead of being
    /// silently replaced.
    #[cfg(feature = "systemd-dbus-integration-tests")]
    pub fn for_integration_execution(
        execution_id: WorkloadExecutionId,
        executable: impl Into<String>,
        args: Vec<String>,
    ) -> Result<Self> {
        let unit_name = systemd_unit_for_execution(&execution_id)?;
        let properties = vec![
            SystemdDbusProperty::Description(format!(
                "Nimbus NDB5 integration test {}",
                execution_id.as_str()
            )),
            SystemdDbusProperty::LogExtraFields(vec![execution_journal_field(&execution_id)]),
            SystemdDbusProperty::ExecStart(SystemdExecStart {
                executable: executable.into(),
                args,
                ignore_failure: false,
            }),
        ];
        Ok(Self {
            cgroup_path: cgroup_path_for_unit(&unit_name),
            journal_selectors: vec![
                SystemdJournalSelector::new("_SYSTEMD_UNIT", unit_name.as_str())?,
                SystemdJournalSelector::new(
                    WORKLOAD_EXECUTION_JOURNAL_FIELD,
                    execution_id.as_str(),
                )?,
            ],
            unit_name,
            mode: StartTransientMode::Fail,
            properties,
            execution_id,
            activation_fence: None,
        })
    }

    pub fn unit_name(&self) -> &SystemdUnitName {
        &self.unit_name
    }

    pub fn mode(&self) -> StartTransientMode {
        self.mode
    }

    pub fn properties(&self) -> &[SystemdDbusProperty] {
        &self.properties
    }

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
    }

    pub fn cgroup_path(&self) -> &str {
        &self.cgroup_path
    }

    pub fn journal_selectors(&self) -> &[SystemdJournalSelector] {
        &self.journal_selectors
    }

    pub(crate) fn activation_fence(&self) -> Option<&HostActivationFence> {
        self.activation_fence.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SystemdDbusProperty {
    Description(String),
    Slice(String),
    Restart(HostRestartPolicy),
    RestartSec(u64),
    MemoryMax(u64),
    CpuWeight(u64),
    TasksMax(u64),
    LogExtraFields(Vec<String>),
    ExecStart(SystemdExecStart),
}

impl SystemdDbusProperty {
    fn from_host_property(property: &HostLifecycleProperty) -> Self {
        match property {
            HostLifecycleProperty::Description(value) => Self::Description(value.clone()),
            HostLifecycleProperty::Slice(value) => Self::Slice(value.clone()),
            HostLifecycleProperty::Restart(value) => Self::Restart(*value),
            HostLifecycleProperty::RestartSec(value) => Self::RestartSec(*value),
            HostLifecycleProperty::MemoryMaxBytes(value) => Self::MemoryMax(*value),
            HostLifecycleProperty::CpuWeight(value) => Self::CpuWeight(*value),
            HostLifecycleProperty::TasksMax(value) => Self::TasksMax(*value),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Description(_) => "Description",
            Self::Slice(_) => "Slice",
            Self::Restart(_) => "Restart",
            Self::RestartSec(_) => "RestartSec",
            Self::MemoryMax(_) => "MemoryMax",
            Self::CpuWeight(_) => "CPUWeight",
            Self::TasksMax(_) => "TasksMax",
            Self::LogExtraFields(_) => "LogExtraFields",
            Self::ExecStart(_) => "ExecStart",
        }
    }
}

fn execution_journal_field(execution_id: &WorkloadExecutionId) -> String {
    format!(
        "{WORKLOAD_EXECUTION_JOURNAL_FIELD}={}",
        execution_id.as_str()
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdExecStart {
    executable: String,
    args: Vec<String>,
    ignore_failure: bool,
}

impl SystemdExecStart {
    fn from_provider_plan(plan: &HostProviderPlan) -> Result<Self> {
        Ok(Self {
            executable: plan.executable().as_str().to_string(),
            args: plan.args().to_vec(),
            ignore_failure: false,
        })
    }

    #[cfg(all(test, target_os = "linux", feature = "systemd-dbus-test-bus"))]
    pub(crate) fn for_test(executable: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            executable: executable.into(),
            args,
            ignore_failure: false,
        }
    }

    pub fn executable(&self) -> &str {
        &self.executable
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn ignore_failure(&self) -> bool {
        self.ignore_failure
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdJournalSelector {
    field: String,
    value: String,
}

impl SystemdJournalSelector {
    pub fn new(field: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        let field = field.into();
        let value = value.into();
        if field.is_empty()
            || !field
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
        {
            return Err(Error::InvalidInput(format!(
                "journal selector field `{field}` must be uppercase ASCII or underscore"
            )));
        }
        if value.is_empty() || value.contains('\n') || value.contains('\0') {
            return Err(Error::InvalidInput(format!(
                "journal selector `{field}` has an invalid value"
            )));
        }
        Ok(Self { field, value })
    }

    pub fn field(&self) -> &str {
        &self.field
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdStartTransientUnitResponse {
    unit_name: SystemdUnitName,
    job_path: String,
}

impl SystemdStartTransientUnitResponse {
    pub fn new(unit_name: SystemdUnitName, job_path: impl Into<String>) -> Result<Self> {
        Ok(Self {
            unit_name,
            job_path: valid_object_path(job_path, "systemd start job path")?,
        })
    }

    pub fn unit_name(&self) -> &SystemdUnitName {
        &self.unit_name
    }

    pub fn job_path(&self) -> &str {
        &self.job_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdStopUnitRequest {
    execution_id: WorkloadExecutionId,
    unit_name: SystemdUnitName,
    mode: StartTransientMode,
}

impl SystemdStopUnitRequest {
    pub fn for_execution(execution_id: WorkloadExecutionId) -> Result<Self> {
        let unit_name = systemd_unit_for_execution(&execution_id)?;
        Ok(Self {
            execution_id,
            // StopUnit replacement is a teardown-only systemd operation. Exact
            // provisioning always uses StartTransientMode::Fail.
            mode: StartTransientMode::Replace,
            unit_name,
        })
    }

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
    }

    pub fn unit_name(&self) -> &SystemdUnitName {
        &self.unit_name
    }

    pub fn mode(&self) -> StartTransientMode {
        self.mode
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdInspectUnitRequest {
    execution_id: WorkloadExecutionId,
    unit_name: SystemdUnitName,
}

impl SystemdInspectUnitRequest {
    pub fn for_execution(execution_id: WorkloadExecutionId) -> Result<Self> {
        let unit_name = systemd_unit_for_execution(&execution_id)?;
        Ok(Self {
            execution_id,
            unit_name,
        })
    }

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
    }

    pub fn unit_name(&self) -> &SystemdUnitName {
        &self.unit_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdStopUnitResponse {
    job_path: String,
    status: SystemdUnitStatus,
}

impl SystemdStopUnitResponse {
    pub fn new(job_path: impl Into<String>, status: SystemdUnitStatus) -> Result<Self> {
        Ok(Self {
            job_path: valid_object_path(job_path, "systemd stop job path")?,
            status,
        })
    }

    pub fn job_path(&self) -> &str {
        &self.job_path
    }

    pub fn status(&self) -> &SystemdUnitStatus {
        &self.status
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdUnitStatus {
    execution_id: WorkloadExecutionId,
    unit_name: SystemdUnitName,
    active_state: String,
    sub_state: String,
    job_path: Option<String>,
    main_pid: Option<u32>,
    cgroup_path: String,
    journal_selectors: Vec<SystemdJournalSelector>,
    activation_fence: Option<HostActivationFence>,
    #[serde(skip)]
    presence: SystemdUnitPresence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SystemdUnitPresence {
    Present,
    ExplicitlyAbsent,
}

impl SystemdUnitStatus {
    pub fn new(
        execution_id: WorkloadExecutionId,
        unit_name: SystemdUnitName,
        active_state: impl Into<String>,
        sub_state: impl Into<String>,
    ) -> Result<Self> {
        let cgroup_path = cgroup_path_for_unit(&unit_name);
        let journal_selectors = vec![
            SystemdJournalSelector::new("_SYSTEMD_UNIT", unit_name.as_str())?,
            SystemdJournalSelector::new(WORKLOAD_EXECUTION_JOURNAL_FIELD, execution_id.as_str())?,
        ];
        Ok(Self {
            execution_id,
            unit_name,
            active_state: active_state.into(),
            sub_state: sub_state.into(),
            job_path: None,
            main_pid: None,
            cgroup_path,
            journal_selectors,
            activation_fence: None,
            presence: SystemdUnitPresence::Present,
        })
    }

    #[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
    pub(crate) fn explicitly_absent(
        execution_id: WorkloadExecutionId,
        unit_name: SystemdUnitName,
    ) -> Result<Self> {
        let mut status = Self::new(execution_id, unit_name, "inactive", "dead")?;
        status.presence = SystemdUnitPresence::ExplicitlyAbsent;
        Ok(status)
    }

    pub fn with_job_path(mut self, job_path: impl Into<String>) -> Result<Self> {
        self.job_path = Some(valid_object_path(job_path, "systemd status job path")?);
        Ok(self)
    }

    pub fn with_main_pid(mut self, main_pid: u32) -> Self {
        self.main_pid = Some(main_pid);
        self
    }

    #[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
    pub(crate) fn with_activation_fence(mut self, fence: HostActivationFence) -> Self {
        self.activation_fence = Some(fence);
        self
    }

    pub fn to_host_lifecycle_status(&self) -> Result<HostLifecycleStatus> {
        let observed = match self.active_state.as_str() {
            "activating" => HostBackendObservedState::Submitted,
            "active" => {
                if self.sub_state == "running" {
                    HostBackendObservedState::Running
                } else {
                    HostBackendObservedState::Ready
                }
            }
            "inactive" => HostBackendObservedState::Stopped,
            "failed" => HostBackendObservedState::Failed(format!(
                "systemd unit {} is failed ({})",
                self.unit_name.as_str(),
                self.sub_state
            )),
            _ => HostBackendObservedState::Unknown,
        };
        status_from_observed(self, observed)
    }

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
    }

    pub fn unit_name(&self) -> &SystemdUnitName {
        &self.unit_name
    }

    pub fn active_state(&self) -> &str {
        &self.active_state
    }

    pub fn sub_state(&self) -> &str {
        &self.sub_state
    }

    pub fn job_path(&self) -> Option<&str> {
        self.job_path.as_deref()
    }

    pub fn main_pid(&self) -> Option<u32> {
        self.main_pid
    }

    pub fn cgroup_path(&self) -> &str {
        &self.cgroup_path
    }

    pub fn journal_selectors(&self) -> &[SystemdJournalSelector] {
        &self.journal_selectors
    }

    pub(crate) fn activation_fence(&self) -> Option<&HostActivationFence> {
        self.activation_fence.as_ref()
    }

    fn is_absent(&self) -> bool {
        self.presence == SystemdUnitPresence::ExplicitlyAbsent
    }
}

fn journal_selectors(
    unit_name: &SystemdUnitName,
    fields: &[String],
) -> Result<Vec<SystemdJournalSelector>> {
    let mut selectors = vec![SystemdJournalSelector::new(
        "_SYSTEMD_UNIT",
        unit_name.as_str(),
    )?];
    for field in fields {
        let (name, value) = field.split_once('=').ok_or_else(|| {
            Error::InvalidInput("systemd journal field must use NAME=value form".to_owned())
        })?;
        selectors.push(SystemdJournalSelector::new(name, value)?);
    }
    Ok(selectors)
}

fn status_from_observed(
    status: &SystemdUnitStatus,
    observed: HostBackendObservedState,
) -> Result<HostLifecycleStatus> {
    let (phase, reason, message) = match observed {
        HostBackendObservedState::Planned => (
            TenantWorkloadPhase::Pending,
            HostLifecycleStatusReason::Planned,
            None,
        ),
        HostBackendObservedState::Submitted => (
            TenantWorkloadPhase::Bound,
            HostLifecycleStatusReason::Submitted,
            None,
        ),
        HostBackendObservedState::Running => (
            TenantWorkloadPhase::Running,
            HostLifecycleStatusReason::Running,
            None,
        ),
        HostBackendObservedState::Ready => (
            TenantWorkloadPhase::Ready,
            HostLifecycleStatusReason::Ready,
            None,
        ),
        HostBackendObservedState::Stopped => (
            TenantWorkloadPhase::Deleting,
            HostLifecycleStatusReason::Stopped,
            Some("systemd transient unit is inactive".to_string()),
        ),
        HostBackendObservedState::Failed(message) => (
            TenantWorkloadPhase::Denied,
            HostLifecycleStatusReason::Failed,
            Some(message),
        ),
        HostBackendObservedState::Unknown => (
            TenantWorkloadPhase::Degraded,
            HostLifecycleStatusReason::Unknown,
            Some("systemd transient unit state is unknown".to_string()),
        ),
    };
    let mut lifecycle_evidence = TenantWorkloadLifecycleEvidence::for_observed_unit(
        HostLifecycleBackendKind::SystemdTransientUnit,
        &status.unit_name,
        reason,
    )
    .with_cgroup_path(&status.cgroup_path)?
    .with_journal_selectors(journal_selector_evidence(&status.journal_selectors)?)
    .with_message(message.clone());
    if let Some(job_path) = status.job_path.as_deref() {
        lifecycle_evidence = lifecycle_evidence.with_job_path(job_path)?;
    }
    if let Some(main_pid) = status.main_pid {
        lifecycle_evidence = lifecycle_evidence.with_process_id(u64::from(main_pid));
    }
    Ok(HostLifecycleStatus::new_for_backend(
        status.execution_id.clone(),
        status.unit_name.clone(),
        phase,
        reason,
        message,
        lifecycle_evidence,
    ))
}

fn journal_selector_evidence(
    selectors: &[SystemdJournalSelector],
) -> Result<Vec<HostLifecycleJournalSelectorEvidence>> {
    selectors
        .iter()
        .map(|selector| {
            HostLifecycleJournalSelectorEvidence::new(selector.field(), selector.value())
        })
        .collect()
}

fn systemd_unit_for_execution(execution_id: &WorkloadExecutionId) -> Result<SystemdUnitName> {
    SystemdUnitName::for_execution(execution_id, SystemdUnitKind::Service)
}

fn cgroup_path_for_unit(unit_name: &SystemdUnitName) -> String {
    format!("/system.slice/{}", unit_name.as_str())
}

fn valid_object_path(value: impl Into<String>, field: &str) -> Result<String> {
    let value = value.into();
    if !value.starts_with('/') || value.contains('\0') || value.contains(char::is_whitespace) {
        return Err(Error::InvalidInput(format!(
            "{field} `{value}` must be an absolute D-Bus object path"
        )));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "systemd_transient/tests.rs"]
mod tests;

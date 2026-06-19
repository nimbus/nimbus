use std::sync::Arc;

use nimbus_core::{Error, Result};
use serde::Serialize;

use super::{
    HostBackendObservedState, HostLifecycleBackend, HostLifecycleBackendCapabilities,
    HostLifecycleBackendKind, HostLifecycleFuture, HostLifecycleJournalSelectorEvidence,
    HostLifecyclePlan, HostLifecycleProperty, HostLifecycleRequest, HostLifecycleStatus,
    HostLifecycleStatusReason, HostRestartPolicy, LocalEnforcementBinding, SystemdUnitKind,
    SystemdUnitName, TenantWorkloadId, TenantWorkloadLifecycleEvidence, TenantWorkloadPhase,
    TenantWorkloadStatus,
};

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

    fn start<'a>(
        &'a self,
        plan: HostLifecyclePlan,
    ) -> HostLifecycleFuture<'a, TenantWorkloadStatus> {
        Box::pin(async move {
            self.ensure_capable()?;
            let request = SystemdStartTransientUnitRequest::from_plan(&plan)?;
            let response = self.client.start_transient_unit(request).await?;
            if response.unit_name() != plan.unit_name() {
                return Err(Error::InvalidInput(format!(
                    "systemd StartTransientUnit returned unit {}, but plan requested {}",
                    response.unit_name().as_str(),
                    plan.unit_name().as_str()
                )));
            }
            let request = SystemdStartTransientUnitRequest::from_plan(&plan)?;
            let lifecycle_evidence = TenantWorkloadLifecycleEvidence::from_plan(
                &plan,
                HostLifecycleStatusReason::Submitted,
            )
            .with_job_path(response.job_path())?
            .with_cgroup_path(request.cgroup_path())?
            .with_journal_selectors(journal_selector_evidence(request.journal_selectors())?);
            let status =
                HostLifecycleStatus::from_backend_state(&plan, HostBackendObservedState::Submitted)
                    .with_lifecycle_evidence(lifecycle_evidence);
            let workload_status = status.to_workload_status(&plan)?;
            Ok(workload_status)
        })
    }

    fn stop<'a>(
        &'a self,
        workload_id: TenantWorkloadId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            self.ensure_capable()?;
            let response = self
                .client
                .stop_unit(SystemdStopUnitRequest::for_workload(workload_id)?)
                .await?;
            response.status().to_host_lifecycle_status()
        })
    }

    fn inspect<'a>(
        &'a self,
        workload_id: TenantWorkloadId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async move {
            self.ensure_capable()?;
            let status = self
                .client
                .inspect_unit(SystemdInspectUnitRequest::for_workload(workload_id)?)
                .await?;
            status.to_host_lifecycle_status()
        })
    }
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
    workload_id: TenantWorkloadId,
    cgroup_path: String,
    journal_selectors: Vec<SystemdJournalSelector>,
}

impl SystemdStartTransientUnitRequest {
    pub fn from_plan(plan: &HostLifecyclePlan) -> Result<Self> {
        if plan.backend() != HostLifecycleBackendKind::SystemdTransientUnit {
            return Err(Error::InvalidInput(format!(
                "cannot build StartTransientUnit request for {:?} backend",
                plan.backend()
            )));
        }
        let mut properties = vec![
            SystemdDbusProperty::Description(format!(
                "Nimbus tenant workload {}",
                plan.workload_id().as_str()
            )),
            SystemdDbusProperty::ExecStart(SystemdExecStart::from_plan(plan)?),
        ];
        properties.extend(
            plan.properties()
                .properties()
                .iter()
                .map(SystemdDbusProperty::from_host_property),
        );
        Ok(Self {
            unit_name: plan.unit_name().clone(),
            mode: StartTransientMode::Replace,
            properties,
            workload_id: plan.workload_id().clone(),
            cgroup_path: cgroup_path_for_unit(plan.unit_name()),
            journal_selectors: vec![
                SystemdJournalSelector::new("_SYSTEMD_UNIT", plan.unit_name().as_str())?,
                SystemdJournalSelector::new("NIMBUS_WORKLOAD_ID", plan.workload_id().as_str())?,
            ],
        })
    }

    /// Build a request directly from a workload id and executable, without a
    /// full `HostLifecyclePlan`. Used by NDB5's live integration tests to
    /// drive `StartTransientUnit` against a real session bus. Uses
    /// `StartTransientMode::Fail` so a stale unit surfaces instead of being
    /// silently replaced.
    #[cfg(feature = "systemd-dbus-integration-tests")]
    pub fn for_integration_test(
        workload_id: TenantWorkloadId,
        executable: impl Into<String>,
        args: Vec<String>,
    ) -> Result<Self> {
        let unit_name = systemd_unit_for_workload(&workload_id)?;
        let properties = vec![
            SystemdDbusProperty::Description(format!(
                "Nimbus NDB5 integration test {}",
                workload_id.as_str()
            )),
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
                SystemdJournalSelector::new("NIMBUS_WORKLOAD_ID", workload_id.as_str())?,
            ],
            unit_name,
            mode: StartTransientMode::Fail,
            properties,
            workload_id,
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

    pub fn workload_id(&self) -> &TenantWorkloadId {
        &self.workload_id
    }

    pub fn cgroup_path(&self) -> &str {
        &self.cgroup_path
    }

    pub fn journal_selectors(&self) -> &[SystemdJournalSelector] {
        &self.journal_selectors
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
            Self::ExecStart(_) => "ExecStart",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemdExecStart {
    executable: String,
    args: Vec<String>,
    ignore_failure: bool,
}

impl SystemdExecStart {
    fn from_plan(plan: &HostLifecyclePlan) -> Result<Self> {
        Ok(Self {
            executable: plan.executable().as_str().to_string(),
            args: plan.args().to_vec(),
            ignore_failure: false,
        })
    }

    #[cfg(all(test, feature = "systemd-dbus-test-bus"))]
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
    workload_id: TenantWorkloadId,
    unit_name: SystemdUnitName,
    mode: StartTransientMode,
}

impl SystemdStopUnitRequest {
    pub fn for_workload(workload_id: TenantWorkloadId) -> Result<Self> {
        let unit_name = systemd_unit_for_workload(&workload_id)?;
        Ok(Self {
            workload_id,
            mode: StartTransientMode::Replace,
            unit_name,
        })
    }

    pub fn workload_id(&self) -> &TenantWorkloadId {
        &self.workload_id
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
    workload_id: TenantWorkloadId,
    unit_name: SystemdUnitName,
}

impl SystemdInspectUnitRequest {
    pub fn for_workload(workload_id: TenantWorkloadId) -> Result<Self> {
        let unit_name = systemd_unit_for_workload(&workload_id)?;
        Ok(Self {
            workload_id,
            unit_name,
        })
    }

    pub fn workload_id(&self) -> &TenantWorkloadId {
        &self.workload_id
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
    workload_id: TenantWorkloadId,
    unit_name: SystemdUnitName,
    active_state: String,
    sub_state: String,
    job_path: Option<String>,
    main_pid: Option<u32>,
    cgroup_path: String,
    journal_selectors: Vec<SystemdJournalSelector>,
}

impl SystemdUnitStatus {
    pub fn new(
        workload_id: TenantWorkloadId,
        unit_name: SystemdUnitName,
        active_state: impl Into<String>,
        sub_state: impl Into<String>,
    ) -> Result<Self> {
        let cgroup_path = cgroup_path_for_unit(&unit_name);
        let journal_selectors = vec![
            SystemdJournalSelector::new("_SYSTEMD_UNIT", unit_name.as_str())?,
            SystemdJournalSelector::new("NIMBUS_WORKLOAD_ID", workload_id.as_str())?,
        ];
        Ok(Self {
            workload_id,
            unit_name,
            active_state: active_state.into(),
            sub_state: sub_state.into(),
            job_path: None,
            main_pid: None,
            cgroup_path,
            journal_selectors,
        })
    }

    pub fn with_job_path(mut self, job_path: impl Into<String>) -> Result<Self> {
        self.job_path = Some(valid_object_path(job_path, "systemd status job path")?);
        Ok(self)
    }

    pub fn with_main_pid(mut self, main_pid: u32) -> Self {
        self.main_pid = Some(main_pid);
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

    pub fn workload_id(&self) -> &TenantWorkloadId {
        &self.workload_id
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
        status.workload_id.clone(),
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

fn systemd_unit_for_workload(workload_id: &TenantWorkloadId) -> Result<SystemdUnitName> {
    SystemdUnitName::for_workload(workload_id, SystemdUnitKind::Service)
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
mod tests {
    use std::sync::{Arc, Mutex};

    use nimbus_core::{PrincipalContext, TenantId};
    use nimbus_runtime::{RuntimeLimits, RuntimePolicy};

    use super::*;
    use crate::{HostExecutable, HostLifecyclePropertySet, HostRestartPolicy, TenantWorkloadPhase};
    use nimbus_tenant::{
        RuntimeIsolationTier, TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
        TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision,
        WorkloadAttributes, WorkloadLocation,
    };

    #[derive(Clone)]
    struct FakeSystemdDbusClient {
        capabilities: SystemdTransientCapabilities,
        last_start: Arc<Mutex<Option<SystemdStartTransientUnitRequest>>>,
        last_stop: Arc<Mutex<Option<SystemdStopUnitRequest>>>,
        status: Arc<Mutex<Option<SystemdUnitStatus>>>,
    }

    impl FakeSystemdDbusClient {
        fn available() -> Self {
            Self {
                capabilities: SystemdTransientCapabilities::available(),
                last_start: Arc::new(Mutex::new(None)),
                last_stop: Arc::new(Mutex::new(None)),
                status: Arc::new(Mutex::new(None)),
            }
        }

        fn with_capabilities(capabilities: SystemdTransientCapabilities) -> Self {
            Self {
                capabilities,
                last_start: Arc::new(Mutex::new(None)),
                last_stop: Arc::new(Mutex::new(None)),
                status: Arc::new(Mutex::new(None)),
            }
        }

        fn last_start(&self) -> SystemdStartTransientUnitRequest {
            self.last_start
                .lock()
                .expect("fake client lock should not be poisoned")
                .clone()
                .expect("start should have been called")
        }
    }

    impl SystemdDbusClient for FakeSystemdDbusClient {
        fn capabilities(&self) -> SystemdTransientCapabilities {
            self.capabilities.clone()
        }

        fn start_transient_unit<'a>(
            &'a self,
            request: SystemdStartTransientUnitRequest,
        ) -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse> {
            Box::pin(async move {
                let response = SystemdStartTransientUnitResponse::new(
                    request.unit_name().clone(),
                    "/org/freedesktop/systemd1/job/42",
                )?;
                let status = SystemdUnitStatus::new(
                    request.workload_id().clone(),
                    request.unit_name().clone(),
                    "activating",
                    "start",
                )?
                .with_job_path(response.job_path())?;
                *self
                    .last_start
                    .lock()
                    .expect("fake client lock should not be poisoned") = Some(request);
                *self
                    .status
                    .lock()
                    .expect("fake client lock should not be poisoned") = Some(status);
                Ok(response)
            })
        }

        fn stop_unit<'a>(
            &'a self,
            request: SystemdStopUnitRequest,
        ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse> {
            Box::pin(async move {
                *self
                    .last_stop
                    .lock()
                    .expect("fake client lock should not be poisoned") = Some(request.clone());
                let status = SystemdUnitStatus::new(
                    request.workload_id().clone(),
                    request.unit_name().clone(),
                    "inactive",
                    "dead",
                )?;
                *self
                    .status
                    .lock()
                    .expect("fake client lock should not be poisoned") = Some(status.clone());
                SystemdStopUnitResponse::new("/org/freedesktop/systemd1/job/43", status)
            })
        }

        fn inspect_unit<'a>(
            &'a self,
            request: SystemdInspectUnitRequest,
        ) -> HostLifecycleFuture<'a, SystemdUnitStatus> {
            Box::pin(async move {
                let status = self
                    .status
                    .lock()
                    .expect("fake client lock should not be poisoned")
                    .clone()
                    .unwrap_or_else(|| {
                        SystemdUnitStatus::new(
                            request.workload_id().clone(),
                            request.unit_name().clone(),
                            "active",
                            "running",
                        )
                        .expect("status should build")
                        .with_main_pid(1001)
                    });
                Ok(status)
            })
        }
    }

    fn admitted_decision() -> TenantIsolationDecision {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext {
                authenticated: true,
                claims: serde_json::Map::from_iter([(
                    "tenant_id".to_string(),
                    serde_json::Value::String("tenant-a".to_string()),
                )]),
                verified_claims: serde_json::Map::new(),
            },
            "systemd.transient",
        )
        .with_deployment_generation(12)
        .with_workload_location(WorkloadLocation::new().with_node_id("node-a"));
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let workload = WorkloadAttributes::runtime_function(
            "service:run",
            RuntimeIsolationTier::InProcessUntrusted,
        )
        .with_invocation_id("invoke-systemd");
        let input = TenantIsolationPolicyInput::new(workload)
            .with_runtime_policy(
                &context,
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
            )
            .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
            .with_storage(TenantStoragePolicyDecision::namespace("tenant-a"));
        context
            .admit_decision(input)
            .expect("decision should admit")
    }

    fn binding() -> LocalEnforcementBinding {
        LocalEnforcementBinding::from_decision(&admitted_decision())
            .expect("binding should materialize")
    }

    fn request() -> HostLifecycleRequest {
        HostLifecycleRequest::new(
            HostLifecycleBackendKind::SystemdTransientUnit,
            HostExecutable::trusted("/usr/libexec/nimbus/conmon-crun-launcher")
                .expect("trusted executable should parse"),
        )
        .with_args(["--bundle", "/run/nimbus/bundles/workload"])
        .expect("args should parse")
        .with_properties(
            HostLifecyclePropertySet::from_raw_systemd_properties([
                ("Description", "Nimbus workload"),
                ("Restart", "on-failure"),
                ("RestartSec", "2"),
                ("MemoryMax", "536870912"),
                ("CPUWeight", "100"),
                ("TasksMax", "128"),
            ])
            .expect("properties should parse"),
        )
    }

    #[test]
    fn start_transient_unit_request_uses_trusted_exec_and_allowlisted_properties() {
        let binding = binding();
        let plan = HostLifecyclePlan::from_binding(&binding, request()).expect("plan should build");
        let request =
            SystemdStartTransientUnitRequest::from_plan(&plan).expect("request should build");

        assert_eq!(request.unit_name(), plan.unit_name());
        assert_eq!(request.mode().as_dbus_str(), "replace");
        assert_eq!(request.workload_id(), plan.workload_id());
        assert!(
            request.cgroup_path().contains(plan.unit_name().as_str()),
            "cgroup path should correlate to unit"
        );
        assert!(request.journal_selectors().iter().any(|selector| {
            selector.field() == "_SYSTEMD_UNIT" && selector.value() == plan.unit_name().as_str()
        }));
        assert!(request.journal_selectors().iter().any(|selector| {
            selector.field() == "NIMBUS_WORKLOAD_ID"
                && selector.value() == plan.workload_id().as_str()
        }));

        let exec = request
            .properties()
            .iter()
            .find_map(|property| match property {
                SystemdDbusProperty::ExecStart(exec) => Some(exec),
                _ => None,
            })
            .expect("ExecStart property should be generated by Nimbus");
        assert_eq!(
            exec.executable(),
            "/usr/libexec/nimbus/conmon-crun-launcher"
        );
        assert_eq!(
            exec.args(),
            &[
                "--bundle".to_string(),
                "/run/nimbus/bundles/workload".to_string()
            ]
        );
        assert!(!exec.ignore_failure());
        assert!(request.properties().iter().any(|property| {
            matches!(
                property,
                SystemdDbusProperty::Restart(HostRestartPolicy::OnFailure)
            )
        }));
        assert!(
            request
                .properties()
                .iter()
                .any(|property| { matches!(property, SystemdDbusProperty::MemoryMax(536870912)) })
        );
    }

    #[test]
    fn systemd_backend_rejects_disallowed_properties_and_wrong_backend_plan() {
        let binding = binding();
        let backend = SystemdTransientUnitBackend::new(FakeSystemdDbusClient::available());
        let denied = HostLifecyclePropertySet::from_raw_systemd_properties([(
            "ExecStart",
            "/bin/sh -c escape",
        )])
        .expect_err("raw ExecStart should fail before backend validation");
        assert!(denied.to_string().contains("not allowlisted"));

        let wrong_request = HostLifecycleRequest::new(
            HostLifecycleBackendKind::DirectProcess,
            HostExecutable::trusted("/usr/libexec/nimbus/conmon-crun-launcher")
                .expect("trusted executable should parse"),
        );
        let error = backend
            .validate(&binding, wrong_request)
            .expect_err("systemd backend should reject direct process plan");
        assert!(
            error
                .to_string()
                .contains("requires a systemd_transient_unit plan"),
            "error should name backend mismatch: {error}"
        );
    }

    #[tokio::test]
    async fn backend_calls_start_transient_unit_and_maps_stop_inspect_status() {
        let client = FakeSystemdDbusClient::available();
        let backend = SystemdTransientUnitBackend::new(client.clone());
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("systemd plan should validate");
        let workload_id = plan.workload_id().clone();

        let started = backend
            .start(plan)
            .await
            .expect("systemd start should submit transient unit");
        assert_eq!(started.phase(), TenantWorkloadPhase::Bound);
        let start = client.last_start();
        assert_eq!(
            start.unit_name().as_str(),
            started.evidence_correlation_ids()[0]
        );

        let inspected = backend
            .inspect(workload_id.clone())
            .await
            .expect("inspect should map fake D-Bus status");
        assert_eq!(
            inspected.reason(),
            super::super::HostLifecycleStatusReason::Submitted
        );

        let stopped = backend
            .stop(workload_id)
            .await
            .expect("stop should map fake D-Bus status");
        assert_eq!(
            stopped.reason(),
            super::super::HostLifecycleStatusReason::Stopped
        );
    }

    #[test]
    fn systemd_backend_fails_closed_when_dbus_or_features_are_unavailable() {
        let binding = binding();
        for (capabilities, expected) in [
            (
                SystemdTransientCapabilities::available().without_dbus(),
                "D-Bus is unavailable",
            ),
            (
                SystemdTransientCapabilities::available().without_transient_units(),
                "transient units are unavailable",
            ),
            (
                SystemdTransientCapabilities::available().without_service_units(),
                "service units are unavailable",
            ),
        ] {
            let backend = SystemdTransientUnitBackend::new(
                FakeSystemdDbusClient::with_capabilities(capabilities),
            );
            let error = backend
                .validate(&binding, request())
                .expect_err("unavailable systemd feature should fail closed");
            assert!(
                error.to_string().contains(expected),
                "expected `{expected}` in error, got {error}"
            );
        }

        let backend = SystemdTransientUnitBackend::unavailable("not linux");
        let error = backend
            .validate(&binding, request())
            .expect_err("unavailable default client should fail closed");
        assert!(error.to_string().contains("D-Bus is unavailable"));
    }
}

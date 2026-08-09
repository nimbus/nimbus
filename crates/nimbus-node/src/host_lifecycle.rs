use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use nimbus_core::{Error, Result, non_empty};
#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
use nimbus_workloads::{
    NodeIdentity, TenantWorkloadUid, WorkloadDesiredDigest, WorkloadProvisionAttemptId,
    WorkloadProvisionSourceDigest,
};
use nimbus_workloads::{
    WorkloadExecutionAttemptId, WorkloadExecutionReference, WorkloadProvisionDispatchClaim,
    WorkloadProvisionProviderTarget, WorkloadProvisionStep, WorkloadProvisionSubjects,
};
use serde::Serialize;

#[path = "host_lifecycle/activation_fence.rs"]
mod activation_fence;
pub(crate) use activation_fence::HostActivationFence;
#[path = "host_lifecycle/restart.rs"]
mod restart;
pub use restart::{HostRestartProviderClaim, HostRestartProviderClaimInput};

use super::{
    LocalEnforcementBinding, NodeStatusAuthorizer, TenantWorkloadCondition,
    TenantWorkloadConditionStatus, TenantWorkloadConditionType, TenantWorkloadPhase,
    TenantWorkloadSpec, TenantWorkloadStatus, TenantWorkloadStatusPatch, WorkloadExecutionId,
};

pub type HostLifecycleFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait HostLifecycleBackend: Send + Sync + 'static {
    fn validate(
        &self,
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<HostLifecyclePlan>;

    fn stop<'a>(
        &'a self,
        execution_id: WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus>;

    fn inspect<'a>(
        &'a self,
        execution_id: WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus>;

    /// Inspect one exact provider plan without granting activation authority.
    ///
    /// Backends with retained activation fences override this method to reject
    /// crossed attempts. The default preserves existing unfenced backends.
    fn inspect_exact<'a>(
        &'a self,
        plan: HostLifecyclePlan,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        self.inspect(plan.execution_id().clone())
    }

    /// Execute one exact provider activation without reconstructing tenant policy.
    fn activate_exact<'a>(
        &'a self,
        _execution: WorkloadExecutionReference,
        _claim: WorkloadProvisionDispatchClaim,
        _request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "host lifecycle backend does not implement exact activation".to_owned(),
            ))
        })
    }

    /// Inspect one exact provider activation without granting effect authority.
    fn inspect_activation<'a>(
        &'a self,
        _execution: WorkloadExecutionReference,
        _claim: WorkloadProvisionDispatchClaim,
        _request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "host lifecycle backend does not implement exact activation inspection".to_owned(),
            ))
        })
    }

    /// Stop one exact source attempt under a compute-confirmed restart claim.
    fn quiesce_restart_exact<'a>(
        &'a self,
        _claim: HostRestartProviderClaim,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "host lifecycle backend does not implement exact restart quiescence".to_owned(),
            ))
        })
    }

    /// Inspect source-attempt quiescence without granting a stop effect.
    fn inspect_restart_quiescence<'a>(
        &'a self,
        _claim: HostRestartProviderClaim,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "host lifecycle backend does not implement restart quiescence inspection"
                    .to_owned(),
            ))
        })
    }

    /// Activate one exact target attempt under a compute-confirmed restart claim.
    fn activate_restart_exact<'a>(
        &'a self,
        _claim: HostRestartProviderClaim,
        _request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "host lifecycle backend does not implement exact restart activation".to_owned(),
            ))
        })
    }

    /// Inspect target-attempt activation without granting an activation effect.
    fn inspect_restart_activation<'a>(
        &'a self,
        _claim: HostRestartProviderClaim,
        _request: HostLifecycleRequest,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "host lifecycle backend does not implement restart activation inspection"
                    .to_owned(),
            ))
        })
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
    pub fn for_execution(
        execution_id: &WorkloadExecutionId,
        kind: SystemdUnitKind,
    ) -> Result<Self> {
        Self::new(format!(
            "nimbus-{}.{}",
            execution_id.as_str(),
            kind.extension()
        ))
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
            HostLifecycleProperty::Restart(HostRestartPolicy::No),
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
        Self::from_provider_plan(&HostProviderPlan::from_lifecycle(plan), reason)
    }

    pub(crate) fn from_provider_plan(
        plan: &HostProviderPlan,
        reason: HostLifecycleStatusReason,
    ) -> Self {
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

    pub(crate) fn ensure_external_restart_disabled(&self) -> Result<()> {
        let restart_properties = self
            .properties
            .properties()
            .iter()
            .filter_map(|property| match property {
                HostLifecycleProperty::Restart(policy) => Some(*policy),
                _ => None,
            })
            .collect::<Vec<_>>();
        let has_single_restart_property = restart_properties.len() <= 1;
        if !has_single_restart_property {
            return Err(Error::InvalidInput(
                "node workload request must not contain duplicate Restart properties".to_owned(),
            ));
        }
        if restart_properties
            .first()
            .is_some_and(|policy| *policy != HostRestartPolicy::No)
        {
            return Err(Error::PermissionDenied(
                "node workload provider restart must be disabled; compute owns restart decisions"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLifecyclePlan {
    execution_id: WorkloadExecutionId,
    spec: TenantWorkloadSpec,
    backend: HostLifecycleBackendKind,
    unit_name: SystemdUnitName,
    executable: HostExecutable,
    args: Vec<String>,
    properties: HostLifecyclePropertySet,
    trust_class: RuntimePoolTrustClass,
    activation_fence: Option<HostActivationFence>,
}

impl HostLifecyclePlan {
    pub fn from_binding(
        binding: &LocalEnforcementBinding,
        request: HostLifecycleRequest,
    ) -> Result<Self> {
        let spec = binding.spec().clone();
        let execution_id = spec.execution_id()?;
        let unit_name = SystemdUnitName::for_execution(&execution_id, SystemdUnitKind::Service)?;
        Ok(Self {
            execution_id,
            spec,
            backend: request.backend,
            unit_name,
            executable: request.executable,
            args: request.args,
            properties: request.properties,
            trust_class: request.trust_class,
            activation_fence: None,
        })
    }

    /// Bind this provider plan to one durable compute-authorized activation claim.
    ///
    /// The node remains an effect sink. It retains the complete provider fence
    /// for idempotent replay and rejects crossed execution authority before a
    /// process or unit can be created.
    pub fn with_activation_claim(mut self, claim: &WorkloadProvisionDispatchClaim) -> Result<Self> {
        self.activation_fence = Some(HostActivationFence::from_claim(&self, claim)?);
        Ok(self)
    }

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
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

    #[cfg(test)]
    pub(crate) fn activation_fence(&self) -> Option<&HostActivationFence> {
        self.activation_fence.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn with_test_activation_fence(mut self, seed: u8, dispatch_epoch: u64) -> Self {
        self.activation_fence = Some(HostActivationFence::for_test(&self, seed, dispatch_epoch));
        self
    }
}

/// Effect-local plan for one host activation provider.
///
/// This plan retains only authenticated execution and provider inputs. It does
/// not carry tenant admission state and cannot authorize tenant status writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostProviderPlan {
    execution_id: WorkloadExecutionId,
    backend: HostLifecycleBackendKind,
    unit_name: SystemdUnitName,
    executable: HostExecutable,
    args: Vec<String>,
    properties: HostLifecyclePropertySet,
    trust_class: RuntimePoolTrustClass,
    activation_fence: Option<HostActivationFence>,
}

impl HostProviderPlan {
    pub(crate) fn from_lifecycle(plan: &HostLifecyclePlan) -> Self {
        Self {
            execution_id: plan.execution_id.clone(),
            backend: plan.backend,
            unit_name: plan.unit_name.clone(),
            executable: plan.executable.clone(),
            args: plan.args.clone(),
            properties: plan.properties.clone(),
            trust_class: plan.trust_class,
            activation_fence: plan.activation_fence.clone(),
        }
    }

    pub(crate) fn from_execution(
        execution: &WorkloadExecutionReference,
        claim: &WorkloadProvisionDispatchClaim,
        request: HostLifecycleRequest,
    ) -> Result<Self> {
        request.ensure_external_restart_disabled()?;
        let activation_fence = HostActivationFence::from_execution(execution, claim)?;
        let unit_name =
            SystemdUnitName::for_execution(execution.execution_id(), SystemdUnitKind::Service)?;
        Ok(Self {
            execution_id: execution.execution_id().clone(),
            backend: request.backend,
            unit_name,
            executable: request.executable,
            args: request.args,
            properties: request.properties,
            trust_class: request.trust_class,
            activation_fence: Some(activation_fence),
        })
    }

    pub(crate) fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
    }

    pub(crate) const fn backend(&self) -> HostLifecycleBackendKind {
        self.backend
    }

    pub(crate) fn unit_name(&self) -> &SystemdUnitName {
        &self.unit_name
    }

    pub(crate) fn executable(&self) -> &HostExecutable {
        &self.executable
    }

    pub(crate) fn args(&self) -> &[String] {
        &self.args
    }

    pub(crate) fn properties(&self) -> &HostLifecyclePropertySet {
        &self.properties
    }

    pub(crate) fn activation_fence(&self) -> Option<&HostActivationFence> {
        self.activation_fence.as_ref()
    }

    /// Compare the complete provider-relevant lifecycle projection.
    ///
    /// A lifecycle observer does not possess the compute dispatch claim, so it
    /// cannot reproduce `activation_fence`. Every other provider input must
    /// remain exact before the retained effect can be observed.
    pub(crate) fn matches_lifecycle_projection(&self, plan: &HostLifecyclePlan) -> bool {
        self.execution_id == plan.execution_id
            && self.backend == plan.backend
            && self.unit_name == plan.unit_name
            && self.executable == plan.executable
            && self.args == plan.args
            && self.properties == plan.properties
            && self.trust_class == plan.trust_class
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct HostProvisionActivationFence {
    workload_uid: String,
    node_identity: String,
    execution_id: WorkloadExecutionId,
    execution_attempt_id: WorkloadExecutionAttemptId,
    execution_provider_id: String,
    attempt_id: String,
    dispatch_epoch: u64,
    claimed_revision: u64,
    generation: u64,
    desired_digest: String,
    source_digest: String,
    network_plan_digest: String,
}

impl HostProvisionActivationFence {
    #[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
    const JOURNAL_FIELD_NAMES: [&'static str; 12] = [
        "NIMBUS_WORKLOAD_UID",
        "NIMBUS_NODE_IDENTITY",
        "NIMBUS_WORKLOAD_EXECUTION_ID",
        "NIMBUS_WORKLOAD_EXECUTION_ATTEMPT_ID",
        "NIMBUS_WORKLOAD_EXECUTION_PROVIDER_ID",
        "NIMBUS_PROVISION_ATTEMPT_ID",
        "NIMBUS_PROVISION_DISPATCH_EPOCH",
        "NIMBUS_PROVISION_CLAIMED_REVISION",
        "NIMBUS_WORKLOAD_GENERATION",
        "NIMBUS_WORKLOAD_DESIRED_DIGEST",
        "NIMBUS_WORKLOAD_SOURCE_DIGEST",
        "NIMBUS_NETWORK_PLAN_DIGEST",
    ];

    fn from_claim(
        plan: &HostLifecyclePlan,
        claim: &WorkloadProvisionDispatchClaim,
    ) -> Result<Self> {
        let attempt = claim.attempt();
        let WorkloadProvisionSubjects::Execution(execution) = attempt.subjects() else {
            return Err(Error::PermissionDenied(
                "host activation claim must name one exact execution".to_owned(),
            ));
        };
        let fence = Self::from_execution(execution, claim)?;
        let spec = plan.spec();
        if execution.execution_id() != plan.execution_id()
            || execution.workload_uid() != spec.workload_uid()
            || execution.node_identity()
                != spec.assigned_node_id().ok_or_else(|| {
                    Error::PermissionDenied(
                        "host activation requires an admitted node assignment".to_owned(),
                    )
                })?
            || execution.generation() != spec.generation()
        {
            return Err(Error::PermissionDenied(
                "host activation claim is crossed with the admitted execution".to_owned(),
            ));
        }
        Ok(fence)
    }

    fn from_execution(
        execution: &WorkloadExecutionReference,
        claim: &WorkloadProvisionDispatchClaim,
    ) -> Result<Self> {
        let attempt = claim.attempt();
        if attempt.step() != WorkloadProvisionStep::ActivateWorkload {
            return Err(Error::PermissionDenied(
                "host activation requires an activate_workload dispatch claim".to_owned(),
            ));
        }
        let WorkloadProvisionProviderTarget::Execution { provider_id, .. } =
            claim.provider_target()
        else {
            return Err(Error::PermissionDenied(
                "host activation requires execution-provider authority".to_owned(),
            ));
        };
        let WorkloadProvisionSubjects::Execution(subject) = attempt.subjects() else {
            return Err(Error::PermissionDenied(
                "host activation claim must name one exact execution".to_owned(),
            ));
        };
        if subject != execution
            || execution.generation() != attempt.generation()
            || execution.desired_digest() != attempt.desired_digest()
            || execution.node_identity() != attempt.required_node()
        {
            return Err(Error::PermissionDenied(
                "host activation claim is crossed with the authenticated execution".to_owned(),
            ));
        }
        Ok(Self {
            workload_uid: execution.workload_uid().as_str().to_owned(),
            node_identity: execution.node_identity().as_str().to_owned(),
            execution_id: execution.execution_id().clone(),
            execution_attempt_id: execution.attempt_id().clone(),
            execution_provider_id: provider_id.to_string(),
            attempt_id: attempt.attempt_id().as_str().to_owned(),
            dispatch_epoch: claim.dispatch_epoch().as_u64(),
            claimed_revision: claim.claimed_revision().as_u64(),
            generation: attempt.generation().as_u64(),
            desired_digest: attempt.desired_digest().to_string(),
            source_digest: attempt.source_digest().to_string(),
            network_plan_digest: attempt.network_plan_digest().to_string(),
        })
    }

    pub(crate) fn journal_fields(&self) -> Vec<String> {
        vec![
            format!("NIMBUS_WORKLOAD_UID={}", self.workload_uid),
            format!("NIMBUS_NODE_IDENTITY={}", self.node_identity),
            format!("NIMBUS_WORKLOAD_EXECUTION_ID={}", self.execution_id),
            format!(
                "NIMBUS_WORKLOAD_EXECUTION_ATTEMPT_ID={}",
                self.execution_attempt_id
            ),
            format!(
                "NIMBUS_WORKLOAD_EXECUTION_PROVIDER_ID={}",
                self.execution_provider_id
            ),
            format!("NIMBUS_PROVISION_ATTEMPT_ID={}", self.attempt_id),
            format!("NIMBUS_PROVISION_DISPATCH_EPOCH={}", self.dispatch_epoch),
            format!(
                "NIMBUS_PROVISION_CLAIMED_REVISION={}",
                self.claimed_revision
            ),
            format!("NIMBUS_WORKLOAD_GENERATION={}", self.generation),
            format!("NIMBUS_WORKLOAD_DESIRED_DIGEST={}", self.desired_digest),
            format!("NIMBUS_WORKLOAD_SOURCE_DIGEST={}", self.source_digest),
            format!("NIMBUS_NETWORK_PLAN_DIGEST={}", self.network_plan_digest),
        ]
    }

    /// Reconstruct the exact provider fence returned by systemd's
    /// `LogExtraFields` property.
    ///
    /// A legacy unit carrying only the workload execution selector is
    /// unfenced and returns `None`; any partial, duplicate, or malformed exact
    /// fence fails closed. Callers compare the reconstructed value with the
    /// compute-issued fence before adopting an existing unit.
    #[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
    fn from_log_extra_fields(fields: &[Vec<u8>]) -> Result<Option<Self>> {
        let mut retained = BTreeMap::<&'static str, &str>::new();
        for field in fields {
            if !field.starts_with(b"NIMBUS_") {
                continue;
            }
            let field = std::str::from_utf8(field).map_err(|_| {
                invalid_activation_fence("a Nimbus LogExtraFields value is not UTF-8")
            })?;
            let (name, value) = field.split_once('=').ok_or_else(|| {
                invalid_activation_fence("a Nimbus LogExtraFields value is not NAME=value")
            })?;
            let Some(name) = Self::JOURNAL_FIELD_NAMES
                .iter()
                .copied()
                .find(|candidate| *candidate == name)
            else {
                continue;
            };
            if value.is_empty() {
                return Err(invalid_activation_fence(format!(
                    "systemd activation fence field {name} is empty"
                )));
            }
            if retained.insert(name, value).is_some() {
                return Err(invalid_activation_fence(format!(
                    "systemd activation fence field {name} is duplicated"
                )));
            }
        }

        let exact_field_count = retained
            .keys()
            .filter(|name| **name != "NIMBUS_WORKLOAD_EXECUTION_ID")
            .count();
        if exact_field_count == 0 {
            return Ok(None);
        }
        if retained.len() != Self::JOURNAL_FIELD_NAMES.len() {
            return Err(invalid_activation_fence(
                "systemd activation fence is incomplete",
            ));
        }

        let field = |name: &'static str| -> Result<&str> {
            retained
                .get(name)
                .copied()
                .ok_or_else(|| invalid_activation_fence(format!("missing {name}")))
        };
        let workload_uid = field("NIMBUS_WORKLOAD_UID")?.to_owned();
        TenantWorkloadUid::try_from(workload_uid.clone()).map_err(|_| {
            invalid_activation_fence("systemd activation fence has an invalid workload UID")
        })?;
        let node_identity = field("NIMBUS_NODE_IDENTITY")?.to_owned();
        NodeIdentity::new(node_identity.clone()).map_err(|_| {
            invalid_activation_fence("systemd activation fence has an invalid node identity")
        })?;
        let execution_id = field("NIMBUS_WORKLOAD_EXECUTION_ID")?
            .parse::<WorkloadExecutionId>()
            .map_err(|_| {
                invalid_activation_fence("systemd activation fence has an invalid execution ID")
            })?;
        let execution_attempt_id = field("NIMBUS_WORKLOAD_EXECUTION_ATTEMPT_ID")?
            .parse::<WorkloadExecutionAttemptId>()
            .map_err(|_| {
                invalid_activation_fence(
                    "systemd activation fence has an invalid execution attempt ID",
                )
            })?;
        let execution_provider_id = field("NIMBUS_WORKLOAD_EXECUTION_PROVIDER_ID")?.to_owned();
        execution_provider_id
            .parse::<nimbus_workloads::WorkloadExecutionProviderId>()
            .map_err(|_| {
                invalid_activation_fence(
                    "systemd activation fence has an invalid execution provider ID",
                )
            })?;
        let attempt_id = field("NIMBUS_PROVISION_ATTEMPT_ID")?.to_owned();
        attempt_id
            .parse::<WorkloadProvisionAttemptId>()
            .map_err(|_| {
                invalid_activation_fence("systemd activation fence has an invalid attempt ID")
            })?;
        let dispatch_epoch =
            parse_fence_counter(field("NIMBUS_PROVISION_DISPATCH_EPOCH")?, "dispatch epoch")?;
        let claimed_revision = parse_fence_counter(
            field("NIMBUS_PROVISION_CLAIMED_REVISION")?,
            "claimed revision",
        )?;
        let generation =
            parse_fence_counter(field("NIMBUS_WORKLOAD_GENERATION")?, "workload generation")?;
        let desired_digest = field("NIMBUS_WORKLOAD_DESIRED_DIGEST")?.to_owned();
        desired_digest
            .parse::<WorkloadDesiredDigest>()
            .map_err(|_| {
                invalid_activation_fence("systemd activation fence has an invalid desired digest")
            })?;
        let source_digest = field("NIMBUS_WORKLOAD_SOURCE_DIGEST")?.to_owned();
        source_digest
            .parse::<WorkloadProvisionSourceDigest>()
            .map_err(|_| {
                invalid_activation_fence("systemd activation fence has an invalid source digest")
            })?;
        let network_plan_digest = field("NIMBUS_NETWORK_PLAN_DIGEST")?.to_owned();
        validate_lower_hex_digest(&network_plan_digest, "network plan digest")?;

        Ok(Some(Self {
            workload_uid,
            node_identity,
            execution_id,
            execution_attempt_id,
            execution_provider_id,
            attempt_id,
            dispatch_epoch,
            claimed_revision,
            generation,
            desired_digest,
            source_digest,
            network_plan_digest,
        }))
    }

    #[cfg(test)]
    fn for_test(plan: &HostLifecyclePlan, seed: u8, dispatch_epoch: u64) -> Self {
        let digest = |offset: u8| format!("{:02x}", seed.wrapping_add(offset)).repeat(32);
        Self {
            workload_uid: plan.spec().workload_uid().as_str().to_owned(),
            node_identity: plan
                .spec()
                .assigned_node_id()
                .expect("test plan must retain an assigned node")
                .as_str()
                .to_owned(),
            execution_id: plan.execution_id().clone(),
            execution_attempt_id: WorkloadExecutionAttemptId::for_execution(
                plan.execution_id(),
                nimbus_workloads::WorkloadRestartEpoch::new(0),
            ),
            execution_provider_id: format!("wep_{}", digest(5)),
            attempt_id: format!("wpa_{}", digest(1)),
            dispatch_epoch,
            claimed_revision: dispatch_epoch + 1,
            generation: plan.spec().generation().as_u64(),
            desired_digest: digest(2),
            source_digest: digest(3),
            network_plan_digest: digest(4),
        }
    }

    fn matches_restart_source(&self, claim: &HostRestartProviderClaim) -> bool {
        let source = claim.source_execution();
        self.workload_uid == source.workload_uid().as_str()
            && self.node_identity == source.node_identity().as_str()
            && self.execution_id == *source.execution_id()
            && self.execution_attempt_id == *source.attempt_id()
            && self.execution_provider_id == claim.provider_selection.to_string()
            && self.generation == source.generation().as_u64()
            && self.desired_digest == source.desired_digest().to_string()
            && self.source_digest == claim.source_digest.to_string()
            && self.network_plan_digest == claim.network_plan_digest.as_str()
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
fn invalid_activation_fence(reason: impl Into<String>) -> Error {
    Error::PermissionDenied(format!(
        "cannot adopt systemd unit with invalid activation fence: {}",
        reason.into()
    ))
}

#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
fn parse_fence_counter(value: &str, label: &str) -> Result<u64> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || value.bytes().any(|byte| !byte.is_ascii_digit())
    {
        return Err(invalid_activation_fence(format!(
            "systemd activation fence {label} is not canonical unsigned decimal text"
        )));
    }
    value.parse().map_err(|_| {
        invalid_activation_fence(format!("systemd activation fence {label} exceeds u64"))
    })
}

#[cfg(any(test, all(target_os = "linux", feature = "systemd-dbus")))]
fn validate_lower_hex_digest(value: &str, label: &str) -> Result<()> {
    if value.len() != 64
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_activation_fence(format!(
            "systemd activation fence {label} is not 64 lowercase hexadecimal characters"
        )));
    }
    Ok(())
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
    execution_id: WorkloadExecutionId,
    unit_name: SystemdUnitName,
    phase: TenantWorkloadPhase,
    reason: HostLifecycleStatusReason,
    message: Option<String>,
    lifecycle_evidence: TenantWorkloadLifecycleEvidence,
}

impl HostLifecycleStatus {
    pub(crate) fn new_for_backend(
        execution_id: WorkloadExecutionId,
        unit_name: SystemdUnitName,
        phase: TenantWorkloadPhase,
        reason: HostLifecycleStatusReason,
        message: Option<String>,
        lifecycle_evidence: TenantWorkloadLifecycleEvidence,
    ) -> Self {
        Self {
            execution_id,
            unit_name,
            phase,
            reason,
            message,
            lifecycle_evidence,
        }
    }

    pub fn from_backend_state(plan: &HostLifecyclePlan, state: HostBackendObservedState) -> Self {
        Self::from_provider_state(&HostProviderPlan::from_lifecycle(plan), state)
    }

    pub(crate) fn from_provider_state(
        plan: &HostProviderPlan,
        state: HostBackendObservedState,
    ) -> Self {
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
        let lifecycle_evidence = TenantWorkloadLifecycleEvidence::from_provider_plan(plan, reason)
            .with_message(message.clone());
        Self {
            execution_id: plan.execution_id.clone(),
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

    pub fn execution_id(&self) -> &WorkloadExecutionId {
        &self.execution_id
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

#[cfg(test)]
#[path = "host_lifecycle/tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) mod test_support;

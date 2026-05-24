use serde::Serialize;

use crate::error::{NimbusRuntimeError, Result};
use crate::limits::{
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeLimits, RuntimePoolKind,
};

use super::lifecycle::{BunJscLifecycleAck, BunJscLifecycleState, BunJscLifecycleTrace};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BunJscPoolMode {
    TrustedRetained,
    FreshDiscardOuterQuota,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BunJscPoolPolicy {
    pub(crate) mode: BunJscPoolMode,
    pub(crate) trust_tier: RuntimeBackendTrustTier,
    pub(crate) lockdown_profile: RuntimeBackendLockdownProfile,
    pub(crate) lifecycle_policy: RuntimeBackendLifecyclePolicy,
    pub(crate) runtime_pool_kind: RuntimePoolKind,
    pub(crate) retained_vm_reuse_allowed: bool,
    pub(crate) outer_quota_required: bool,
    pub(crate) product_selectable: bool,
}

impl BunJscPoolPolicy {
    pub(crate) fn from_limits(limits: &RuntimeLimits) -> Result<Self> {
        if !matches!(limits.backend_kind, RuntimeBackendKind::BunJsc) {
            return Err(NimbusRuntimeError::Contract(format!(
                "Bun/JSC pool policy requires Bun/JSC backend kind, got {:?}",
                limits.backend_kind
            )));
        }

        match (
            limits.backend_trust_tier,
            limits.backend_lockdown_profile,
            limits.backend_lifecycle_policy,
            limits.runtime_pool_kind,
        ) {
            (
                RuntimeBackendTrustTier::ProofOnly,
                RuntimeBackendLockdownProfile::BunJscProofOnly,
                RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
                RuntimePoolKind::BunJscTrustedRetained,
            )
            | (
                RuntimeBackendTrustTier::InProcessTrustedOnly,
                RuntimeBackendLockdownProfile::BunJscTrustedGeneratedWrapper,
                RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
                RuntimePoolKind::BunJscTrustedRetained,
            ) => Ok(Self {
                mode: BunJscPoolMode::TrustedRetained,
                trust_tier: limits.backend_trust_tier,
                lockdown_profile: limits.backend_lockdown_profile,
                lifecycle_policy: limits.backend_lifecycle_policy,
                runtime_pool_kind: limits.runtime_pool_kind,
                retained_vm_reuse_allowed: true,
                outer_quota_required: false,
                product_selectable: false,
            }),
            (
                RuntimeBackendTrustTier::InProcessUntrusted,
                RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
                RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired,
                RuntimePoolKind::BunJscFreshDiscard,
            ) => Ok(Self {
                mode: BunJscPoolMode::FreshDiscardOuterQuota,
                trust_tier: limits.backend_trust_tier,
                lockdown_profile: limits.backend_lockdown_profile,
                lifecycle_policy: limits.backend_lifecycle_policy,
                runtime_pool_kind: limits.runtime_pool_kind,
                retained_vm_reuse_allowed: false,
                outer_quota_required: true,
                product_selectable: true,
            }),
            (trust_tier, lockdown_profile, lifecycle_policy, runtime_pool_kind) => {
                Err(NimbusRuntimeError::Contract(format!(
                    "Bun/JSC pool policy requires matching trust, lockdown, lifecycle, and pool profiles, got {:?}, {:?}, {:?}, and {:?}",
                    trust_tier, lockdown_profile, lifecycle_policy, runtime_pool_kind
                )))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct BunJscPoolMetricsSnapshot {
    pub(crate) admitted_invocations: u64,
    pub(crate) disabled_invocations: u64,
    pub(crate) cancellation_requests: u64,
    pub(crate) event_loop_progress_ticks: u64,
    pub(crate) teardown_completions: u64,
}

#[derive(Debug, Default)]
pub(crate) struct BunJscPool {
    lifecycle: BunJscLifecycleTrace,
    metrics: BunJscPoolMetricsSnapshot,
}

impl BunJscPool {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn lifecycle_state(&self) -> BunJscLifecycleState {
        self.lifecycle.state()
    }

    pub(crate) fn lifecycle_transition_count(&self) -> usize {
        self.lifecycle.transitions().len()
    }

    pub(crate) fn metrics_snapshot(&self) -> BunJscPoolMetricsSnapshot {
        self.metrics
    }

    pub(crate) fn record_admission(&mut self) {
        self.metrics.admitted_invocations = self.metrics.admitted_invocations.saturating_add(1);
    }

    pub(crate) fn record_disabled_invocation(&mut self) {
        self.metrics.disabled_invocations = self.metrics.disabled_invocations.saturating_add(1);
    }

    pub(crate) fn record_event_loop_progress(&mut self) {
        self.metrics.event_loop_progress_ticks =
            self.metrics.event_loop_progress_ticks.saturating_add(1);
    }

    pub(crate) fn request_cancellation(&mut self) -> Result<()> {
        self.lifecycle
            .acknowledge(BunJscLifecycleAck::CancelRequested)
            .map_err(|error| NimbusRuntimeError::Contract(error.to_string()))?;
        self.metrics.cancellation_requests = self.metrics.cancellation_requests.saturating_add(1);
        Ok(())
    }

    pub(crate) fn acknowledge(&mut self, ack: BunJscLifecycleAck) -> Result<BunJscLifecycleState> {
        let state = self
            .lifecycle
            .acknowledge(ack)
            .map_err(|error| NimbusRuntimeError::Contract(error.to_string()))?;
        if matches!(state, BunJscLifecycleState::TeardownComplete) {
            self.metrics.teardown_completions = self.metrics.teardown_completions.saturating_add(1);
        }
        Ok(state)
    }

    pub(crate) fn verify_scaffold_contract() -> Result<()> {
        let mut pool = Self::new();
        if !matches!(pool.lifecycle_state(), BunJscLifecycleState::Created) {
            return Err(NimbusRuntimeError::Contract(
                "Bun/JSC lifecycle must start in Created".to_string(),
            ));
        }
        if pool.request_cancellation().is_ok() {
            return Err(NimbusRuntimeError::Contract(
                "Bun/JSC lifecycle must reject cancellation before guest entry".to_string(),
            ));
        }
        for ack in [
            BunJscLifecycleAck::BootstrapReady,
            BunJscLifecycleAck::GuestEntered,
            BunJscLifecycleAck::CancelRequested,
            BunJscLifecycleAck::Terminated,
            BunJscLifecycleAck::ResetOrDiscarded,
            BunJscLifecycleAck::TeardownComplete,
        ] {
            if matches!(ack, BunJscLifecycleAck::CancelRequested) {
                pool.request_cancellation()?;
            } else {
                pool.acknowledge(ack)?;
            }
        }
        pool.record_event_loop_progress();
        let metrics = pool.metrics_snapshot();
        if metrics.cancellation_requests != 1
            || metrics.event_loop_progress_ticks != 1
            || metrics.teardown_completions != 1
            || pool.lifecycle_transition_count() != 6
        {
            return Err(NimbusRuntimeError::Contract(
                "Bun/JSC lifecycle scaffold contract drifted".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn disabled_error() -> NimbusRuntimeError {
        NimbusRuntimeError::Contract(
            "Bun/JSC runtime backend is admitted only for the proven fresh/discard lockdown profile, but this Nimbus build does not link a Bun embedder execution adapter yet".to_string(),
        )
    }
}

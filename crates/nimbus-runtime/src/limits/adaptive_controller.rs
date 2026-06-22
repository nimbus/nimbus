use std::num::NonZeroUsize;

use serde::Serialize;

use super::{
    RuntimeControllerReplayAuthorityInput, RuntimeControllerReplayConfig,
    RuntimeControllerReplayDecision, RuntimeControllerReplayObservation, RuntimeHostPressureLevel,
    RuntimeHostResourceDecision, RuntimeMemoryPressureLevel, replay_runtime_controller,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAdaptiveControllerMode {
    Disabled,
    Shadow,
    Canary,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveCanaryPolicy {
    pub hash_modulus: NonZeroUsize,
    pub admitted_remainders: usize,
}

impl Default for RuntimeAdaptiveCanaryPolicy {
    fn default() -> Self {
        Self {
            hash_modulus: nonzero_usize(100),
            admitted_remainders: 0,
        }
    }
}

impl RuntimeAdaptiveCanaryPolicy {
    pub fn percent(percent: u8) -> Self {
        assert!(percent <= 100, "adaptive canary percent must be <= 100");
        Self {
            hash_modulus: nonzero_usize(100),
            admitted_remainders: usize::from(percent),
        }
    }

    pub fn admits(self, hash: u64) -> bool {
        if self.admitted_remainders == 0 {
            return false;
        }
        let modulus = u64::try_from(self.hash_modulus.get()).unwrap_or(u64::MAX);
        let admitted = u64::try_from(self.admitted_remainders).unwrap_or(u64::MAX);
        hash % modulus < admitted.min(modulus)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveControllerSettings {
    live_adaptive_defaults_enabled: bool,
    mode: RuntimeAdaptiveControllerMode,
    canary: RuntimeAdaptiveCanaryPolicy,
    rollback_to_static_defaults: bool,
    replay_config: RuntimeControllerReplayConfig,
}

impl Default for RuntimeAdaptiveControllerSettings {
    fn default() -> Self {
        Self {
            live_adaptive_defaults_enabled: false,
            mode: RuntimeAdaptiveControllerMode::Disabled,
            canary: RuntimeAdaptiveCanaryPolicy::default(),
            rollback_to_static_defaults: false,
            replay_config: RuntimeControllerReplayConfig::default(),
        }
    }
}

impl RuntimeAdaptiveControllerSettings {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn shadow(replay_config: RuntimeControllerReplayConfig) -> Self {
        Self {
            mode: RuntimeAdaptiveControllerMode::Shadow,
            replay_config,
            ..Self::default()
        }
    }

    pub fn canary(replay_config: RuntimeControllerReplayConfig, percent: u8) -> Self {
        Self {
            live_adaptive_defaults_enabled: percent > 0,
            mode: RuntimeAdaptiveControllerMode::Canary,
            canary: RuntimeAdaptiveCanaryPolicy::percent(percent),
            replay_config,
            ..Self::default()
        }
    }

    pub fn live(replay_config: RuntimeControllerReplayConfig) -> Self {
        Self {
            live_adaptive_defaults_enabled: true,
            mode: RuntimeAdaptiveControllerMode::Live,
            replay_config,
            ..Self::default()
        }
    }

    pub fn with_rollback_to_static_defaults(mut self, enabled: bool) -> Self {
        self.rollback_to_static_defaults = enabled;
        self
    }

    pub fn live_adaptive_defaults_enabled(self) -> bool {
        self.live_adaptive_defaults_enabled
    }

    pub fn mode(self) -> RuntimeAdaptiveControllerMode {
        self.mode
    }

    pub fn canary_policy(self) -> RuntimeAdaptiveCanaryPolicy {
        self.canary
    }

    pub fn rollback_to_static_defaults(self) -> bool {
        self.rollback_to_static_defaults
    }

    pub fn replay_config(self) -> RuntimeControllerReplayConfig {
        self.replay_config
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveWarmPoolAuthorityInput {
    pub replay_input: RuntimeControllerReplayAuthorityInput,
    pub static_warm_target: usize,
    pub current_retained_runtimes: usize,
    pub projected_bytes_per_runtime: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveWarmPoolSnapshot {
    pub observed_at_millis: u64,
    pub host_resource_decision: RuntimeHostResourceDecision,
    pub authorities: Vec<RuntimeAdaptiveWarmPoolAuthorityInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAdaptiveWarmPoolDecisionReason {
    DisabledStaticDefault,
    ShadowOnly,
    CanaryExcluded,
    CanaryActuation,
    LiveActuation,
    OperatorRollback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAdaptiveWarmPoolActuationKind {
    NoopDisabled,
    ShadowOnly,
    CanarySkipped,
    ApplyTarget,
    RollbackToStatic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveWarmPoolActuation {
    pub kind: RuntimeAdaptiveWarmPoolActuationKind,
    pub target_warm_runtimes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveWarmPoolDecision {
    pub replay: RuntimeControllerReplayDecision,
    pub static_warm_target: usize,
    pub current_retained_runtimes: usize,
    pub recommended_warm_target: usize,
    pub effective_warm_target: usize,
    pub projected_runtime_rss_bytes: u64,
    pub reason: RuntimeAdaptiveWarmPoolDecisionReason,
    pub actuation: RuntimeAdaptiveWarmPoolActuation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveWarmPoolEvaluation {
    pub observed_at_millis: u64,
    pub mode: RuntimeAdaptiveControllerMode,
    pub live_adaptive_defaults_enabled: bool,
    pub rollback_to_static_defaults: bool,
    pub host_pressure_level: RuntimeHostPressureLevel,
    pub memory_pressure_level: RuntimeMemoryPressureLevel,
    pub decisions: Vec<RuntimeAdaptiveWarmPoolDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveActuationResult {
    pub key_authority_hash: u64,
    pub attempted: bool,
    pub applied: bool,
    pub target_warm_runtimes: usize,
    pub kind: RuntimeAdaptiveWarmPoolActuationKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveWarmPoolRun {
    pub evaluation: RuntimeAdaptiveWarmPoolEvaluation,
    pub actuation_results: Vec<RuntimeAdaptiveActuationResult>,
}

pub trait RuntimeAdaptiveClock {
    fn now_millis(&self) -> u64;
}

pub trait RuntimeAdaptivePressureAdapter {
    fn host_resource_decision(&self) -> RuntimeHostResourceDecision;
}

pub trait RuntimeAdaptiveObservationSource {
    fn snapshot(
        &self,
        observed_at_millis: u64,
        host_resource_decision: RuntimeHostResourceDecision,
    ) -> RuntimeAdaptiveWarmPoolSnapshot;
}

pub trait RuntimeAdaptiveMetricsSink {
    fn record_controller_evaluation(&self, evaluation: &RuntimeAdaptiveWarmPoolEvaluation);
}

pub trait RuntimeAdaptiveActuator {
    fn apply_warm_pool_target(
        &self,
        decision: &RuntimeAdaptiveWarmPoolDecision,
    ) -> RuntimeAdaptiveActuationResult;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeAdaptiveWarmPoolController {
    settings: RuntimeAdaptiveControllerSettings,
}

impl RuntimeAdaptiveWarmPoolController {
    pub fn new(settings: RuntimeAdaptiveControllerSettings) -> Self {
        Self { settings }
    }

    pub fn settings(self) -> RuntimeAdaptiveControllerSettings {
        self.settings
    }

    pub fn evaluate_snapshot(
        &self,
        snapshot: RuntimeAdaptiveWarmPoolSnapshot,
    ) -> RuntimeAdaptiveWarmPoolEvaluation {
        let replay_inputs = snapshot
            .authorities
            .iter()
            .map(|authority| authority.replay_input_with_pressure(snapshot.host_resource_decision))
            .collect::<Vec<_>>();
        let replay_decisions =
            replay_runtime_controller(self.settings.replay_config(), &replay_inputs);
        let decisions = snapshot
            .authorities
            .iter()
            .zip(replay_decisions)
            .map(|(authority, replay)| self.decision_for_authority(authority, replay))
            .collect();

        RuntimeAdaptiveWarmPoolEvaluation {
            observed_at_millis: snapshot.observed_at_millis,
            mode: self.settings.mode(),
            live_adaptive_defaults_enabled: self.settings.live_adaptive_defaults_enabled(),
            rollback_to_static_defaults: self.settings.rollback_to_static_defaults(),
            host_pressure_level: snapshot.host_resource_decision.host_pressure_level,
            memory_pressure_level: snapshot.host_resource_decision.memory_pressure_level,
            decisions,
        }
    }

    pub fn run_with_adapters<O, C, P, M, A>(
        &self,
        observations: &O,
        clock: &C,
        pressure: &P,
        metrics: &M,
        actuator: &A,
    ) -> RuntimeAdaptiveWarmPoolRun
    where
        O: RuntimeAdaptiveObservationSource,
        C: RuntimeAdaptiveClock,
        P: RuntimeAdaptivePressureAdapter,
        M: RuntimeAdaptiveMetricsSink,
        A: RuntimeAdaptiveActuator,
    {
        let host_resource_decision = pressure.host_resource_decision();
        let snapshot = observations.snapshot(clock.now_millis(), host_resource_decision);
        let evaluation = self.evaluate_snapshot(snapshot);
        metrics.record_controller_evaluation(&evaluation);
        let actuation_results = evaluation
            .decisions
            .iter()
            .map(|decision| match decision.actuation.kind {
                RuntimeAdaptiveWarmPoolActuationKind::ApplyTarget
                | RuntimeAdaptiveWarmPoolActuationKind::RollbackToStatic => {
                    actuator.apply_warm_pool_target(decision)
                }
                kind => RuntimeAdaptiveActuationResult {
                    key_authority_hash: decision.replay.key.authority_hash,
                    attempted: false,
                    applied: false,
                    target_warm_runtimes: decision.effective_warm_target,
                    kind,
                },
            })
            .collect();
        RuntimeAdaptiveWarmPoolRun {
            evaluation,
            actuation_results,
        }
    }

    fn decision_for_authority(
        &self,
        authority: &RuntimeAdaptiveWarmPoolAuthorityInput,
        replay: RuntimeControllerReplayDecision,
    ) -> RuntimeAdaptiveWarmPoolDecision {
        let recommended_warm_target = replay.replayed_warm_target;
        let projected_runtime_rss_bytes = authority
            .projected_bytes_per_runtime
            .saturating_mul(usize_to_u64_saturating(recommended_warm_target));
        let (reason, actuation, effective_warm_target) =
            if self.settings.rollback_to_static_defaults() {
                (
                    RuntimeAdaptiveWarmPoolDecisionReason::OperatorRollback,
                    RuntimeAdaptiveWarmPoolActuation {
                        kind: RuntimeAdaptiveWarmPoolActuationKind::RollbackToStatic,
                        target_warm_runtimes: authority.static_warm_target,
                    },
                    authority.static_warm_target,
                )
            } else {
                match self.settings.mode() {
                    RuntimeAdaptiveControllerMode::Disabled => (
                        RuntimeAdaptiveWarmPoolDecisionReason::DisabledStaticDefault,
                        RuntimeAdaptiveWarmPoolActuation {
                            kind: RuntimeAdaptiveWarmPoolActuationKind::NoopDisabled,
                            target_warm_runtimes: authority.static_warm_target,
                        },
                        authority.static_warm_target,
                    ),
                    RuntimeAdaptiveControllerMode::Shadow => (
                        RuntimeAdaptiveWarmPoolDecisionReason::ShadowOnly,
                        RuntimeAdaptiveWarmPoolActuation {
                            kind: RuntimeAdaptiveWarmPoolActuationKind::ShadowOnly,
                            target_warm_runtimes: recommended_warm_target,
                        },
                        authority.static_warm_target,
                    ),
                    RuntimeAdaptiveControllerMode::Canary => {
                        if self
                            .settings
                            .canary_policy()
                            .admits(replay.key.authority_hash)
                        {
                            (
                                RuntimeAdaptiveWarmPoolDecisionReason::CanaryActuation,
                                RuntimeAdaptiveWarmPoolActuation {
                                    kind: RuntimeAdaptiveWarmPoolActuationKind::ApplyTarget,
                                    target_warm_runtimes: recommended_warm_target,
                                },
                                recommended_warm_target,
                            )
                        } else {
                            (
                                RuntimeAdaptiveWarmPoolDecisionReason::CanaryExcluded,
                                RuntimeAdaptiveWarmPoolActuation {
                                    kind: RuntimeAdaptiveWarmPoolActuationKind::CanarySkipped,
                                    target_warm_runtimes: recommended_warm_target,
                                },
                                authority.static_warm_target,
                            )
                        }
                    }
                    RuntimeAdaptiveControllerMode::Live => (
                        RuntimeAdaptiveWarmPoolDecisionReason::LiveActuation,
                        RuntimeAdaptiveWarmPoolActuation {
                            kind: RuntimeAdaptiveWarmPoolActuationKind::ApplyTarget,
                            target_warm_runtimes: recommended_warm_target,
                        },
                        recommended_warm_target,
                    ),
                }
            };

        RuntimeAdaptiveWarmPoolDecision {
            replay,
            static_warm_target: authority.static_warm_target,
            current_retained_runtimes: authority.current_retained_runtimes,
            recommended_warm_target,
            effective_warm_target,
            projected_runtime_rss_bytes,
            reason,
            actuation,
        }
    }
}

impl RuntimeAdaptiveWarmPoolAuthorityInput {
    fn replay_input_with_pressure(
        &self,
        host_resource_decision: RuntimeHostResourceDecision,
    ) -> RuntimeControllerReplayAuthorityInput {
        let mut replay_input = self.replay_input.clone();
        if let Some(latest) = replay_input.observations.last_mut() {
            apply_pressure_to_observation(latest, host_resource_decision);
        } else {
            let mut idle = RuntimeControllerReplayObservation::idle();
            apply_pressure_to_observation(&mut idle, host_resource_decision);
            replay_input.observations.push(idle);
        }
        replay_input
    }
}

fn apply_pressure_to_observation(
    observation: &mut RuntimeControllerReplayObservation,
    host_resource_decision: RuntimeHostResourceDecision,
) {
    observation.host_pressure_level = host_resource_decision.host_pressure_level;
    observation.memory_pressure_level = host_resource_decision.memory_pressure_level;
}

fn nonzero_usize(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("adaptive controller constant is nonzero")
}

fn usize_to_u64_saturating(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::limits::{
        RuntimeControllerReplayAuthorityKey, RuntimeControllerReplayState,
        RuntimeHostPressureSample, RuntimeHostResourceBudget, RuntimeMemoryPressureSample,
        RuntimeProfile,
    };

    fn config() -> RuntimeControllerReplayConfig {
        RuntimeControllerReplayConfig {
            stable_window_observations: nonzero_usize(2),
            panic_window_observations: nonzero_usize(1),
            max_scale_up_step: nonzero_usize(16),
            max_scale_down_step: nonzero_usize(16),
            scale_down_hysteresis_observations: 0,
            max_warm_runtimes_per_authority: 16,
            max_warm_runtimes_per_tenant: 16,
            ..RuntimeControllerReplayConfig::default()
        }
    }

    fn key(authority_hash: u64) -> RuntimeControllerReplayAuthorityKey {
        RuntimeControllerReplayAuthorityKey {
            tenant_hash: 7,
            authority_hash,
            profile: RuntimeProfile::WebLean,
        }
    }

    fn authority(
        authority_hash: u64,
        static_warm_target: usize,
    ) -> RuntimeAdaptiveWarmPoolAuthorityInput {
        RuntimeAdaptiveWarmPoolAuthorityInput {
            replay_input: RuntimeControllerReplayAuthorityInput {
                key: key(authority_hash),
                previous_state: RuntimeControllerReplayState {
                    current_warm_target: static_warm_target,
                    scale_down_observations_remaining: 0,
                },
                observations: vec![RuntimeControllerReplayObservation::nominal(
                    4, 2_000_000, 200_000,
                )],
            },
            static_warm_target,
            current_retained_runtimes: static_warm_target,
            projected_bytes_per_runtime: 128 * 1024 * 1024,
        }
    }

    fn host_decision(level: RuntimeHostPressureLevel) -> RuntimeHostResourceDecision {
        RuntimeHostResourceBudget {
            host_millicpus: 4_000,
            system_reserved_millicpus: 0,
            nimbus_control_plane_reserved_millicpus: 0,
            runtime_hard_ceiling_millicpus: None,
            runtime_seat_millicpus: std::num::NonZeroU32::new(1_000).unwrap(),
        }
        .decide(
            4,
            RuntimeHostPressureSample::observed(
                level,
                RuntimeMemoryPressureSample::observed(128, 256, 512).classify(),
                false,
            ),
        )
    }

    fn snapshot(
        authorities: Vec<RuntimeAdaptiveWarmPoolAuthorityInput>,
    ) -> RuntimeAdaptiveWarmPoolSnapshot {
        RuntimeAdaptiveWarmPoolSnapshot {
            observed_at_millis: 42,
            host_resource_decision: host_decision(RuntimeHostPressureLevel::Nominal),
            authorities,
        }
    }

    #[test]
    fn adaptive_controller_disabled_mode_uses_static_defaults() {
        let controller =
            RuntimeAdaptiveWarmPoolController::new(RuntimeAdaptiveControllerSettings::disabled());
        let evaluation = controller.evaluate_snapshot(snapshot(vec![authority(1, 1)]));
        let decision = &evaluation.decisions[0];

        assert_eq!(evaluation.mode, RuntimeAdaptiveControllerMode::Disabled);
        assert!(!evaluation.live_adaptive_defaults_enabled);
        assert_eq!(decision.recommended_warm_target, 3);
        assert_eq!(decision.effective_warm_target, 1);
        assert_eq!(
            decision.actuation.kind,
            RuntimeAdaptiveWarmPoolActuationKind::NoopDisabled
        );
    }

    #[test]
    fn adaptive_controller_shadow_mode_reports_without_actuation() {
        let controller = RuntimeAdaptiveWarmPoolController::new(
            RuntimeAdaptiveControllerSettings::shadow(config()),
        );
        let evaluation = controller.evaluate_snapshot(snapshot(vec![authority(1, 1)]));
        let decision = &evaluation.decisions[0];

        assert_eq!(evaluation.mode, RuntimeAdaptiveControllerMode::Shadow);
        assert!(!evaluation.live_adaptive_defaults_enabled);
        assert_eq!(decision.recommended_warm_target, 3);
        assert_eq!(decision.effective_warm_target, 1);
        assert_eq!(
            decision.actuation.kind,
            RuntimeAdaptiveWarmPoolActuationKind::ShadowOnly
        );
    }

    #[test]
    fn adaptive_controller_canary_actuates_only_admitted_authorities() {
        let controller = RuntimeAdaptiveWarmPoolController::new(
            RuntimeAdaptiveControllerSettings::canary(config(), 10),
        );
        let evaluation =
            controller.evaluate_snapshot(snapshot(vec![authority(7, 1), authority(17, 1)]));

        let admitted = &evaluation.decisions[0];
        let excluded = &evaluation.decisions[1];
        assert!(evaluation.live_adaptive_defaults_enabled);
        assert_eq!(
            admitted.actuation.kind,
            RuntimeAdaptiveWarmPoolActuationKind::ApplyTarget
        );
        assert_eq!(
            admitted.effective_warm_target,
            admitted.recommended_warm_target
        );
        assert_eq!(
            excluded.actuation.kind,
            RuntimeAdaptiveWarmPoolActuationKind::CanarySkipped
        );
        assert_eq!(excluded.effective_warm_target, excluded.static_warm_target);
    }

    #[test]
    fn adaptive_controller_operator_rollback_forces_static_defaults() {
        let controller = RuntimeAdaptiveWarmPoolController::new(
            RuntimeAdaptiveControllerSettings::live(config())
                .with_rollback_to_static_defaults(true),
        );
        let evaluation = controller.evaluate_snapshot(snapshot(vec![authority(1, 2)]));
        let decision = &evaluation.decisions[0];

        assert!(evaluation.rollback_to_static_defaults);
        assert_eq!(decision.effective_warm_target, 2);
        assert_eq!(
            decision.actuation.kind,
            RuntimeAdaptiveWarmPoolActuationKind::RollbackToStatic
        );
        assert_eq!(
            decision.reason,
            RuntimeAdaptiveWarmPoolDecisionReason::OperatorRollback
        );
    }

    #[test]
    fn adaptive_controller_pressure_snapshot_overrides_latest_observation() {
        let controller = RuntimeAdaptiveWarmPoolController::new(
            RuntimeAdaptiveControllerSettings::live(config()),
        );
        let mut snapshot = snapshot(vec![authority(1, 4)]);
        snapshot.host_resource_decision = host_decision(RuntimeHostPressureLevel::Critical);

        let evaluation = controller.evaluate_snapshot(snapshot);
        let decision = &evaluation.decisions[0];

        assert_eq!(
            evaluation.host_pressure_level,
            RuntimeHostPressureLevel::Critical
        );
        assert_eq!(decision.recommended_warm_target, 0);
        assert_eq!(decision.effective_warm_target, 0);
        assert!(decision.replay.evict_idle_retained_runtimes);
    }

    #[derive(Default)]
    struct FakeClock;

    impl RuntimeAdaptiveClock for FakeClock {
        fn now_millis(&self) -> u64 {
            1234
        }
    }

    struct FakePressure(RuntimeHostResourceDecision);

    impl RuntimeAdaptivePressureAdapter for FakePressure {
        fn host_resource_decision(&self) -> RuntimeHostResourceDecision {
            self.0
        }
    }

    struct FakeObservationSource {
        authorities: Vec<RuntimeAdaptiveWarmPoolAuthorityInput>,
    }

    impl RuntimeAdaptiveObservationSource for FakeObservationSource {
        fn snapshot(
            &self,
            observed_at_millis: u64,
            host_resource_decision: RuntimeHostResourceDecision,
        ) -> RuntimeAdaptiveWarmPoolSnapshot {
            RuntimeAdaptiveWarmPoolSnapshot {
                observed_at_millis,
                host_resource_decision,
                authorities: self.authorities.clone(),
            }
        }
    }

    #[derive(Default)]
    struct FakeMetrics {
        recorded: Cell<usize>,
    }

    impl RuntimeAdaptiveMetricsSink for FakeMetrics {
        fn record_controller_evaluation(&self, evaluation: &RuntimeAdaptiveWarmPoolEvaluation) {
            self.recorded.set(evaluation.decisions.len());
        }
    }

    #[derive(Default)]
    struct FakeActuator {
        applied_targets: RefCell<Vec<usize>>,
    }

    impl RuntimeAdaptiveActuator for FakeActuator {
        fn apply_warm_pool_target(
            &self,
            decision: &RuntimeAdaptiveWarmPoolDecision,
        ) -> RuntimeAdaptiveActuationResult {
            self.applied_targets
                .borrow_mut()
                .push(decision.effective_warm_target);
            RuntimeAdaptiveActuationResult {
                key_authority_hash: decision.replay.key.authority_hash,
                attempted: true,
                applied: true,
                target_warm_runtimes: decision.effective_warm_target,
                kind: decision.actuation.kind,
            }
        }
    }

    #[test]
    fn adaptive_controller_adapters_keep_observation_metrics_and_actuation_separate() {
        let controller = RuntimeAdaptiveWarmPoolController::new(
            RuntimeAdaptiveControllerSettings::live(config()),
        );
        let observations = FakeObservationSource {
            authorities: vec![authority(1, 1)],
        };
        let metrics = FakeMetrics::default();
        let actuator = FakeActuator::default();

        let run = controller.run_with_adapters(
            &observations,
            &FakeClock,
            &FakePressure(host_decision(RuntimeHostPressureLevel::Nominal)),
            &metrics,
            &actuator,
        );

        assert_eq!(run.evaluation.observed_at_millis, 1234);
        assert_eq!(metrics.recorded.get(), 1);
        assert_eq!(run.actuation_results.len(), 1);
        assert!(run.actuation_results[0].attempted);
        assert_eq!(actuator.applied_targets.borrow().as_slice(), &[3]);
    }
}

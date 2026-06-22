use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use serde::Serialize;

use super::{RuntimeHostPressureLevel, RuntimeMemoryPressureLevel, RuntimeProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeControllerReplayConfig {
    pub observation_window_millis: NonZeroUsize,
    pub stable_window_observations: NonZeroUsize,
    pub panic_window_observations: NonZeroUsize,
    pub headroom_entries: usize,
    pub max_scale_up_step: NonZeroUsize,
    pub max_scale_down_step: NonZeroUsize,
    pub scale_down_hysteresis_observations: usize,
    pub max_warm_runtimes_per_authority: usize,
    pub max_warm_runtimes_per_tenant: usize,
    pub compute_stall_signal_per_mille: u16,
}

impl Default for RuntimeControllerReplayConfig {
    fn default() -> Self {
        Self {
            observation_window_millis: nonzero_usize(1_000),
            stable_window_observations: nonzero_usize(60),
            panic_window_observations: nonzero_usize(5),
            headroom_entries: 1,
            max_scale_up_step: nonzero_usize(4),
            max_scale_down_step: nonzero_usize(2),
            scale_down_hysteresis_observations: 3,
            max_warm_runtimes_per_authority: 16,
            max_warm_runtimes_per_tenant: 64,
            compute_stall_signal_per_mille: 800,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct RuntimeControllerReplayAuthorityKey {
    pub tenant_hash: u64,
    pub authority_hash: u64,
    pub profile: RuntimeProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeControllerReplayObservation {
    pub arrivals: u64,
    pub occupancy_micros_total: u64,
    pub compute_micros_total: u64,
    pub isolate_stall_micros_total: u64,
    pub spillover_requests: u64,
    pub host_pressure_level: RuntimeHostPressureLevel,
    pub memory_pressure_level: RuntimeMemoryPressureLevel,
}

impl RuntimeControllerReplayObservation {
    pub fn nominal(arrivals: u64, occupancy_micros_total: u64, compute_micros_total: u64) -> Self {
        Self {
            arrivals,
            occupancy_micros_total,
            compute_micros_total,
            isolate_stall_micros_total: 0,
            spillover_requests: 0,
            host_pressure_level: RuntimeHostPressureLevel::Nominal,
            memory_pressure_level: RuntimeMemoryPressureLevel::Nominal,
        }
    }

    pub fn idle() -> Self {
        Self::nominal(0, 0, 0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RuntimeControllerReplayState {
    pub current_warm_target: usize,
    pub scale_down_observations_remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeControllerReplayAuthorityInput {
    pub key: RuntimeControllerReplayAuthorityKey,
    pub previous_state: RuntimeControllerReplayState,
    pub observations: Vec<RuntimeControllerReplayObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeControllerReplayDecision {
    pub key: RuntimeControllerReplayAuthorityKey,
    pub desired_warm_target: usize,
    pub replayed_warm_target: usize,
    pub prewarming_paused: bool,
    pub evict_idle_retained_runtimes: bool,
    pub cpu_scale_signal: bool,
    pub isolate_stall_signal: bool,
    pub spillover_signal: bool,
    pub rate_limited: bool,
    pub hysteresis_held: bool,
    pub tenant_cap_limited: bool,
    pub next_state: RuntimeControllerReplayState,
}

pub fn replay_runtime_controller(
    config: RuntimeControllerReplayConfig,
    inputs: &[RuntimeControllerReplayAuthorityInput],
) -> Vec<RuntimeControllerReplayDecision> {
    let mut decisions = inputs
        .iter()
        .map(|input| replay_authority(config, input))
        .collect::<Vec<_>>();
    apply_tenant_cap(config, &mut decisions);
    decisions
}

fn replay_authority(
    config: RuntimeControllerReplayConfig,
    input: &RuntimeControllerReplayAuthorityInput,
) -> RuntimeControllerReplayDecision {
    let stable_window = ReplayWindow::from_observations(
        &input.observations,
        config.stable_window_observations.get(),
        config.observation_window_millis.get(),
    );
    let panic_window = ReplayWindow::from_observations(
        &input.observations,
        config.panic_window_observations.get(),
        config.observation_window_millis.get(),
    );
    let latest = input
        .observations
        .last()
        .copied()
        .unwrap_or_else(RuntimeControllerReplayObservation::idle);

    let stable_target = stable_window.warm_target(config.headroom_entries);
    let panic_target = panic_window.warm_target(config.headroom_entries);
    let isolate_stall_signal =
        stable_window.isolate_stall_micros_total > 0 || panic_window.isolate_stall_micros_total > 0;
    let spillover_signal =
        stable_window.spillover_requests > 0 || panic_window.spillover_requests > 0;
    let burst_signal = isolate_stall_signal || spillover_signal;
    let mut desired_warm_target = if burst_signal {
        stable_target.max(panic_target)
    } else {
        stable_target
    }
    .min(config.max_warm_runtimes_per_authority);

    let prewarming_paused = !matches!(
        latest.host_pressure_level,
        RuntimeHostPressureLevel::Nominal
    ) || !matches!(
        latest.memory_pressure_level,
        RuntimeMemoryPressureLevel::Nominal
    );
    let evict_idle_retained_runtimes = matches!(
        latest.host_pressure_level,
        RuntimeHostPressureLevel::Critical
    ) || !matches!(
        latest.memory_pressure_level,
        RuntimeMemoryPressureLevel::Nominal
    );
    if matches!(
        latest.host_pressure_level,
        RuntimeHostPressureLevel::Critical
    ) || matches!(
        latest.memory_pressure_level,
        RuntimeMemoryPressureLevel::Critical
    ) {
        desired_warm_target = 0;
    } else if prewarming_paused {
        desired_warm_target = desired_warm_target.min(input.previous_state.current_warm_target);
    }

    let cpu_scale_signal =
        stable_window.compute_per_mille() >= u64::from(config.compute_stall_signal_per_mille);
    let (replayed_warm_target, rate_limited, hysteresis_held, next_hysteresis) =
        apply_rate_and_hysteresis(
            config,
            input.previous_state,
            desired_warm_target,
            prewarming_paused && evict_idle_retained_runtimes,
        );

    RuntimeControllerReplayDecision {
        key: input.key,
        desired_warm_target,
        replayed_warm_target,
        prewarming_paused,
        evict_idle_retained_runtimes,
        cpu_scale_signal,
        isolate_stall_signal,
        spillover_signal,
        rate_limited,
        hysteresis_held,
        tenant_cap_limited: false,
        next_state: RuntimeControllerReplayState {
            current_warm_target: replayed_warm_target,
            scale_down_observations_remaining: next_hysteresis,
        },
    }
}

fn apply_rate_and_hysteresis(
    config: RuntimeControllerReplayConfig,
    state: RuntimeControllerReplayState,
    desired_warm_target: usize,
    bypass_scale_down_delay: bool,
) -> (usize, bool, bool, usize) {
    let current = state.current_warm_target;
    if desired_warm_target > current {
        let capped = desired_warm_target.min(current + config.max_scale_up_step.get());
        return (
            capped,
            capped != desired_warm_target,
            false,
            config.scale_down_hysteresis_observations,
        );
    }
    if desired_warm_target < current {
        if !bypass_scale_down_delay && state.scale_down_observations_remaining > 0 {
            return (
                current,
                false,
                true,
                state.scale_down_observations_remaining - 1,
            );
        }
        if bypass_scale_down_delay {
            return (desired_warm_target, false, false, 0);
        }
        let capped =
            desired_warm_target.max(current.saturating_sub(config.max_scale_down_step.get()));
        return (capped, capped != desired_warm_target, false, 0);
    }
    (
        current,
        false,
        false,
        config.scale_down_hysteresis_observations,
    )
}

fn apply_tenant_cap(
    config: RuntimeControllerReplayConfig,
    decisions: &mut [RuntimeControllerReplayDecision],
) {
    let mut tenant_indexes: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (index, decision) in decisions.iter().enumerate() {
        tenant_indexes
            .entry(decision.key.tenant_hash)
            .or_default()
            .push(index);
    }

    for indexes in tenant_indexes.values() {
        let total = indexes
            .iter()
            .map(|index| decisions[*index].replayed_warm_target)
            .sum::<usize>();
        if total <= config.max_warm_runtimes_per_tenant {
            continue;
        }

        let mut remaining = config.max_warm_runtimes_per_tenant;
        let mut allocated = indexes
            .iter()
            .map(|index| (*index, 0usize))
            .collect::<BTreeMap<_, _>>();

        let mut by_cold_first = indexes.clone();
        by_cold_first.sort_by_key(|index| {
            (
                decisions[*index].replayed_warm_target,
                decisions[*index].key.authority_hash,
            )
        });
        for index in by_cold_first {
            if remaining == 0 {
                break;
            }
            if decisions[index].replayed_warm_target > 0 {
                allocated.insert(index, 1);
                remaining -= 1;
            }
        }

        let mut by_demand = indexes.clone();
        by_demand.sort_by_key(|index| {
            (
                std::cmp::Reverse(decisions[*index].replayed_warm_target),
                decisions[*index].key.authority_hash,
            )
        });
        while remaining > 0 {
            let mut progressed = false;
            for index in &by_demand {
                if remaining == 0 {
                    break;
                }
                let current = allocated[index];
                if current < decisions[*index].replayed_warm_target {
                    allocated.insert(*index, current + 1);
                    remaining -= 1;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }

        for index in indexes {
            let capped = allocated[index];
            if capped < decisions[*index].replayed_warm_target {
                decisions[*index].replayed_warm_target = capped;
                decisions[*index].tenant_cap_limited = true;
                decisions[*index].next_state.current_warm_target = capped;
                decisions[*index]
                    .next_state
                    .scale_down_observations_remaining = 0;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ReplayWindow {
    duration_micros: u64,
    arrivals: u64,
    occupancy_micros_total: u64,
    compute_micros_total: u64,
    isolate_stall_micros_total: u64,
    spillover_requests: u64,
}

impl ReplayWindow {
    fn from_observations(
        observations: &[RuntimeControllerReplayObservation],
        max_observations: usize,
        observation_window_millis: usize,
    ) -> Self {
        let selected = observations
            .iter()
            .rev()
            .take(max_observations)
            .copied()
            .collect::<Vec<_>>();
        let mut window = Self {
            duration_micros: usize_to_u64_saturating(selected.len())
                .saturating_mul(usize_to_u64_saturating(observation_window_millis))
                .saturating_mul(1_000),
            ..Self::default()
        };
        for observation in selected {
            window.arrivals = window.arrivals.saturating_add(observation.arrivals);
            window.occupancy_micros_total = window
                .occupancy_micros_total
                .saturating_add(observation.occupancy_micros_total);
            window.compute_micros_total = window
                .compute_micros_total
                .saturating_add(observation.compute_micros_total);
            window.isolate_stall_micros_total = window
                .isolate_stall_micros_total
                .saturating_add(observation.isolate_stall_micros_total);
            window.spillover_requests = window
                .spillover_requests
                .saturating_add(observation.spillover_requests);
        }
        window
    }

    fn warm_target(self, headroom_entries: usize) -> usize {
        if self.arrivals == 0
            && self.isolate_stall_micros_total == 0
            && self.spillover_requests == 0
        {
            return 0;
        }
        usize_from_u64_saturating(ceil_div_u64(
            self.occupancy_micros_total.max(1),
            self.duration_micros.max(1),
        ))
        .saturating_add(headroom_entries)
    }

    fn compute_per_mille(self) -> u64 {
        if self.duration_micros == 0 {
            return 0;
        }
        self.compute_micros_total
            .saturating_mul(1_000)
            .saturating_div(self.duration_micros)
    }
}

fn ceil_div_u64(numerator: u64, denominator: u64) -> u64 {
    numerator / denominator + u64::from(numerator % denominator != 0)
}

fn nonzero_usize(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("controller replay config uses nonzero constants")
}

fn usize_to_u64_saturating(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn usize_from_u64_saturating(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(authority_hash: u64) -> RuntimeControllerReplayAuthorityKey {
        RuntimeControllerReplayAuthorityKey {
            tenant_hash: 7,
            authority_hash,
            profile: RuntimeProfile::NodeFull,
        }
    }

    fn config() -> RuntimeControllerReplayConfig {
        RuntimeControllerReplayConfig {
            stable_window_observations: nonzero_usize(4),
            panic_window_observations: nonzero_usize(1),
            max_scale_up_step: nonzero_usize(16),
            max_scale_down_step: nonzero_usize(16),
            scale_down_hysteresis_observations: 2,
            max_warm_runtimes_per_authority: 16,
            max_warm_runtimes_per_tenant: 16,
            ..RuntimeControllerReplayConfig::default()
        }
    }

    fn input(
        authority_hash: u64,
        current_warm_target: usize,
        observations: Vec<RuntimeControllerReplayObservation>,
    ) -> RuntimeControllerReplayAuthorityInput {
        RuntimeControllerReplayAuthorityInput {
            key: key(authority_hash),
            previous_state: RuntimeControllerReplayState {
                current_warm_target,
                scale_down_observations_remaining: 0,
            },
            observations,
        }
    }

    #[test]
    fn controller_replay_uses_stable_and_panic_windows_for_burst_targets() {
        let mut observations = vec![
            RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
            RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
            RuntimeControllerReplayObservation::nominal(1, 250_000, 50_000),
        ];
        let mut burst = RuntimeControllerReplayObservation::nominal(8, 4_000_000, 500_000);
        burst.spillover_requests = 3;
        burst.isolate_stall_micros_total = 250_000;
        observations.push(burst);

        let decisions = replay_runtime_controller(config(), &[input(1, 0, observations)]);
        let decision = decisions[0];

        assert_eq!(decision.desired_warm_target, 5);
        assert_eq!(decision.replayed_warm_target, 5);
        assert!(decision.spillover_signal);
        assert!(decision.isolate_stall_signal);
        assert!(!decision.cpu_scale_signal);
    }

    #[test]
    fn controller_replay_holds_scale_down_until_hysteresis_expires() {
        let input = RuntimeControllerReplayAuthorityInput {
            key: key(1),
            previous_state: RuntimeControllerReplayState {
                current_warm_target: 4,
                scale_down_observations_remaining: 1,
            },
            observations: vec![RuntimeControllerReplayObservation::idle()],
        };

        let decisions = replay_runtime_controller(config(), &[input]);
        let decision = decisions[0];

        assert_eq!(decision.desired_warm_target, 0);
        assert_eq!(decision.replayed_warm_target, 4);
        assert!(decision.hysteresis_held);
        assert_eq!(decision.next_state.scale_down_observations_remaining, 0);
    }

    #[test]
    fn controller_replay_pauses_and_shrinks_under_memory_pressure() {
        let mut critical = RuntimeControllerReplayObservation::nominal(10, 5_000_000, 250_000);
        critical.memory_pressure_level = RuntimeMemoryPressureLevel::Critical;

        let decisions = replay_runtime_controller(config(), &[input(1, 6, vec![critical])]);
        let decision = decisions[0];

        assert_eq!(decision.desired_warm_target, 0);
        assert_eq!(decision.replayed_warm_target, 0);
        assert!(decision.prewarming_paused);
        assert!(decision.evict_idle_retained_runtimes);
    }

    #[test]
    fn controller_replay_separates_compute_stall_from_warm_capacity_stall() {
        let decision = replay_runtime_controller(
            config(),
            &[input(
                1,
                0,
                vec![RuntimeControllerReplayObservation::nominal(
                    10, 900_000, 900_000,
                )],
            )],
        )[0];

        assert_eq!(decision.desired_warm_target, 2);
        assert!(decision.cpu_scale_signal);
        assert!(!decision.isolate_stall_signal);
    }

    #[test]
    fn controller_replay_applies_tenant_caps_to_zipf_hot_cold_mix() {
        let capped_config = RuntimeControllerReplayConfig {
            max_warm_runtimes_per_tenant: 2,
            ..config()
        };
        let hot = input(
            1,
            0,
            vec![RuntimeControllerReplayObservation::nominal(
                100, 10_000_000, 1_000_000,
            )],
        );
        let cold = input(
            2,
            0,
            vec![RuntimeControllerReplayObservation::nominal(
                1, 100_000, 10_000,
            )],
        );

        let decisions = replay_runtime_controller(capped_config, &[hot, cold]);
        let hot_decision = decisions
            .iter()
            .find(|decision| decision.key.authority_hash == 1)
            .expect("hot authority decision should exist");
        let cold_decision = decisions
            .iter()
            .find(|decision| decision.key.authority_hash == 2)
            .expect("cold authority decision should exist");

        assert_eq!(hot_decision.replayed_warm_target, 1);
        assert_eq!(cold_decision.replayed_warm_target, 1);
        assert!(hot_decision.tenant_cap_limited);
        assert!(
            cold_decision.tenant_cap_limited,
            "headroom can be trimmed, but the cold authority must not be starved"
        );
    }

    #[test]
    fn controller_replay_decays_periodic_load_with_rate_limit_after_hysteresis() {
        let periodic_idle = input(1, 5, vec![RuntimeControllerReplayObservation::idle()]);
        let periodic_idle = RuntimeControllerReplayAuthorityInput {
            previous_state: RuntimeControllerReplayState {
                current_warm_target: 5,
                scale_down_observations_remaining: 0,
            },
            ..periodic_idle
        };
        let slow_scale_down_config = RuntimeControllerReplayConfig {
            max_scale_down_step: nonzero_usize(2),
            ..config()
        };

        let decisions = replay_runtime_controller(slow_scale_down_config, &[periodic_idle]);
        let decision = decisions[0];

        assert_eq!(decision.desired_warm_target, 0);
        assert_eq!(decision.replayed_warm_target, 3);
        assert!(decision.rate_limited);
    }
}

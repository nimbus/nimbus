use super::*;

/// One mixed-load round measured from the engine's cumulative B2 phase metrics.
///
/// Provider RTT rounds reopen the engine from a seed before every sample, so
/// the selected tenant runtimes begin with zero phase observations. The
/// post-round snapshot therefore describes only the mutations in this round.
#[derive(Debug, Clone, Copy)]
pub(super) struct GateHoldRound {
    elapsed: Duration,
    completed_mutation_ops: u64,
    phase_sample_count: u64,
    gate_hold: Duration,
    total_commit: Duration,
}

impl GateHoldRound {
    pub(super) fn from_fresh_engine(
        engine: &Engine,
        tenant_states: &[TenantState],
        tenant_limit: usize,
        ops_per_tenant: usize,
        elapsed: Duration,
    ) -> BenchResult<Self> {
        let expected_mutation_ops = mixed_load_mutation_ops(tenant_limit, ops_per_tenant)?;
        let mut phase_sample_count = 0_u64;
        let mut completed_mutation_ops = 0_u64;
        let mut gate_hold_nanos = 0_u64;
        let mut total_commit_nanos = 0_u64;

        for state in tenant_states.iter().take(tenant_limit) {
            let snapshot = engine
                .tenant_engine_diagnostics(&state.tenant_id)?
                .commit_phases;
            phase_sample_count = phase_sample_count.saturating_add(snapshot.sample_count);
            completed_mutation_ops = completed_mutation_ops.saturating_add(snapshot.commit_count);
            gate_hold_nanos = gate_hold_nanos.saturating_add(snapshot.durable_append_nanos);
            total_commit_nanos = total_commit_nanos.saturating_add(snapshot.total_commit_nanos);
        }

        if completed_mutation_ops != expected_mutation_ops {
            return Err(format!(
                "mixed-load phase metrics recorded {completed_mutation_ops} commits; expected {expected_mutation_ops} logical mutation ops"
            )
            .into());
        }
        if phase_sample_count == 0 {
            return Err("mixed-load phase metrics recorded no committer samples".into());
        }

        Ok(Self {
            elapsed,
            completed_mutation_ops,
            phase_sample_count,
            gate_hold: Duration::from_nanos(gate_hold_nanos),
            total_commit: Duration::from_nanos(total_commit_nanos),
        })
    }

    pub(super) fn elapsed(self) -> Duration {
        self.elapsed
    }

    fn mean_gate_hold_per_commit(self) -> Duration {
        duration_from_nanos_f64(
            self.gate_hold.as_secs_f64() * 1_000_000_000.0
                / self.completed_mutation_ops.max(1) as f64,
        )
    }
}

#[derive(Debug, Clone)]
pub(super) struct GateHoldMeasurement {
    backend: MeasuredBackend,
    concurrent_tenants: usize,
    nominal_injected_rtt: Option<Duration>,
    rounds: Vec<GateHoldRound>,
}

pub(super) fn nominal_injected_round_trip(per_direction_delay: Duration) -> Duration {
    per_direction_delay.saturating_mul(2)
}

pub(super) fn record_gate_hold_measurements(
    report: &mut BenchmarkReport,
    concurrent_tenants: usize,
    per_direction_delay: Duration,
    loopback: Vec<GateHoldRound>,
    injected_rtt: Vec<GateHoldRound>,
) {
    report.gate_hold_measurements.push(GateHoldMeasurement {
        backend: MeasuredBackend::provider_loopback(),
        concurrent_tenants,
        nominal_injected_rtt: None,
        rounds: loopback,
    });
    report.gate_hold_measurements.push(GateHoldMeasurement {
        backend: MeasuredBackend::provider_injected_rtt(),
        concurrent_tenants,
        nominal_injected_rtt: Some(nominal_injected_round_trip(per_direction_delay)),
        rounds: injected_rtt,
    });
}

pub(super) fn render_gate_hold_report(
    markdown: &mut String,
    config: &BenchmarkConfig,
    report: &BenchmarkReport,
) {
    if report.gate_hold_measurements.is_empty() {
        return;
    }

    markdown.push_str("## Mixed-Load Commit Gate Under Injected RTT\n\n");
    markdown.push_str(&format!(
        "Gate hold is the B2 `durable_append` phase while the tenant sequence guard is held. The proxy adds `{}` in each direction, giving a nominal injected round trip of `{}`. Mean gate hold is exact across all recorded commits; the median is the median round-level mean because B2 snapshots are cumulative counters rather than per-commit histograms. Gate-hold share is `durable_append / total_commit` across the same committer samples.\n\n",
        format_duration(config.rtt_delay),
        format_duration(nominal_injected_round_trip(config.rtt_delay)),
    ));
    markdown.push_str(
        "Effective mutation ops per RTT is `completed mutation ops / (round wall time / nominal injected RTT)`. This normalizes completed inserts and updates by configured request/response delay intervals; it measures overlap or batching efficiency without treating proxy chunks as protocol-level round-trip counts. Reads and queries remain in round wall time but are not counted as mutation ops.\n\n",
    );
    markdown.push_str(
        "| Backend | Concurrent tenants | Rounds | Mutation commits / round | Phase samples / round | Mean gate hold / commit | Median round mean gate hold / commit | Gate-hold share of commit | Mean round wall time | Mean effective mutation ops / RTT | Median effective mutation ops / RTT |\n",
    );
    markdown.push_str(
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for measurement in &report.gate_hold_measurements {
        let stats = GateHoldStats::from_measurement(measurement);
        markdown.push_str(&format!(
            "| {} | {} | {} | {:.2} | {:.2} | {} | {} | {:.2}% | {} | {} | {} |\n",
            measurement.backend.label(),
            measurement.concurrent_tenants,
            stats.round_count,
            stats.mean_mutation_commits_per_round,
            stats.mean_phase_samples_per_round,
            format_duration(stats.mean_gate_hold_per_commit),
            format_duration(stats.median_round_mean_gate_hold_per_commit),
            stats.gate_hold_commit_share * 100.0,
            format_duration(stats.mean_round_wall_time),
            format_optional_rate(stats.mean_effective_ops_per_rtt),
            format_optional_rate(stats.median_effective_ops_per_rtt),
        ));
    }
    markdown.push('\n');
}

#[derive(Debug)]
struct GateHoldStats {
    round_count: usize,
    mean_mutation_commits_per_round: f64,
    mean_phase_samples_per_round: f64,
    mean_gate_hold_per_commit: Duration,
    median_round_mean_gate_hold_per_commit: Duration,
    gate_hold_commit_share: f64,
    mean_round_wall_time: Duration,
    mean_effective_ops_per_rtt: Option<f64>,
    median_effective_ops_per_rtt: Option<f64>,
}

impl GateHoldStats {
    fn from_measurement(measurement: &GateHoldMeasurement) -> Self {
        assert!(
            !measurement.rounds.is_empty(),
            "gate-hold measurements should contain rounds"
        );
        let round_count = measurement.rounds.len();
        let total_mutation_ops = measurement
            .rounds
            .iter()
            .map(|round| round.completed_mutation_ops)
            .sum::<u64>();
        let total_phase_samples = measurement
            .rounds
            .iter()
            .map(|round| round.phase_sample_count)
            .sum::<u64>();
        let total_gate_hold_secs = measurement
            .rounds
            .iter()
            .map(|round| round.gate_hold.as_secs_f64())
            .sum::<f64>();
        let total_commit_secs = measurement
            .rounds
            .iter()
            .map(|round| round.total_commit.as_secs_f64())
            .sum::<f64>();
        let mean_wall_secs = measurement
            .rounds
            .iter()
            .map(|round| round.elapsed.as_secs_f64())
            .sum::<f64>()
            / round_count as f64;
        let mut round_gate_hold_nanos = measurement
            .rounds
            .iter()
            .map(|round| round.mean_gate_hold_per_commit().as_secs_f64() * 1_000_000_000.0)
            .collect::<Vec<_>>();
        round_gate_hold_nanos.sort_by(f64::total_cmp);

        let mut effective_ops_per_rtt = measurement.nominal_injected_rtt.map(|rtt| {
            measurement
                .rounds
                .iter()
                .map(|round| effective_ops_per_round_trip(*round, rtt))
                .collect::<Vec<_>>()
        });
        let (mean_effective_ops_per_rtt, median_effective_ops_per_rtt) =
            if let Some(values) = effective_ops_per_rtt.as_mut() {
                values.sort_by(f64::total_cmp);
                (
                    Some(values.iter().sum::<f64>() / values.len() as f64),
                    Some(median_f64(values)),
                )
            } else {
                (None, None)
            };

        Self {
            round_count,
            mean_mutation_commits_per_round: total_mutation_ops as f64 / round_count as f64,
            mean_phase_samples_per_round: total_phase_samples as f64 / round_count as f64,
            mean_gate_hold_per_commit: duration_from_nanos_f64(
                total_gate_hold_secs * 1_000_000_000.0 / total_mutation_ops.max(1) as f64,
            ),
            median_round_mean_gate_hold_per_commit: duration_from_nanos_f64(median_f64(
                &round_gate_hold_nanos,
            )),
            gate_hold_commit_share: total_gate_hold_secs / total_commit_secs.max(f64::MIN_POSITIVE),
            mean_round_wall_time: Duration::from_secs_f64(mean_wall_secs),
            mean_effective_ops_per_rtt,
            median_effective_ops_per_rtt,
        }
    }
}

/// Calculates logical mutation completions per nominal injected round trip.
///
/// The proxy delays forwarded request and response chunks independently, so a
/// nominal round trip is twice the configured per-direction delay. Dividing
/// wall time by that interval estimates how many delay opportunities elapsed;
/// dividing completed mutation operations by the result exposes overlap from
/// concurrency, pooling, batching, or protocol pipelining.
fn effective_ops_per_round_trip(round: GateHoldRound, nominal_rtt: Duration) -> f64 {
    round.completed_mutation_ops as f64 * nominal_rtt.as_secs_f64()
        / round.elapsed.as_secs_f64().max(f64::MIN_POSITIVE)
}

fn mixed_load_mutation_ops(tenant_limit: usize, ops_per_tenant: usize) -> BenchResult<u64> {
    let mutations_per_tenant = (0..ops_per_tenant)
        .filter(|step| matches!(step % 4, 2 | 3))
        .count();
    Ok(u64::try_from(
        tenant_limit.saturating_mul(mutations_per_tenant),
    )?)
}

fn format_optional_rate(rate: Option<f64>) -> String {
    rate.map_or_else(|| "—".to_string(), |rate| format!("{rate:.2}"))
}

use super::PpscScenario;

/// Deterministically minimizes a failing scenario without reordering steps.
///
/// The predicate returns `true` while the target failure remains observable.
/// Prefix minimization makes crash/replay failures concise; chunk deletion then
/// removes unrelated setup while retaining the original seed and operation
/// order. A scenario that no longer reproduces is returned unchanged.
pub fn shrink_failing_ppsc_scenario(
    scenario: &PpscScenario,
    mut still_fails: impl FnMut(&PpscScenario) -> bool,
) -> PpscScenario {
    if !still_fails(scenario) {
        return scenario.clone();
    }

    let mut best = scenario.clone();
    for length in 1..best.steps.len() {
        let candidate =
            scenario_with_steps(&best, best.steps.iter().take(length).cloned().collect());
        if still_fails(&candidate) {
            best = candidate;
            break;
        }
    }

    let mut granularity = 2_usize;
    while best.steps.len() > 1 {
        let chunk_size = best.steps.len().div_ceil(granularity);
        let mut reduced = false;
        let mut start = 0_usize;
        while start < best.steps.len() {
            let end = start.saturating_add(chunk_size).min(best.steps.len());
            let retained = best
                .steps
                .iter()
                .enumerate()
                .filter_map(|(index, step)| {
                    (!(start..end).contains(&index)).then_some(step.clone())
                })
                .collect::<Vec<_>>();
            if !retained.is_empty() {
                let candidate = scenario_with_steps(&best, retained);
                if still_fails(&candidate) {
                    best = candidate;
                    granularity = 2;
                    reduced = true;
                    break;
                }
            }
            start = end;
        }
        if reduced {
            continue;
        }
        if granularity >= best.steps.len() {
            break;
        }
        granularity = granularity.saturating_mul(2).min(best.steps.len());
    }
    best
}

fn scenario_with_steps(scenario: &PpscScenario, steps: Vec<super::PpscStep>) -> PpscScenario {
    PpscScenario::new(scenario.name.clone(), scenario.seed, steps)
        .expect("shrinker must retain at least one bounded scenario step")
}

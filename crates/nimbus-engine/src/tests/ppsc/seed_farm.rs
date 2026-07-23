use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;

use futures::FutureExt;
use nimbus_testing::ppsc::{
    PpscBackend, PpscScenario, PpscSeedFarmArtifacts, PpscSeedFarmConfig, PpscSeedFarmSummary,
    audit_ppsc_history, retained_ppsc_scenarios,
};

use super::PpscEngineRunner;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "dedicated deterministic PPSC seed-farm lane owns explicit ranges and artifacts"]
async fn ppsc_seed_farm_executes_selected_redb_scenarios() {
    let config = PpscSeedFarmConfig::from_environment()
        .unwrap_or_else(|error| panic!("PPSC seed-farm configuration is invalid: {error}"));
    assert_eq!(
        config.backend,
        PpscBackend::Redb,
        "the bulk PPSC seed farm owns the redb production-interface lane"
    );
    let artifacts = PpscSeedFarmArtifacts::new(&config.failure_dir)
        .unwrap_or_else(|error| panic!("PPSC seed-farm artifacts could not initialize: {error}"));
    let retained_seeds = retained_ppsc_scenarios()
        .into_iter()
        .map(|scenario| scenario.seed)
        .collect::<BTreeSet<_>>();
    let mut executed = 0;
    let mut passed = 0;
    let mut retained_executed = 0;

    for seed in &config.seeds {
        let scenario = PpscScenario::seeded(*seed, config.step_count)
            .unwrap_or_else(|error| panic!("PPSC seed {seed} could not generate: {error}"));
        let pending = artifacts
            .begin_seed(&config, &scenario)
            .unwrap_or_else(|error| {
                panic!("PPSC seed {seed} interruption marker could not publish: {error}")
            });
        println!(
            "PPSC_SEED_FARM seed={} backend={} shard={}/{} replay={}",
            seed,
            config.backend.as_str(),
            config.shard_index + 1,
            config.shard_count,
            scenario.replay_command(config.backend)
        );
        let result = AssertUnwindSafe(async {
            let history = PpscEngineRunner::new_embedded(config.backend, &scenario)
                .await
                .run(scenario.clone())
                .await;
            audit_ppsc_history(&history)
                .unwrap_or_else(|error| panic!("redb seed-farm audit: {error}"));
        })
        .catch_unwind()
        .await;
        executed += 1;
        retained_executed += usize::from(retained_seeds.contains(seed));
        match result {
            Ok(()) => {
                artifacts
                    .mark_seed_passed(&pending)
                    .unwrap_or_else(|error| {
                        panic!("PPSC seed {seed} completion marker failed: {error}")
                    });
                passed += 1;
            }
            Err(payload) => {
                let message = panic_payload_message(payload.as_ref());
                let failure_path = artifacts
                    .mark_seed_failed(&config, &scenario, &pending, &message)
                    .unwrap_or_else(|artifact_error| {
                        panic!(
                            "PPSC seed {seed} failed ({message}) and its failure bundle could not publish: {artifact_error}"
                        )
                    });
                let summary = PpscSeedFarmSummary {
                    format_version: 1,
                    revision: config.revision.clone(),
                    backend: config.backend,
                    seed_start: config.seed_start,
                    seed_count: config.seed_count,
                    shard_index: config.shard_index,
                    shard_count: config.shard_count,
                    selected: config.selected_count(),
                    executed,
                    passed,
                    failed: 1,
                    retained: retained_executed,
                };
                artifacts.write_summary(&summary).unwrap_or_else(|error| {
                    panic!("PPSC failure summary could not publish: {error}")
                });
                panic!(
                    "PPSC seed {seed} failed: {message}; failure bundle: {}; replay: {}",
                    failure_path.display(),
                    scenario.replay_command(config.backend)
                );
            }
        }
    }

    let summary = PpscSeedFarmSummary {
        format_version: 1,
        revision: config.revision.clone(),
        backend: config.backend,
        seed_start: config.seed_start,
        seed_count: config.seed_count,
        shard_index: config.shard_index,
        shard_count: config.shard_count,
        selected: config.selected_count(),
        executed,
        passed,
        failed: 0,
        retained: retained_executed,
    };
    let summary_path = artifacts
        .write_summary(&summary)
        .unwrap_or_else(|error| panic!("PPSC seed-farm summary could not publish: {error}"));
    assert!(
        summary.is_complete_success(),
        "PPSC seed farm must fail closed on zero or partial execution: {summary:?}"
    );
    println!(
        "PPSC_SEED_FARM_SUMMARY executed={} passed={} failed={} retained={} selected={} artifact={}",
        summary.executed,
        summary.passed,
        summary.failed,
        summary.retained,
        summary.selected,
        summary_path.display()
    );
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    "PPSC seed panicked with a non-string payload".to_string()
}

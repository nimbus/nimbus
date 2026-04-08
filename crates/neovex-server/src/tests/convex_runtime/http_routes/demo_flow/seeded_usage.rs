use super::*;

mod model;
mod scenarios;
mod support;

use self::model::{seeded_convex_demo_faulted_overlap_step, seeded_convex_demo_operation_count};
use self::scenarios::assert_seeded_convex_demo_usage_scenario_matches_model;

async fn run_seeded_usage_verification_corpus(
    mode: VerificationHarnessMode,
    test_name: &str,
    faulted: bool,
) {
    for case in selected_generated_task_history_seed_corpus(mode)
        .expect("verification corpus should resolve")
    {
        let operation_count = seeded_convex_demo_operation_count(case.step_count);
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            operation_count,
            Some(case),
            test_name,
            faulted.then(|| seeded_convex_demo_faulted_overlap_step(operation_count)),
        )
        .await;
    }
}

#[tokio::test]
async fn convex_http_demo_seeded_usage_scenario_matches_model() {
    assert_seeded_convex_demo_usage_scenario_matches_model(
        17,
        seeded_convex_demo_operation_count(24),
        None,
        "convex_http_demo_seeded_usage_scenario_matches_model",
        None,
    )
    .await;
}

#[tokio::test]
async fn convex_http_demo_faulted_seeded_usage_scenario_matches_model() {
    assert_seeded_convex_demo_usage_scenario_matches_model(
        23,
        seeded_convex_demo_operation_count(24),
        None,
        "convex_http_demo_faulted_seeded_usage_scenario_matches_model",
        Some(seeded_convex_demo_faulted_overlap_step(
            seeded_convex_demo_operation_count(24),
        )),
    )
    .await;
}

#[tokio::test]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model_on_convex_demo_surface()
 {
    run_seeded_usage_verification_corpus(
        VerificationHarnessMode::PullRequest,
        "verification_harness_pr_generated_history_seed_corpus_matches_model",
        false,
    )
    .await;
}

#[tokio::test]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model_on_convex_demo_surface()
 {
    run_seeded_usage_verification_corpus(
        VerificationHarnessMode::Nightly,
        "verification_harness_nightly_generated_history_seed_corpus_matches_model",
        false,
    )
    .await;
}

#[tokio::test]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface()
 {
    run_seeded_usage_verification_corpus(
        VerificationHarnessMode::PullRequest,
        "verification_harness_pr_generated_history_seed_corpus_matches_model",
        true,
    )
    .await;
}

#[tokio::test]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface()
 {
    run_seeded_usage_verification_corpus(
        VerificationHarnessMode::Nightly,
        "verification_harness_nightly_generated_history_seed_corpus_matches_model",
        true,
    )
    .await;
}

use std::panic::{self, AssertUnwindSafe};

use super::*;
use crate::test_support::RuntimeReproCase;

const VERIFICATION_CASE_FILTER_ENV: &str = "NIMBUS_VERIFY_CASE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeVerificationHarnessMode {
    PullRequest,
    Nightly,
}

impl RuntimeVerificationHarnessMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::PullRequest => "pr",
            Self::Nightly => "nightly",
        }
    }
}

#[derive(Clone, Copy)]
struct RuntimeVerificationHarnessCase {
    metadata: RuntimeReproCase,
    runner: fn(),
}

impl RuntimeVerificationHarnessCase {
    const fn new(metadata: RuntimeReproCase, runner: fn()) -> Self {
        Self { metadata, runner }
    }

    fn id(self) -> &'static str {
        self.metadata.id()
    }

    fn repro_command(self, mode: RuntimeVerificationHarnessMode) -> String {
        format!(
            "bash scripts/verification-harness.sh repro runtime {} {}",
            mode.as_str(),
            self.id()
        )
    }

    fn failure_context(self, mode: RuntimeVerificationHarnessMode, invariant: &str) -> String {
        self.metadata
            .failure_context_with_repro(invariant, &self.repro_command(mode))
    }
}

const PR_RUNTIME_VERIFICATION_CASES: [RuntimeVerificationHarnessCase; 5] = [
    RuntimeVerificationHarnessCase::new(
        super::bundle_integrity::BUNDLE_INTEGRITY_RECHECK_CASE,
        run_bundle_integrity_recheck_after_prior_success_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::bundle_integrity::PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE.metadata(),
        run_product_default_bundle_queue_health_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::cooperative::CONCURRENT_DISPATCH_CASE.metadata(),
        run_cooperative_concurrent_dispatch_case,
    ),
    RuntimeVerificationHarnessCase::new(
        crate::executor::tests::queue_fairness::TENANT_QUEUE_LIMIT_REJECTION_CASE,
        run_tenant_queue_limit_rejection_case,
    ),
    RuntimeVerificationHarnessCase::new(
        crate::executor::tests::queue_fairness::TENANT_FAIRNESS_NO_STARVATION_CASE,
        run_tenant_fairness_no_starvation_case,
    ),
];

const NIGHTLY_RUNTIME_VERIFICATION_CASES: [RuntimeVerificationHarnessCase; 11] = [
    RuntimeVerificationHarnessCase::new(
        super::bundle_integrity::BUNDLE_INTEGRITY_RECHECK_CASE,
        run_bundle_integrity_recheck_after_prior_success_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::bundle_integrity::PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE.metadata(),
        run_product_default_bundle_queue_health_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::cooperative::PARK_AND_RESUME_CASE.metadata(),
        run_cooperative_park_and_resume_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::cooperative::IMMEDIATE_ASYNC_CASE.metadata(),
        run_cooperative_immediate_async_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::cooperative::WARM_POOL_TWO_CYCLE_CASE.metadata(),
        run_warm_pool_two_cycle_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::cooperative::CONCURRENT_DISPATCH_CASE.metadata(),
        run_cooperative_concurrent_dispatch_case,
    ),
    RuntimeVerificationHarnessCase::new(
        crate::executor::tests::queue_fairness::TENANT_QUEUE_LIMIT_REJECTION_CASE,
        run_tenant_queue_limit_rejection_case,
    ),
    RuntimeVerificationHarnessCase::new(
        crate::executor::tests::queue_fairness::TENANT_FAIRNESS_NO_STARVATION_CASE,
        run_tenant_fairness_no_starvation_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::locker::LOCKER_SNAPSHOT_CASE.metadata(),
        run_locker_snapshot_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::locker::LOCKER_INTERLEAVE_CASE.metadata(),
        run_locker_interleave_case,
    ),
    RuntimeVerificationHarnessCase::new(
        super::warm_pool::CROSS_TENANT_WARM_POOL_CASE.metadata(),
        run_warm_pool_cross_tenant_isolation_case,
    ),
];

fn run_bundle_integrity_recheck_after_prior_success_case() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            super::bundle_integrity::runtime_bundle_rechecks_integrity_after_prior_success_inner(),
        );
}

fn run_product_default_bundle_queue_health_case() {
    run_v8_sensitive_runtime_test_in_subprocess(
        super::bundle_integrity::PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE,
    );
}

fn run_cooperative_park_and_resume_case() {
    run_v8_sensitive_runtime_test_in_subprocess(super::cooperative::PARK_AND_RESUME_CASE);
}

fn run_cooperative_immediate_async_case() {
    run_v8_sensitive_runtime_test_in_subprocess(super::cooperative::IMMEDIATE_ASYNC_CASE);
}

fn run_warm_pool_two_cycle_case() {
    run_v8_sensitive_runtime_test_in_subprocess(super::cooperative::WARM_POOL_TWO_CYCLE_CASE);
}

fn run_cooperative_concurrent_dispatch_case() {
    run_v8_sensitive_runtime_test_in_subprocess(super::cooperative::CONCURRENT_DISPATCH_CASE);
}

fn run_tenant_queue_limit_rejection_case() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(crate::executor::tests::queue_fairness::tenant_queue_limit_rejections_record_metrics_inner());
}

fn run_tenant_fairness_no_starvation_case() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            crate::executor::tests::queue_fairness::tenant_fairness_prevents_one_tenant_from_starving_another_inner(),
        );
}

fn run_locker_snapshot_case() {
    run_v8_sensitive_runtime_test_in_subprocess(super::locker::LOCKER_SNAPSHOT_CASE);
}

fn run_locker_interleave_case() {
    run_v8_sensitive_runtime_test_in_subprocess(super::locker::LOCKER_INTERLEAVE_CASE);
}

fn run_warm_pool_cross_tenant_isolation_case() {
    run_v8_sensitive_runtime_test_in_subprocess(super::warm_pool::CROSS_TENANT_WARM_POOL_CASE);
}

fn runtime_verification_corpus(
    mode: RuntimeVerificationHarnessMode,
) -> &'static [RuntimeVerificationHarnessCase] {
    match mode {
        RuntimeVerificationHarnessMode::PullRequest => &PR_RUNTIME_VERIFICATION_CASES,
        RuntimeVerificationHarnessMode::Nightly => &NIGHTLY_RUNTIME_VERIFICATION_CASES,
    }
}

fn selected_runtime_verification_cases(
    mode: RuntimeVerificationHarnessMode,
) -> std::result::Result<Vec<RuntimeVerificationHarnessCase>, String> {
    let filter = match std::env::var(VERIFICATION_CASE_FILTER_ENV) {
        Ok(filter) => Some(filter),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(format!(
                "failed to read {VERIFICATION_CASE_FILTER_ENV}: {error}"
            ));
        }
    };

    let cases = runtime_verification_corpus(mode);
    match filter {
        Some(filter) => {
            let selected = cases
                .iter()
                .copied()
                .filter(|case| case.id() == filter)
                .collect::<Vec<_>>();
            if selected.is_empty() {
                return Err(format!(
                    "unknown runtime verification harness case `{filter}`"
                ));
            }
            Ok(selected)
        }
        None => Ok(cases.to_vec()),
    }
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_string(),
            Err(_) => "non-string panic payload".to_string(),
        },
    }
}

fn run_runtime_verification_corpus(mode: RuntimeVerificationHarnessMode, test_name: &str) {
    let cases = selected_runtime_verification_cases(mode).unwrap_or_else(|error| {
        panic!("{test_name}: {error}");
    });

    for case in cases {
        eprintln!("running {}", case.metadata.describe());
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| (case.runner)()));
        if let Err(payload) = outcome {
            panic!(
                "{}. Original panic: {}",
                case.failure_context(mode, "runtime verification harness case failed"),
                panic_payload_to_string(payload)
            );
        }
    }
}

#[test]
#[ignore = "verification harness PR corpus runs in dedicated harness lanes"]
fn verification_harness_pr_runtime_liveness_and_integrity_cases() {
    run_runtime_verification_corpus(
        RuntimeVerificationHarnessMode::PullRequest,
        "verification_harness_pr_runtime_liveness_and_integrity_cases",
    );
}

#[test]
#[ignore = "verification harness nightly corpus runs in dedicated harness lanes"]
fn verification_harness_nightly_runtime_liveness_and_integrity_cases() {
    run_runtime_verification_corpus(
        RuntimeVerificationHarnessMode::Nightly,
        "verification_harness_nightly_runtime_liveness_and_integrity_cases",
    );
}

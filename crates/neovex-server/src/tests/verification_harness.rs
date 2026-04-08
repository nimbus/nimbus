use std::panic::{self, AssertUnwindSafe};

use super::*;

const VERIFICATION_CASE_FILTER_ENV: &str = "NEOVEX_VERIFY_CASE";

#[derive(Clone, Copy)]
struct ServerVerificationHarnessCase {
    metadata: DeterministicTestCase,
    runner: fn(),
}

impl ServerVerificationHarnessCase {
    const fn new(metadata: DeterministicTestCase, runner: fn()) -> Self {
        Self { metadata, runner }
    }

    fn id(self) -> &'static str {
        self.metadata.id()
    }

    fn repro_command(self, mode: VerificationHarnessMode) -> String {
        format!(
            "bash scripts/verification-harness.sh repro server {} {}",
            mode.as_str(),
            self.id()
        )
    }

    fn failure_context(self, mode: VerificationHarnessMode, invariant: &str) -> String {
        self.metadata
            .failure_context_with_repro(invariant, &self.repro_command(mode))
    }
}

const PR_SERVER_VERIFICATION_CASES: [ServerVerificationHarnessCase; 5] = [
    ServerVerificationHarnessCase::new(
        super::auth::websocket_auth::WEBSOCKET_DISCONNECT_CLEANUP_CASE,
        run_websocket_disconnect_cleanup_case,
    ),
    ServerVerificationHarnessCase::new(
        super::auth::websocket_auth::WEBSOCKET_AUTH_CHANGE_RESUBSCRIBE_CASE,
        run_websocket_auth_change_resubscribe_case,
    ),
    ServerVerificationHarnessCase::new(
        super::scheduling::cron_and_history::SCHEDULED_JOB_HISTORY_FAILURE_CASE,
        run_scheduled_job_history_failure_case,
    ),
    ServerVerificationHarnessCase::new(
        super::convex_runtime::fairness::FAIRNESS_HTTP_REJECTION_CASE,
        run_fairness_http_rejection_case,
    ),
    ServerVerificationHarnessCase::new(
        super::convex_runtime::fairness::FAIRNESS_WEBSOCKET_REJECTION_CASE,
        run_fairness_websocket_rejection_case,
    ),
];

const NIGHTLY_SERVER_VERIFICATION_CASES: [ServerVerificationHarnessCase; 5] = [
    ServerVerificationHarnessCase::new(
        super::auth::websocket_auth::WEBSOCKET_DISCONNECT_CLEANUP_CASE,
        run_websocket_disconnect_cleanup_case,
    ),
    ServerVerificationHarnessCase::new(
        super::auth::websocket_auth::WEBSOCKET_AUTH_CHANGE_RESUBSCRIBE_CASE,
        run_websocket_auth_change_resubscribe_case,
    ),
    ServerVerificationHarnessCase::new(
        super::scheduling::cron_and_history::SCHEDULED_JOB_HISTORY_FAILURE_CASE,
        run_scheduled_job_history_failure_case,
    ),
    ServerVerificationHarnessCase::new(
        super::convex_runtime::fairness::FAIRNESS_HTTP_REJECTION_CASE,
        run_fairness_http_rejection_case,
    ),
    ServerVerificationHarnessCase::new(
        super::convex_runtime::fairness::FAIRNESS_WEBSOCKET_REJECTION_CASE,
        run_fairness_websocket_rejection_case,
    ),
];

fn run_websocket_disconnect_cleanup_case() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            super::auth::websocket_auth::convex_websocket_disconnect_releases_runtime_subscription_children_inner(),
        );
}

fn run_websocket_auth_change_resubscribe_case() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            super::auth::websocket_auth::convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed_inner(),
        );
}

fn run_scheduled_job_history_failure_case() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            super::scheduling::cron_and_history::scheduled_job_history_endpoint_reports_failures_inner(),
        );
}

fn run_fairness_http_rejection_case() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(super::convex_runtime::fairness::convex_runtime_http_rejections_return_too_many_requests_inner());
}

fn run_fairness_websocket_rejection_case() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            super::convex_runtime::fairness::convex_runtime_websocket_bootstrap_rejections_send_error_frames_inner(),
        );
}

fn server_verification_corpus(
    mode: VerificationHarnessMode,
) -> &'static [ServerVerificationHarnessCase] {
    match mode {
        VerificationHarnessMode::PullRequest => &PR_SERVER_VERIFICATION_CASES,
        VerificationHarnessMode::Nightly => &NIGHTLY_SERVER_VERIFICATION_CASES,
    }
}

fn selected_server_verification_cases(
    mode: VerificationHarnessMode,
) -> std::result::Result<Vec<ServerVerificationHarnessCase>, String> {
    let filter = match std::env::var(VERIFICATION_CASE_FILTER_ENV) {
        Ok(filter) => Some(filter),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => {
            return Err(format!(
                "failed to read {VERIFICATION_CASE_FILTER_ENV}: {error}"
            ));
        }
    };

    let cases = server_verification_corpus(mode);
    match filter {
        Some(filter) => {
            let selected = cases
                .iter()
                .copied()
                .filter(|case| case.id() == filter)
                .collect::<Vec<_>>();
            if selected.is_empty() {
                return Err(format!(
                    "unknown server verification harness case `{filter}`"
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

fn run_server_verification_corpus(mode: VerificationHarnessMode, test_name: &str) {
    let cases = selected_server_verification_cases(mode).unwrap_or_else(|error| {
        panic!("{test_name}: {error}");
    });

    for case in cases {
        eprintln!("running {}", case.metadata.describe());
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| (case.runner)()));
        if let Err(payload) = outcome {
            panic!(
                "{}. Original panic: {}",
                case.failure_context(mode, "server verification harness case failed"),
                panic_payload_to_string(payload)
            );
        }
    }
}

#[test]
#[ignore = "verification harness PR corpus runs in dedicated harness lanes"]
fn verification_harness_pr_transport_liveness_campaigns() {
    run_server_verification_corpus(
        VerificationHarnessMode::PullRequest,
        "verification_harness_pr_transport_liveness_campaigns",
    );
}

#[test]
#[ignore = "verification harness nightly corpus runs in dedicated harness lanes"]
fn verification_harness_nightly_transport_liveness_campaigns() {
    run_server_verification_corpus(
        VerificationHarnessMode::Nightly,
        "verification_harness_nightly_transport_liveness_campaigns",
    );
}

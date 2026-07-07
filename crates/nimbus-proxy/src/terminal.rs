use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::decision_log::{DecisionLogger, DurableDecisionSink, EgressDecisionLog};
use crate::phase::{EgressProxyRequestPhase, RequestPhaseRecorder};
use crate::policy_state::PolicyGeneration;
use crate::request::ParsedProxyRequest;
use crate::response::{HttpProxyResponse, write_http_response_async};

const AUDIT_UNHEALTHY_FAIL_CLOSED_REASON: &str =
    "egress proxy decision audit is unhealthy; failing closed until restart";

#[derive(Clone)]
struct AbortTerminalSlot {
    state: Arc<Mutex<AbortTerminalState>>,
    response_started: Arc<AtomicBool>,
}

#[derive(Clone)]
pub(crate) struct ResponseStartedSignal {
    slot: AbortTerminalSlot,
}

enum AbortTerminalState {
    Armed(EgressDecisionLog),
    Disarmed,
}

impl AbortTerminalSlot {
    fn new(decision_log: EgressDecisionLog) -> Self {
        Self {
            state: Arc::new(Mutex::new(AbortTerminalState::Armed(decision_log))),
            response_started: Arc::new(AtomicBool::new(false)),
        }
    }

    fn response_started_signal(&self) -> ResponseStartedSignal {
        ResponseStartedSignal { slot: self.clone() }
    }

    fn disarm(&self) {
        let mut slot = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = AbortTerminalState::Disarmed;
    }

    fn take(&self) -> Option<EgressDecisionLog> {
        let mut slot = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match std::mem::replace(&mut *slot, AbortTerminalState::Disarmed) {
            AbortTerminalState::Armed(decision_log) => Some(decision_log),
            AbortTerminalState::Disarmed => None,
        }
    }
}

impl ResponseStartedSignal {
    /// Switch the abort fallback from the pre-response synthetic deny to the
    /// supplied after-response allow. This is a no-op after terminal completion
    /// has disarmed the request.
    pub(crate) fn mark_response_started(&self, decision_log: EgressDecisionLog) -> bool {
        let mut slot = self
            .slot
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &mut *slot {
            AbortTerminalState::Armed(existing) => {
                *existing = decision_log;
                self.slot.response_started.store(true, Ordering::SeqCst);
                true
            }
            AbortTerminalState::Disarmed => false,
        }
    }

    pub(crate) fn response_started(&self) -> bool {
        self.slot.response_started.load(Ordering::SeqCst)
    }

    pub(crate) fn disarm(&self) {
        self.slot.disarm();
    }
}

pub(crate) struct RequestIdGenerator {
    prefix: String,
    counter: AtomicU64,
}

impl RequestIdGenerator {
    pub(crate) fn new() -> Self {
        let random_state = RandomState::new();
        let mut hasher = random_state.build_hasher();
        hasher.write_u32(std::process::id());
        hasher.write_u64(0x6e696d6275735f70);
        Self {
            prefix: format!("{:016x}", hasher.finish()),
            counter: AtomicU64::new(0),
        }
    }

    pub(crate) fn next(&self) -> String {
        let sequence = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("{}-{sequence:016x}", self.prefix)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TerminalSinks<'a> {
    durable_decision_sink: &'a DurableDecisionSink,
    audit_healthy: &'a AtomicBool,
    decision_logger: &'a DecisionLogger,
}

impl<'a> TerminalSinks<'a> {
    pub(crate) fn new(
        durable_decision_sink: &'a DurableDecisionSink,
        audit_healthy: &'a AtomicBool,
        decision_logger: &'a DecisionLogger,
    ) -> Self {
        Self {
            durable_decision_sink,
            audit_healthy,
            decision_logger,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ParsedRequestLogContext<'a> {
    pub(crate) request_id: &'a str,
    pub(crate) parsed: &'a ParsedProxyRequest,
}

/// Fires only if a connection task is dropped (PEP shutdown aborts its
/// `JoinSet`) or exits abnormally after parse but before any terminal event:
/// the exactly-one audit invariant must survive cancellation.
pub(crate) struct AbortTerminalGuard {
    phase_recorder: RequestPhaseRecorder,
    decision_logger: DecisionLogger,
    durable_decision_sink: DurableDecisionSink,
    audit_healthy: Arc<AtomicBool>,
    slot: AbortTerminalSlot,
}

impl AbortTerminalGuard {
    pub(crate) fn new(
        phase_recorder: RequestPhaseRecorder,
        decision_logger: DecisionLogger,
        durable_decision_sink: DurableDecisionSink,
        audit_healthy: Arc<AtomicBool>,
        decision_log: EgressDecisionLog,
    ) -> Self {
        Self {
            phase_recorder,
            decision_logger,
            durable_decision_sink,
            audit_healthy,
            slot: AbortTerminalSlot::new(decision_log),
        }
    }

    pub(crate) fn response_started_signal(&self) -> ResponseStartedSignal {
        self.slot.response_started_signal()
    }
}

impl Drop for AbortTerminalGuard {
    fn drop(&mut self) {
        let decision_log = self.slot.take();
        let Some(decision_log) = decision_log else {
            return;
        };
        let terminal_recorded = self.phase_recorder.terminal_recorded();
        if !terminal_recorded && !self.phase_recorder.durable_terminal_recorded() {
            let _ = record_durable_decision(
                &self.phase_recorder,
                &self.durable_decision_sink,
                self.audit_healthy.as_ref(),
                &decision_log,
            );
        }
        if !terminal_recorded {
            emit_terminal_log(&self.phase_recorder, &self.decision_logger, decision_log);
        }
    }
}

/// Emits the terminal event for a request the strict parser rejected before an
/// authority existed, then writes the fail-closed response.
pub(crate) async fn malformed_terminal(
    client: &mut TcpStream,
    phase_recorder: &RequestPhaseRecorder,
    terminal_sinks: TerminalSinks<'_>,
    request_id: &str,
    response: HttpProxyResponse,
) -> io::Result<()> {
    let decision_log = EgressDecisionLog::malformed(request_id, response.body().to_owned());
    if record_durable_decision(
        phase_recorder,
        terminal_sinks.durable_decision_sink,
        terminal_sinks.audit_healthy,
        &decision_log,
    )
    .is_err()
    {
        emit_terminal_log(phase_recorder, terminal_sinks.decision_logger, decision_log);
        return close_client_without_response(client).await;
    }
    emit_terminal_log(phase_recorder, terminal_sinks.decision_logger, decision_log);
    write_http_response_async(client, response).await
}

pub(crate) async fn audit_unhealthy_terminal(
    client: &mut TcpStream,
    phase_recorder: &RequestPhaseRecorder,
    terminal_sinks: TerminalSinks<'_>,
    request: ParsedRequestLogContext<'_>,
) -> io::Result<()> {
    let decision_log = EgressDecisionLog::denied(
        request.request_id,
        request.parsed,
        AUDIT_UNHEALTHY_FAIL_CLOSED_REASON.to_owned(),
        None,
    );
    let _ = record_durable_decision(
        phase_recorder,
        terminal_sinks.durable_decision_sink,
        terminal_sinks.audit_healthy,
        &decision_log,
    );
    emit_terminal_log(phase_recorder, terminal_sinks.decision_logger, decision_log);
    close_client_without_response(client).await
}

pub(crate) async fn deny_terminal(
    client: &mut TcpStream,
    phase_recorder: &RequestPhaseRecorder,
    terminal_sinks: TerminalSinks<'_>,
    request: ParsedRequestLogContext<'_>,
    matched_rule: Option<String>,
    policy_generation: Option<PolicyGeneration>,
    response: HttpProxyResponse,
) -> io::Result<()> {
    let mut decision_log = EgressDecisionLog::denied(
        request.request_id,
        request.parsed,
        response.body().to_owned(),
        matched_rule,
    );
    if let Some(policy_generation) = policy_generation {
        decision_log = decision_log.with_policy_generation(policy_generation);
    }
    if record_durable_decision(
        phase_recorder,
        terminal_sinks.durable_decision_sink,
        terminal_sinks.audit_healthy,
        &decision_log,
    )
    .is_err()
    {
        emit_terminal_log(phase_recorder, terminal_sinks.decision_logger, decision_log);
        return close_client_without_response(client).await;
    }
    emit_terminal_log(phase_recorder, terminal_sinks.decision_logger, decision_log);
    write_http_response_async(client, response).await
}

pub(crate) async fn audit_failure_terminal(
    client: &mut TcpStream,
    phase_recorder: &RequestPhaseRecorder,
    terminal_sinks: TerminalSinks<'_>,
    request: ParsedRequestLogContext<'_>,
    matched_rule: Option<String>,
    policy_generation: PolicyGeneration,
    error: io::Error,
) -> io::Result<()> {
    let decision_log = EgressDecisionLog::denied(
        request.request_id,
        request.parsed,
        durable_audit_failure_reason(&error),
        matched_rule,
    )
    .with_policy_generation(policy_generation);
    emit_terminal_log(phase_recorder, terminal_sinks.decision_logger, decision_log);
    close_client_without_response(client).await
}

pub(crate) fn emit_terminal_log(
    phase_recorder: &RequestPhaseRecorder,
    decision_logger: &DecisionLogger,
    decision_log: EgressDecisionLog,
) {
    phase_recorder.record(EgressProxyRequestPhase::TerminalLog);
    decision_logger(decision_log);
}

pub(crate) fn record_durable_decision(
    phase_recorder: &RequestPhaseRecorder,
    durable_decision_sink: &DurableDecisionSink,
    audit_healthy: &AtomicBool,
    decision_log: &EgressDecisionLog,
) -> io::Result<()> {
    match durable_decision_sink(decision_log) {
        Ok(()) => {
            phase_recorder.mark_durable_any_recorded();
            if decision_log.record_kind().is_terminal() {
                phase_recorder.mark_durable_terminal_recorded();
            }
            Ok(())
        }
        Err(error) => {
            audit_healthy.store(false, Ordering::SeqCst);
            Err(error)
        }
    }
}

fn durable_audit_failure_reason(error: &io::Error) -> String {
    format!("egress proxy decision audit append failed: {error}")
}

pub(crate) async fn close_client_without_response(client: &mut TcpStream) -> io::Result<()> {
    let _ = client.shutdown().await;
    Ok(())
}

pub(crate) async fn upstream_error_terminal(
    client: &mut TcpStream,
    phase_recorder: &RequestPhaseRecorder,
    terminal_sinks: TerminalSinks<'_>,
    request_id: &str,
    parsed: &ParsedProxyRequest,
    matched_rule: Option<String>,
    policy_generation: PolicyGeneration,
) -> io::Result<()> {
    let response = HttpProxyResponse::bad_gateway("egress proxy failed to dial the upstream");
    let decision_log =
        EgressDecisionLog::denied(request_id, parsed, response.body().to_owned(), matched_rule)
            .with_policy_generation(policy_generation);
    if record_durable_decision(
        phase_recorder,
        terminal_sinks.durable_decision_sink,
        terminal_sinks.audit_healthy,
        &decision_log,
    )
    .is_err()
    {
        emit_terminal_log(phase_recorder, terminal_sinks.decision_logger, decision_log);
        return close_client_without_response(client).await;
    }
    emit_terminal_log(phase_recorder, terminal_sinks.decision_logger, decision_log);
    write_http_response_async(client, response).await
}

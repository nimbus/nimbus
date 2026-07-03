use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderValue, Version};
use pingora_core::upstreams::peer::HttpPeer;
use pingora_core::{Error, InvalidHTTPHeader};
use pingora_http::RequestHeader;
use pingora_proxy::{FailToProxy, ProxyHttp, Session};

use crate::decision_log::{DecisionLogger, EgressDecisionLog};
use crate::phase::{EgressProxyRequestPhase, RequestPhaseRecorder};
use crate::pingora_identity::PingoraPeerPlan;
use crate::policy_state::PolicyGeneration;
use crate::request::{ParsedProxyRequest, ProxyRequestMode};

const UPSTREAM_FAILURE_BODY: &str = "egress proxy failed to dial the upstream";

#[derive(Clone)]
pub(crate) struct ForwardRequestPlan {
    pub(crate) parsed: ParsedProxyRequest,
    pub(crate) downstream_target: String,
    pub(crate) peer_plan: PingoraPeerPlan,
    pub(crate) prepared_header_lines: Vec<String>,
    pub(crate) origin_form: String,
    pub(crate) allowed_decision_log: EgressDecisionLog,
    pub(crate) matched_rule: Option<String>,
    pub(crate) policy_generation: PolicyGeneration,
    pub(crate) connect_timeout: Duration,
    pub(crate) io_timeout: Duration,
    pub(crate) phase_recorder: RequestPhaseRecorder,
    pub(crate) decision_logger: DecisionLogger,
}

pub(crate) struct NimbusForwardApp {
    plan: ForwardRequestPlan,
}

pub(crate) struct NimbusForwardContext {
    terminal_logged: bool,
}

impl NimbusForwardApp {
    pub(crate) fn new(plan: ForwardRequestPlan) -> Self {
        Self { plan }
    }

    fn emit_denied_terminal(&self, ctx: &mut NimbusForwardContext, reason: String) {
        if ctx.terminal_logged {
            return;
        }
        ctx.terminal_logged = true;
        let decision_log =
            EgressDecisionLog::denied(&self.plan.parsed, reason, self.plan.matched_rule.clone())
                .with_policy_generation(self.plan.policy_generation);
        self.emit_terminal(decision_log);
    }

    fn emit_allowed_terminal(&self, ctx: &mut NimbusForwardContext) {
        if ctx.terminal_logged {
            return;
        }
        ctx.terminal_logged = true;
        self.emit_terminal(self.plan.allowed_decision_log.clone());
    }

    fn emit_terminal(&self, decision_log: EgressDecisionLog) {
        self.plan
            .phase_recorder
            .record(EgressProxyRequestPhase::TerminalLog);
        (self.plan.decision_logger)(decision_log);
    }
}

#[async_trait]
impl ProxyHttp for NimbusForwardApp {
    type CTX = NimbusForwardContext;

    fn new_ctx(&self) -> Self::CTX {
        NimbusForwardContext {
            terminal_logged: false,
        }
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora_core::Result<bool>
    where
        Self::CTX: Send + Sync,
    {
        session.set_keepalive(None);
        session.set_read_timeout(Some(self.plan.io_timeout));
        session.set_write_timeout(Some(self.plan.io_timeout));
        let request = session.req_header();
        let method_matches = request.method.as_str() == self.plan.parsed.method;
        let uri = request.uri.to_string();
        let uri_matches = uri == self.plan.downstream_target;
        if !method_matches || !uri_matches {
            let body = Bytes::from_static(b"egress proxy internal request mismatch");
            let _ = session.respond_error_with_body(400, body).await;
            self.emit_denied_terminal(ctx, "egress proxy internal request mismatch".to_owned());
            return Ok(true);
        }
        self.plan
            .phase_recorder
            .record(EgressProxyRequestPhase::Forward);
        Ok(false)
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> pingora_core::Result<Box<HttpPeer>> {
        let mut peer = self.plan.peer_plan.to_pingora_peer();
        peer.options.connection_timeout = Some(self.plan.connect_timeout);
        peer.options.read_timeout = Some(self.plan.io_timeout);
        peer.options.write_timeout = Some(self.plan.io_timeout);
        // Reuse is disabled until the connection-broker plan owns
        // collision-safe pooling across the full Nimbus pool key.
        peer.options.idle_timeout = Some(Duration::ZERO);
        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora_core::Result<()>
    where
        Self::CTX: Send + Sync,
    {
        let mut rewritten = RequestHeader::build(
            self.plan.parsed.method.as_str(),
            self.plan.origin_form.as_bytes(),
            Some(self.plan.prepared_header_lines.len() + 2),
        )?;
        rewritten.set_version(match self.plan.parsed.version.as_str() {
            "HTTP/1.0" => Version::HTTP_10,
            "HTTP/1.1" => Version::HTTP_11,
            _ => {
                return Err(Error::explain(
                    InvalidHTTPHeader,
                    format!(
                        "unsupported upstream HTTP version {}",
                        self.plan.parsed.version
                    ),
                ));
            }
        });
        rewritten.insert_header(
            "Host",
            format!(
                "{}:{}",
                self.plan.parsed.upstream_host, self.plan.parsed.upstream_port
            ),
        )?;
        for line in &self.plan.prepared_header_lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let name = name.trim();
            if name.eq_ignore_ascii_case("host")
                || name.eq_ignore_ascii_case("connection")
                || name.eq_ignore_ascii_case("proxy-connection")
            {
                continue;
            }
            let value = HeaderValue::from_str(value.trim()).map_err(|error| {
                Error::because(
                    InvalidHTTPHeader,
                    "invalid upstream HTTP header value",
                    error,
                )
            })?;
            rewritten.append_header(name.to_owned(), value)?;
        }
        rewritten.insert_header("Connection", "close")?;
        *upstream_request = rewritten;
        Ok(())
    }

    fn fail_to_connect(
        &self,
        _session: &mut Session,
        _peer: &HttpPeer,
        _ctx: &mut Self::CTX,
        mut error: Box<Error>,
    ) -> Box<Error> {
        error.set_retry(false);
        error
    }

    async fn fail_to_proxy(
        &self,
        session: &mut Session,
        _error: &Error,
        ctx: &mut Self::CTX,
    ) -> FailToProxy
    where
        Self::CTX: Send + Sync,
    {
        if session.response_written().is_none() {
            let _ = session
                .respond_error_with_body(502, Bytes::from_static(UPSTREAM_FAILURE_BODY.as_bytes()))
                .await;
        }
        self.emit_denied_terminal(ctx, UPSTREAM_FAILURE_BODY.to_owned());
        FailToProxy {
            error_code: 502,
            can_reuse_downstream: false,
        }
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        _upstream_response: &mut pingora_http::ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora_core::Result<()>
    where
        Self::CTX: Send + Sync,
    {
        self.plan
            .phase_recorder
            .record(EgressProxyRequestPhase::ResponseFilters);
        Ok(())
    }

    async fn logging(&self, _session: &mut Session, _error: Option<&Error>, ctx: &mut Self::CTX)
    where
        Self::CTX: Send + Sync,
    {
        self.emit_allowed_terminal(ctx);
    }
}

pub(crate) fn downstream_target(parsed: &ParsedProxyRequest) -> String {
    match &parsed.mode {
        ProxyRequestMode::ForwardHttp { origin_form } => origin_form.clone(),
        ProxyRequestMode::ConnectTunnel => String::new(),
    }
}

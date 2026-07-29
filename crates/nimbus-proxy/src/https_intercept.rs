use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use nimbus_egress::{EgressProtocol, EgressRequest, EgressRule, LayeredEgressPolicy};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::MAX_HTTP_HEADER_BYTES;
use crate::body::{
    BODY_STREAM_CHUNK_BYTES, copy_until_eof, read_exact_body_into_buffer,
    stream_content_length_body, timeout_io,
};
use crate::credentials::CredentialSecretProvider;
use crate::decision_log::{
    ABORT_AFTER_RESPONSE_REASON, DecisionLogger, DurableDecisionSink, EgressDecisionLog,
};
use crate::enforcement::{
    ProxyRequestEnforcementContext, prepare_proxy_request_enforcement,
    reject_unapproved_caller_credentials_for_rule,
};
use crate::fairness::TenantFairness;
use crate::phase::{EgressProxyRequestPhase, RequestPhaseRecorder};
use crate::pingora_io::PrereadStream;
use crate::policy_state::PolicyGeneration;
use crate::request::{ParsedProxyRequest, ProxyRequestMode};
use crate::response::{HttpProxyResponse, write_http_response_async};
use crate::terminal::ResponseStartedSignal;
use crate::tls_authority::WorkloadPepTlsAuthority;
use crate::worker::WorkloadPepHealth;

/// Upper bound on informational (1xx) responses forwarded per intercepted
/// request. RFC-conformant upstreams send at most a couple (100 Continue,
/// 103 Early Hints); anything past this is an allowed-but-misbehaving
/// upstream holding the request open, which must fail closed.
const MAX_INFORMATIONAL_RESPONSES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectInterceptAction {
    Splice,
    Intercept,
}

pub(crate) fn classify_connect(
    matched_rule: Option<&EgressRule>,
    policy_requires_interception: bool,
) -> ConnectInterceptAction {
    if policy_requires_interception
        || matched_rule.is_some_and(|rule| rule.credential.is_some() || !rule.dlp.is_empty())
    {
        ConnectInterceptAction::Intercept
    } else {
        ConnectInterceptAction::Splice
    }
}

pub(crate) struct HttpsInterceptContext<'a> {
    pub(crate) parsed_connect: &'a ParsedProxyRequest,
    pub(crate) upstream_addr: SocketAddr,
    pub(crate) policy: &'a LayeredEgressPolicy,
    pub(crate) outer_matched_rule: Option<String>,
    pub(crate) credential_provider: &'a dyn CredentialSecretProvider,
    pub(crate) tls_authority: &'a WorkloadPepTlsAuthority,
    pub(crate) phase_recorder: &'a RequestPhaseRecorder,
    pub(crate) decision_logger: &'a DecisionLogger,
    pub(crate) durable_decision_sink: &'a DurableDecisionSink,
    pub(crate) health: &'a WorkloadPepHealth,
    pub(crate) response_started_signal: ResponseStartedSignal,
    pub(crate) request_id: &'a str,
    pub(crate) policy_generation: PolicyGeneration,
    pub(crate) tenant_fairness: Option<Arc<TenantFairness>>,
    pub(crate) connect_timeout: Duration,
    pub(crate) io_timeout: Duration,
}

pub(crate) struct HttpsInterceptCompletion {
    pub(crate) decision_log: EgressDecisionLog,
    pub(crate) relay_failed_after_response: bool,
}

impl HttpsInterceptCompletion {
    fn normal(decision_log: EgressDecisionLog) -> Self {
        Self {
            decision_log,
            relay_failed_after_response: false,
        }
    }

    fn relay_failed_after_response(decision_log: EgressDecisionLog) -> Self {
        Self {
            decision_log,
            relay_failed_after_response: true,
        }
    }
}

/// Records the terminal phase and emits the decision log BEFORE any
/// client-visible response bytes, matching the plaintext `deny_terminal`
/// ordering: a denial is never observable by the client before it is logged.
/// Sets `terminal_recorded`, so the worker's post-intercept emit (and the
/// abort guard) skip re-emission.
fn emit_inner_terminal(context: &HttpsInterceptContext<'_>, decision_log: &EgressDecisionLog) {
    context
        .phase_recorder
        .record(EgressProxyRequestPhase::TerminalLog);
    (context.decision_logger)(decision_log.clone());
    context.response_started_signal.disarm();
}

fn record_inner_durable_decision(
    context: &HttpsInterceptContext<'_>,
    decision_log: &EgressDecisionLog,
) -> io::Result<()> {
    match (context.durable_decision_sink)(decision_log) {
        Ok(()) => {
            context.phase_recorder.mark_durable_any_recorded();
            if decision_log.record_kind().is_terminal() {
                context.phase_recorder.mark_durable_terminal_recorded();
            }
            Ok(())
        }
        Err(error) => {
            context.health.mark_audit_unhealthy();
            Err(error)
        }
    }
}

async fn close_inner_without_response<W>(
    context: &HttpsInterceptContext<'_>,
    client_tls: &mut W,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let _ = timeout_io(context.io_timeout, client_tls.shutdown()).await;
    Ok(())
}

pub(crate) async fn intercept_connect_h1(
    mut client: TcpStream,
    buffered_client_bytes: &[u8],
    context: HttpsInterceptContext<'_>,
) -> io::Result<HttpsInterceptCompletion> {
    timeout_io(
        context.io_timeout,
        client.write_all(b"HTTP/1.1 200 Connection Established\r\nConnection: close\r\n\r\n"),
    )
    .await?;
    let server_config = context
        .tls_authority
        .server_config_for_host(&context.parsed_connect.upstream_host)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let preread = PrereadStream::new(client, buffered_client_bytes.to_vec());
    let mut client_tls = match time::timeout(
        context.io_timeout,
        TlsAcceptor::from(server_config).accept(preread),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            return Ok(HttpsInterceptCompletion::normal(connect_denied_log(
                context.parsed_connect,
                context.request_id,
                format!("HTTPS interception failed closed: client TLS handshake failed: {error}"),
                context.outer_matched_rule,
            )));
        }
        Err(_) => {
            return Ok(HttpsInterceptCompletion::normal(connect_denied_log(
                context.parsed_connect,
                context.request_id,
                "HTTPS interception failed closed: client TLS handshake failed: I/O timed out"
                    .to_owned(),
                context.outer_matched_rule,
            )));
        }
    };

    let mut inner_buffer = Vec::new();
    if let Err(response) = read_inner_h1_headers(&mut client_tls, &mut inner_buffer, &context).await
    {
        return write_connect_deny(&context, &mut client_tls, response).await;
    }
    let inner = match parse_intercepted_h1_request(
        &inner_buffer,
        &context.parsed_connect.upstream_host,
        context.parsed_connect.upstream_port,
    ) {
        Ok(inner) => inner,
        Err(response) => {
            return write_connect_deny(&context, &mut client_tls, response).await;
        }
    };

    let egress_request = inner
        .egress_request
        .clone()
        .with_resolved_ip(context.upstream_addr.ip());
    let authorization = context.policy.authorize(&egress_request);
    if !authorization.is_allowed() {
        let response = HttpProxyResponse::forbidden(authorization.reason());
        return write_inner_deny(
            &context,
            &mut client_tls,
            &inner,
            response,
            authorization.matched_rule().map(ToOwned::to_owned),
        )
        .await;
    }

    if let Err(response) = reject_unapproved_caller_credentials_for_rule(
        context.policy.sandbox(),
        authorization.matched_rule(),
        &inner.header_lines,
    ) {
        return write_inner_deny(
            &context,
            &mut client_tls,
            &inner,
            response,
            authorization.matched_rule().map(ToOwned::to_owned),
        )
        .await;
    }

    let mut enforcement_buffer = inner_buffer;
    let enforcement = match prepare_proxy_request_enforcement(
        &inner,
        ProxyRequestEnforcementContext {
            policy: context.policy.sandbox(),
            matched_rule: authorization.matched_rule(),
            reason: authorization.reason(),
            credential_provider: context.credential_provider,
            phase_recorder: context.phase_recorder,
            request_id: context.request_id,
        },
    ) {
        Ok(enforcement) => enforcement,
        Err(response) => {
            return write_inner_deny(
                &context,
                &mut client_tls,
                &inner,
                response,
                authorization.matched_rule().map(ToOwned::to_owned),
            )
            .await;
        }
    };
    let inspected_body = if enforcement.requires_dlp() {
        if let Some(content_length) = inner.content_length {
            if enforcement
                .dlp_max_inspection_bytes()
                .is_some_and(|max| content_length <= max)
            {
                match read_exact_body_into_buffer(
                    &mut client_tls,
                    &mut enforcement_buffer,
                    inner.body_offset,
                    content_length,
                    context.io_timeout,
                )
                .await
                {
                    Ok(body) => Some(body),
                    Err(response) => {
                        return write_inner_deny(
                            &context,
                            &mut client_tls,
                            &inner,
                            response,
                            authorization.matched_rule().map(ToOwned::to_owned),
                        )
                        .await;
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    let prepared = match enforcement.finish(inspected_body) {
        Ok(prepared) => prepared,
        Err(response) => {
            return write_inner_deny(
                &context,
                &mut client_tls,
                &inner,
                response,
                authorization.matched_rule().map(ToOwned::to_owned),
            )
            .await;
        }
    };

    if prepared.inspected_body.is_none()
        && inner.content_length.is_none()
        && enforcement_buffer.len() > inner.body_offset
    {
        return write_inner_deny(
            &context,
            &mut client_tls,
            &inner,
            HttpProxyResponse::bad_request(
                "egress proxy HTTP request bodies require Content-Length",
            ),
            authorization.matched_rule().map(ToOwned::to_owned),
        )
        .await;
    }

    let allowed_decision_log = prepared
        .decision_log
        .clone()
        .with_policy_generation(context.policy_generation);
    if record_inner_durable_decision(&context, &allowed_decision_log).is_err() {
        return write_inner_deny(
            &context,
            &mut client_tls,
            &inner,
            HttpProxyResponse::bad_gateway(
                "HTTPS interception failed closed: inner decision audit append failed",
            ),
            authorization.matched_rule().map(ToOwned::to_owned),
        )
        .await;
    }

    context
        .phase_recorder
        .record(crate::phase::EgressProxyRequestPhase::Forward);
    let mut upstream_tls = match connect_upstream_tls(&context).await {
        Ok(upstream_tls) => upstream_tls,
        Err(InterceptUpstreamError::Dial) => {
            return write_inner_deny(
                &context,
                &mut client_tls,
                &inner,
                HttpProxyResponse::bad_gateway(
                    "HTTPS interception failed closed: upstream dial failed",
                ),
                authorization.matched_rule().map(ToOwned::to_owned),
            )
            .await;
        }
        Err(InterceptUpstreamError::Tls(error)) => {
            return write_inner_deny(
                &context,
                &mut client_tls,
                &inner,
                HttpProxyResponse::bad_gateway(&format!(
                    "HTTPS interception failed closed: upstream TLS verification failed: {error}"
                )),
                authorization.matched_rule().map(ToOwned::to_owned),
            )
            .await;
        }
    };

    let request = render_intercepted_upstream_request(&inner, &prepared.header_lines);
    if timeout_io(
        context.io_timeout,
        upstream_tls.write_all(request.as_bytes()),
    )
    .await
    .is_err()
    {
        return write_inner_deny(
            &context,
            &mut client_tls,
            &inner,
            HttpProxyResponse::bad_gateway(
                "HTTPS interception failed closed: upstream write failed",
            ),
            authorization.matched_rule().map(ToOwned::to_owned),
        )
        .await;
    }
    if let Some(body) = prepared.inspected_body.as_ref() {
        if timeout_io(context.io_timeout, upstream_tls.write_all(body))
            .await
            .is_err()
        {
            return write_inner_deny(
                &context,
                &mut client_tls,
                &inner,
                HttpProxyResponse::bad_gateway(
                    "HTTPS interception failed closed: upstream write failed",
                ),
                authorization.matched_rule().map(ToOwned::to_owned),
            )
            .await;
        }
    } else if let Some(content_length) = inner.content_length
        && stream_content_length_body(
            &mut client_tls,
            &mut upstream_tls,
            &enforcement_buffer[inner.body_offset..],
            content_length,
            context.io_timeout,
        )
        .await
        .is_err()
    {
        return write_inner_deny(
            &context,
            &mut client_tls,
            &inner,
            HttpProxyResponse::bad_gateway(
                "HTTPS interception failed closed: upstream write failed",
            ),
            authorization.matched_rule().map(ToOwned::to_owned),
        )
        .await;
    }
    if timeout_io(context.io_timeout, upstream_tls.flush())
        .await
        .is_err()
    {
        return write_inner_deny(
            &context,
            &mut client_tls,
            &inner,
            HttpProxyResponse::bad_gateway(
                "HTTPS interception failed closed: upstream write failed",
            ),
            authorization.matched_rule().map(ToOwned::to_owned),
        )
        .await;
    }

    // Review follow-up (#7): meter the request direction on intercepted
    // connections too — the splice path already meters both directions.
    if let Some(fairness) = &context.tenant_fairness {
        let body_bytes = prepared
            .inspected_body
            .as_ref()
            .map(|body| body.len() as u64)
            .or_else(|| inner.content_length.map(|length| length as u64))
            .unwrap_or(0);
        fairness.record_bytes_to_upstream(request.len() as u64 + body_bytes);
    }

    let mut informational_responses = 0usize;
    let upstream_head = loop {
        let upstream_head =
            match read_upstream_response_head(&mut upstream_tls, context.io_timeout).await {
                Ok(head) => head,
                Err(_) => {
                    return write_inner_deny(
                        &context,
                        &mut client_tls,
                        &inner,
                        HttpProxyResponse::bad_gateway(
                            "HTTPS interception failed closed: upstream response read failed",
                        ),
                        authorization.matched_rule().map(ToOwned::to_owned),
                    )
                    .await;
                }
            };
        // Classify the upstream head with a FAIL-CLOSED default: only a
        // status line that strictly parses to a known code is forwarded.
        // This closes the whole parser-differential class — a head we cannot
        // positively classify (exotic start-line whitespace, malformed
        // version, missing/oversized code, non-UTF8 that defeats parsing) is
        // DENIED, never forwarded, so a tolerant downstream parser can never
        // be shown bytes we did not understand.
        match classify_upstream_status(&upstream_head) {
            UpstreamStatusClass::Final => break upstream_head,
            UpstreamStatusClass::Informational => {}
            UpstreamStatusClass::Upgrade => {
                return write_inner_deny(
                    &context,
                    &mut client_tls,
                    &inner,
                    HttpProxyResponse::bad_gateway(
                        "HTTPS interception does not support protocol upgrades",
                    ),
                    authorization.matched_rule().map(ToOwned::to_owned),
                )
                .await;
            }
            UpstreamStatusClass::Unrecognized => {
                return write_inner_deny(
                    &context,
                    &mut client_tls,
                    &inner,
                    HttpProxyResponse::bad_gateway(
                        "HTTPS interception failed closed: unrecognized upstream response status line",
                    ),
                    authorization.matched_rule().map(ToOwned::to_owned),
                )
                .await;
            }
        }
        // Bound informational responses: an allowed upstream must not be able
        // to hold the request open forever (or stream unbounded 1xx heads to
        // the client) by emitting 100 Continue before every per-read timeout.
        informational_responses += 1;
        if informational_responses > MAX_INFORMATIONAL_RESPONSES {
            return write_inner_deny(
                &context,
                &mut client_tls,
                &inner,
                HttpProxyResponse::bad_gateway(
                    "HTTPS interception failed closed: upstream exceeded the \
                     informational-response limit before a final response",
                ),
                authorization.matched_rule().map(ToOwned::to_owned),
            )
            .await;
        }
        let informational_head = strip_alt_svc_response_headers(&upstream_head);
        if timeout_io(
            context.io_timeout,
            client_tls.write_all(&informational_head),
        )
        .await
        .is_err()
        {
            return write_inner_deny(
                &context,
                &mut client_tls,
                &inner,
                HttpProxyResponse::bad_gateway(
                    "HTTPS interception failed closed: upstream response read failed",
                ),
                authorization.matched_rule().map(ToOwned::to_owned),
            )
            .await;
        }
    };
    let response_content_length = response_content_length(&upstream_head);
    let filtered_head = strip_alt_svc_response_headers(&upstream_head);
    // Past this point the request is authorized AND executed: policy allowed it,
    // credential/DLP passed, upstream was contacted, and its response head is on
    // the wire to the client. The terminal audit event must therefore record
    // the ALLOW verdict faithfully. A later body-transfer error (upstream reset,
    // client gone) is an operational transport failure, not a policy denial;
    // logging it as a deny would corrupt the audit trail into showing the PEP
    // blocked egress it actually permitted.
    let response_head_reached_client =
        timeout_io(context.io_timeout, client_tls.write_all(&filtered_head))
            .await
            .is_ok();
    if response_head_reached_client {
        context.response_started_signal.mark_response_started(
            allowed_decision_log
                .clone()
                .into_terminal_after_response(ABORT_AFTER_RESPONSE_REASON),
        );
    }
    let relayed_to_workload = match response_content_length {
        Some(length) => stream_response_content_length(
            &mut upstream_tls,
            &mut client_tls,
            length,
            context.io_timeout,
        )
        .await
        .map(|()| length as u64),
        None => copy_until_eof(&mut upstream_tls, &mut client_tls, context.io_timeout).await,
    };
    // EE3: relay copy-loop byte metering, per tenant (best-effort like the
    // relay itself — a relay error already surfaced to the workload).
    if let (Ok(bytes), Some(fairness)) = (&relayed_to_workload, &context.tenant_fairness) {
        fairness.record_bytes_to_workload(*bytes);
    }
    if response_head_reached_client && relayed_to_workload.is_err() {
        return Ok(HttpsInterceptCompletion::relay_failed_after_response(
            allowed_decision_log,
        ));
    }
    Ok(HttpsInterceptCompletion::normal(allowed_decision_log))
}

async fn write_connect_deny<W>(
    context: &HttpsInterceptContext<'_>,
    client_tls: &mut W,
    response: HttpProxyResponse,
) -> io::Result<HttpsInterceptCompletion>
where
    W: AsyncWrite + Unpin,
{
    let decision_log = connect_denied_log(
        context.parsed_connect,
        context.request_id,
        response.body().to_owned(),
        context.outer_matched_rule.clone(),
    )
    .with_policy_generation(context.policy_generation);
    if record_inner_durable_decision(context, &decision_log).is_err() {
        emit_inner_terminal(context, &decision_log);
        let _ = close_inner_without_response(context, client_tls).await;
        return Ok(HttpsInterceptCompletion::normal(decision_log));
    }
    emit_inner_terminal(context, &decision_log);
    let _ = write_http_response_async(client_tls, response).await;
    Ok(HttpsInterceptCompletion::normal(decision_log))
}

async fn write_inner_deny<W>(
    context: &HttpsInterceptContext<'_>,
    client_tls: &mut W,
    parsed: &ParsedProxyRequest,
    response: HttpProxyResponse,
    matched_rule: Option<String>,
) -> io::Result<HttpsInterceptCompletion>
where
    W: AsyncWrite + Unpin,
{
    let reason = response.body().to_owned();
    let decision_log = EgressDecisionLog::denied(context.request_id, parsed, reason, matched_rule)
        .with_policy_generation(context.policy_generation);
    if record_inner_durable_decision(context, &decision_log).is_err() {
        emit_inner_terminal(context, &decision_log);
        let _ = close_inner_without_response(context, client_tls).await;
        return Ok(HttpsInterceptCompletion::normal(decision_log));
    }
    emit_inner_terminal(context, &decision_log);
    let _ = write_http_response_async(client_tls, response).await;
    Ok(HttpsInterceptCompletion::normal(decision_log))
}

#[derive(Debug)]
enum InterceptUpstreamError {
    Dial,
    Tls(String),
}

async fn connect_upstream_tls(
    context: &HttpsInterceptContext<'_>,
) -> std::result::Result<tokio_rustls::client::TlsStream<TcpStream>, InterceptUpstreamError> {
    let upstream = time::timeout(
        context.connect_timeout,
        TcpStream::connect(context.upstream_addr),
    )
    .await
    .map_err(|_| InterceptUpstreamError::Dial)?
    .map_err(|_| InterceptUpstreamError::Dial)?;
    let config = context
        .tls_authority
        .upstream_client_config()
        .map_err(|error| InterceptUpstreamError::Tls(error.to_string()))?;
    let server_name = ServerName::try_from(context.parsed_connect.upstream_host.clone())
        .map_err(|_| InterceptUpstreamError::Tls("upstream TLS SNI is invalid".to_owned()))?;
    time::timeout(
        context.io_timeout,
        TlsConnector::from(config).connect(server_name, upstream),
    )
    .await
    .map_err(|_| InterceptUpstreamError::Tls("upstream TLS handshake timed out".to_owned()))?
    .map_err(|error| InterceptUpstreamError::Tls(error.to_string()))
}

async fn read_inner_h1_headers<R>(
    stream: &mut R,
    buffer: &mut Vec<u8>,
    context: &HttpsInterceptContext<'_>,
) -> std::result::Result<(), HttpProxyResponse>
where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0_u8; 1024];
    loop {
        let read = timeout_io(context.io_timeout, stream.read(&mut chunk))
            .await
            .map_err(|_| {
                HttpProxyResponse::bad_request(
                    "HTTPS interception failed closed: TLS client request was unreadable",
                )
            })?;
        if read == 0 {
            return Err(HttpProxyResponse::bad_request(
                "HTTPS interception failed closed: TLS client closed before HTTP headers",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.starts_with(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n") {
            return Err(HttpProxyResponse::not_implemented(
                "HTTPS interception failed closed: HTTP/2 is not supported",
            ));
        }
        if find_header_end(buffer).is_some() {
            return Ok(());
        }
        if buffer.len() > MAX_HTTP_HEADER_BYTES {
            return Err(HttpProxyResponse::request_header_fields_too_large(
                "HTTPS interception failed closed: inner request headers are too large",
            ));
        }
    }
}

async fn read_upstream_response_head<R>(stream: &mut R, io_timeout: Duration) -> io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = timeout_io(io_timeout, stream.read(&mut byte)).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "upstream closed before response headers",
            ));
        }
        buffer.push(byte[0]);
        if find_header_end(&buffer).is_some() {
            return Ok(buffer);
        }
        if buffer.len() > MAX_HTTP_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "upstream response headers are too large",
            ));
        }
    }
}

fn parse_intercepted_h1_request(
    buffer: &[u8],
    expected_host: &str,
    expected_port: u16,
) -> std::result::Result<ParsedProxyRequest, HttpProxyResponse> {
    let Some(header_end) = find_header_end(buffer) else {
        return Err(HttpProxyResponse::bad_request(
            "HTTPS interception failed closed: inner HTTP headers are missing",
        ));
    };
    let body_offset = header_end + 4;
    let headers = std::str::from_utf8(&buffer[..header_end]).map_err(|_| {
        HttpProxyResponse::bad_request(
            "HTTPS interception failed closed: inner HTTP headers must be UTF-8",
        )
    })?;
    if has_bare_cr_or_lf(headers.as_bytes()) {
        return Err(HttpProxyResponse::bad_request(
            "HTTPS interception failed closed: malformed inner HTTP line endings",
        ));
    }
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if method.is_empty() || target.is_empty() || version.is_empty() || parts.next().is_some() {
        return Err(HttpProxyResponse::bad_request(
            "HTTPS interception failed closed: inner request line is invalid",
        ));
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(HttpProxyResponse::not_implemented(
            "HTTPS interception failed closed: only HTTP/1.1 interception is supported",
        ));
    }
    if method.eq_ignore_ascii_case("CONNECT") || !target.starts_with('/') {
        return Err(HttpProxyResponse::bad_request(
            "HTTPS interception failed closed: inner request must be origin-form HTTP",
        ));
    }

    let mut host_header = None;
    let mut header_lines = Vec::new();
    let mut content_length = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(HttpProxyResponse::bad_request(
                "HTTPS interception failed closed: malformed inner header",
            ));
        };
        let name = name.trim();
        if name.eq_ignore_ascii_case("host") {
            if host_header.is_some() {
                return Err(HttpProxyResponse::bad_request(
                    "HTTPS interception failed closed: multiple Host headers",
                ));
            }
            host_header = Some(value.trim().to_owned());
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(HttpProxyResponse::bad_request(
                "HTTPS interception failed closed: Transfer-Encoding is unsupported",
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value.trim().parse::<usize>().map_err(|_| {
                HttpProxyResponse::bad_request(
                    "HTTPS interception failed closed: invalid Content-Length",
                )
            })?;
            if content_length.replace(parsed).is_some() {
                return Err(HttpProxyResponse::bad_request(
                    "HTTPS interception failed closed: multiple Content-Length headers",
                ));
            }
        }
        if name.eq_ignore_ascii_case("upgrade")
            || (name.eq_ignore_ascii_case("connection")
                && value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade")))
        {
            return Err(HttpProxyResponse::not_implemented(
                "HTTPS interception failed closed: WebSocket/upgrade is not supported",
            ));
        }
        if !name.eq_ignore_ascii_case("connection")
            && !name.eq_ignore_ascii_case("proxy-connection")
        {
            header_lines.push(line.to_owned());
        }
    }
    let host_header = host_header.ok_or_else(|| {
        HttpProxyResponse::bad_request("HTTPS interception failed closed: Host header is required")
    })?;
    if !host_header_matches(&host_header, expected_host, expected_port)? {
        return Err(HttpProxyResponse::bad_request(
            "HTTPS interception failed closed: SNI/Host/authority mismatch",
        ));
    }

    let path = target
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(target);
    let egress_request = EgressRequest::new(
        EgressProtocol::Https,
        expected_host.to_owned(),
        expected_port,
    )
    .with_http(method, path);
    Ok(ParsedProxyRequest {
        egress_request,
        upstream_host: expected_host.to_owned(),
        upstream_port: expected_port,
        mode: ProxyRequestMode::ForwardHttp {
            origin_form: target.to_owned(),
        },
        method: method.to_owned(),
        version: version.to_owned(),
        header_lines,
        content_length,
        body_offset,
    })
}

fn host_header_matches(
    host_header: &str,
    expected_host: &str,
    expected_port: u16,
) -> std::result::Result<bool, HttpProxyResponse> {
    let (host, port) = split_host_header(host_header)?;
    let host = nimbus_egress::canonicalize_authority_host(host).map_err(|_| {
        HttpProxyResponse::bad_request("HTTPS interception failed closed: Host header is invalid")
    })?;
    Ok(host == expected_host && port.unwrap_or(expected_port) == expected_port)
}

fn has_bare_cr_or_lf(headers: &[u8]) -> bool {
    for (index, byte) in headers.iter().enumerate() {
        match *byte {
            b'\r' if headers.get(index + 1) != Some(&b'\n') => return true,
            b'\n' if index == 0 || headers.get(index - 1) != Some(&b'\r') => return true,
            _ => {}
        }
    }
    false
}

fn split_host_header(
    host_header: &str,
) -> std::result::Result<(&str, Option<u16>), HttpProxyResponse> {
    if let Some(rest) = host_header.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return Err(HttpProxyResponse::bad_request(
                "HTTPS interception failed closed: Host header is invalid",
            ));
        };
        let port = if let Some(port) = suffix.strip_prefix(':') {
            Some(port.parse::<u16>().map_err(|_| {
                HttpProxyResponse::bad_request(
                    "HTTPS interception failed closed: Host header port is invalid",
                )
            })?)
        } else if suffix.is_empty() {
            None
        } else {
            return Err(HttpProxyResponse::bad_request(
                "HTTPS interception failed closed: Host header is invalid",
            ));
        };
        return Ok((host, port));
    }
    if let Some((host, port)) = host_header.rsplit_once(':')
        && !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
    {
        let port = port.parse::<u16>().map_err(|_| {
            HttpProxyResponse::bad_request(
                "HTTPS interception failed closed: Host header port is invalid",
            )
        })?;
        return Ok((host, Some(port)));
    }
    Ok((host_header, None))
}

fn render_intercepted_upstream_request(
    parsed: &ParsedProxyRequest,
    header_lines: &[String],
) -> String {
    let ProxyRequestMode::ForwardHttp { origin_form } = &parsed.mode else {
        return String::new();
    };
    let mut rendered = format!("{} {} {}\r\n", parsed.method, origin_form, parsed.version);
    rendered.push_str(&format!(
        "Host: {}:{}\r\n",
        parsed.upstream_host, parsed.upstream_port
    ));
    for line in header_lines {
        let name = line
            .split_once(':')
            .map(|(name, _)| name.trim())
            .unwrap_or_default();
        if name.eq_ignore_ascii_case("host")
            || name.eq_ignore_ascii_case("connection")
            || name.eq_ignore_ascii_case("proxy-connection")
        {
            continue;
        }
        rendered.push_str(line);
        rendered.push_str("\r\n");
    }
    rendered.push_str("Connection: close\r\n\r\n");
    rendered
}

fn strip_alt_svc_response_headers(response: &[u8]) -> Vec<u8> {
    let Some(header_end) = find_header_end(response) else {
        return response.to_vec();
    };
    let Ok(headers) = std::str::from_utf8(&response[..header_end]) else {
        return response.to_vec();
    };
    let mut filtered = String::new();
    for (index, line) in headers.split("\r\n").enumerate() {
        if index == 0
            || !line
                .split_once(':')
                .map(|(name, _)| name.trim().eq_ignore_ascii_case("alt-svc"))
                .unwrap_or(false)
        {
            filtered.push_str(line);
            filtered.push_str("\r\n");
        }
    }
    filtered.push_str("\r\n");
    let mut rendered = filtered.into_bytes();
    rendered.extend_from_slice(&response[header_end + 4..]);
    rendered
}

fn response_content_length(response_head: &[u8]) -> Option<usize> {
    let header_end = find_header_end(response_head)?;
    let headers = std::str::from_utf8(&response_head[..header_end]).ok()?;
    headers.split("\r\n").find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    })
}

/// Fail-closed classification of an upstream response head's status line.
/// `Unrecognized` is the default for anything that does not strictly parse
/// to a known code — the intercept relay denies it rather than forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamStatusClass {
    /// A final response the client should receive (2xx–5xx).
    Final,
    /// An interim informational response (100 / 102–199) to forward.
    Informational,
    /// 101 Switching Protocols — upgrades are unsupported, fail closed.
    Upgrade,
    /// Unparseable or out-of-range status line — fail closed.
    Unrecognized,
}

fn classify_upstream_status(response_head: &[u8]) -> UpstreamStatusClass {
    match response_status(response_head) {
        Some(101) => UpstreamStatusClass::Upgrade,
        Some(100) | Some(102..=199) => UpstreamStatusClass::Informational,
        Some(200..=599) => UpstreamStatusClass::Final,
        // None (could not parse) or any other numeric range is never
        // forwarded — fail closed.
        _ => UpstreamStatusClass::Unrecognized,
    }
}

fn response_status(response_head: &[u8]) -> Option<u16> {
    // Decode ONLY the version and status-code tokens as raw ASCII bytes,
    // never the reason phrase. HTTP/1.x reason phrases (and later header
    // values) may carry non-UTF8 obs-text; UTF-8-decoding any of that would
    // fail open — a valid `HTTP/1.1 101 \xff` would return None and slip
    // past the 101 upgrade guard. Version + status code are ASCII by the
    // HTTP grammar, so we can read them without touching the reason bytes.
    let line_end = response_head
        .windows(2)
        .position(|window| window == b"\r\n")
        .unwrap_or(response_head.len());
    let status_line = &response_head[..line_end];
    // Split on HTTP whitespace (SP or HTAB): the grammar specifies single
    // SP, but tolerant downstream parsers accept HTAB/runs, so a strict
    // SP-only split would let an HTAB-delimited `HTTP/1.1\t101` fail open
    // past the upgrade guard.
    let mut tokens = status_line
        .split(|byte| *byte == b' ' || *byte == b'\t')
        .filter(|token| !token.is_empty());
    // Validate the version token: this is an HTTP/1 relay, so only
    // `HTTP/1.0` / `HTTP/1.1` are understood. Anything else (`ICY`, HTTP/2
    // preface fragments, a CR-embedded version) is unparseable and must fail
    // closed rather than be classified from the next digit token alone.
    let version = tokens.next()?;
    if version != b"HTTP/1.0" && version != b"HTTP/1.1" {
        return None;
    }
    let status_code = tokens.next()?;
    // A status code is exactly three ASCII digits.
    if status_code.len() != 3 || !status_code.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(status_code).ok()?.parse().ok()
}

async fn stream_response_content_length<R, W>(
    reader: &mut R,
    writer: &mut W,
    content_length: usize,
    io_timeout: Duration,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut remaining = content_length;
    let mut chunk = [0_u8; BODY_STREAM_CHUNK_BYTES];
    while remaining > 0 {
        let read_len = remaining.min(chunk.len());
        let read = timeout_io(io_timeout, reader.read(&mut chunk[..read_len])).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "upstream closed before declared response body completed",
            ));
        }
        timeout_io(io_timeout, writer.write_all(&chunk[..read])).await?;
        remaining -= read;
    }
    Ok(())
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn connect_denied_log(
    parsed: &ParsedProxyRequest,
    request_id: &str,
    reason: String,
    matched_rule: Option<String>,
) -> EgressDecisionLog {
    EgressDecisionLog::denied(request_id, parsed, reason, matched_rule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_egress::{EgressCredentialInjection, EgressDlpRule};

    #[test]
    fn response_status_reads_status_line_despite_non_utf8_headers() {
        // A valid 101 status line followed by a header carrying non-UTF8
        // obs-text must still be classified as 101 — the upgrade guard must
        // not fail open just because a later header byte is not UTF-8.
        let mut head = b"HTTP/1.1 101 Switching Protocols\r\nX-Obs: ".to_vec();
        head.extend_from_slice(&[0xff, 0xfe, 0x80]);
        head.extend_from_slice(b"\r\n\r\n");
        assert_eq!(response_status(&head), Some(101));
        assert_eq!(
            classify_upstream_status(&head),
            UpstreamStatusClass::Upgrade
        );

        // Non-UTF8 bytes in the REASON PHRASE itself must not fail open.
        let mut obs_reason = b"HTTP/1.1 101 ".to_vec();
        obs_reason.extend_from_slice(&[0xff, 0xfe]);
        obs_reason.extend_from_slice(b"\r\n\r\n");
        assert_eq!(response_status(&obs_reason), Some(101));
        assert_eq!(
            classify_upstream_status(&obs_reason),
            UpstreamStatusClass::Upgrade
        );

        // A 1xx with an obs-text reason must still classify as informational.
        let mut obs_100 = b"HTTP/1.1 100 ".to_vec();
        obs_100.extend_from_slice(&[0x80]);
        obs_100.extend_from_slice(b"\r\n\r\n");
        assert_eq!(response_status(&obs_100), Some(100));
        assert_eq!(
            classify_upstream_status(&obs_100),
            UpstreamStatusClass::Informational
        );

        // HTAB-delimited status lines (accepted by tolerant parsers) must
        // not fail open past the upgrade guard.
        assert_eq!(
            response_status(b"HTTP/1.1\t101\tSwitching Protocols\r\n\r\n"),
            Some(101)
        );
        assert_eq!(
            response_status(b"HTTP/1.1 101\tSwitching Protocols\r\n\r\n"),
            Some(101)
        );
        // Non-3-digit or missing codes classify as unknown, never a bypass.
        assert_eq!(response_status(b"HTTP/1.1 1010 x\r\n\r\n"), None);
        assert_eq!(response_status(b"HTTP/1.1\r\n\r\n"), None);
    }

    #[test]
    fn response_status_parses_ordinary_final_and_informational() {
        assert_eq!(
            response_status(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"),
            Some(200)
        );
        assert_eq!(response_status(b"HTTP/1.1 100 Continue\r\n\r\n"), Some(100));
        assert_eq!(
            classify_upstream_status(b"HTTP/1.1 100 Continue\r\n\r\n"),
            UpstreamStatusClass::Informational
        );
    }

    #[test]
    fn classify_upstream_status_fails_closed_on_unparseable_status_lines() {
        // Final and known interim/upgrade classes.
        assert_eq!(
            classify_upstream_status(b"HTTP/1.1 200 OK\r\n\r\n"),
            UpstreamStatusClass::Final
        );
        assert_eq!(
            classify_upstream_status(b"HTTP/1.1 503 x\r\n\r\n"),
            UpstreamStatusClass::Final
        );
        assert_eq!(
            classify_upstream_status(b"HTTP/1.1 101 Switching Protocols\r\n\r\n"),
            UpstreamStatusClass::Upgrade
        );

        // The whole parser-differential class fails CLOSED: any start-line
        // whitespace we do not split on (VT, FF, bare CR) yields an
        // unparseable status and is denied, never forwarded — so a tolerant
        // downstream parser can never be shown a 101 we did not classify.
        for exotic in [b"\x0b".as_slice(), b"\x0c", b"\r"] {
            let mut head = b"HTTP/1.1".to_vec();
            head.extend_from_slice(exotic);
            head.extend_from_slice(b"101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n");
            assert_eq!(
                classify_upstream_status(&head),
                UpstreamStatusClass::Unrecognized,
                "exotic start-line whitespace must fail closed, not forward"
            );
        }
        // Missing status code and an unknown numeric range also fail closed.
        assert_eq!(
            classify_upstream_status(b"HTTP/1.1\r\n\r\n"),
            UpstreamStatusClass::Unrecognized
        );
        assert_eq!(
            classify_upstream_status(b"HTTP/1.1 700 weird\r\n\r\n"),
            UpstreamStatusClass::Unrecognized
        );
        // A non-HTTP/1 version token must fail closed even with a valid code.
        assert_eq!(response_status(b"ICY 200 OK\r\n\r\n"), None);
        assert_eq!(response_status(b"HTTP/2 200\r\n\r\n"), None);
        assert_eq!(
            classify_upstream_status(b"ICY 200 OK\r\n\r\n"),
            UpstreamStatusClass::Unrecognized
        );
        assert_eq!(
            response_status(b"HTTP/1.0 204 No Content\r\n\r\n"),
            Some(204)
        );
    }

    #[test]
    fn connect_classifier_selects_splice_or_intercept_from_rule_requirements() {
        let plain = EgressRule::new("plain", EgressProtocol::Https, "allowed.test", 443);
        let credential = EgressRule::new("credential", EgressProtocol::Https, "allowed.test", 443)
            .with_credential_injection(EgressCredentialInjection::new(
                "api-token",
                "Authorization",
            ));
        let dlp = EgressRule::new("dlp", EgressProtocol::Https, "allowed.test", 443)
            .with_dlp_rules([EgressDlpRule::new("secret", "secret")]);
        let both = EgressRule::new("both", EgressProtocol::Https, "allowed.test", 443)
            .with_credential_injection(EgressCredentialInjection::new("api-token", "Authorization"))
            .with_dlp_rules([EgressDlpRule::new("secret", "secret")]);

        assert_eq!(
            classify_connect(Some(&plain), false),
            ConnectInterceptAction::Splice
        );
        assert_eq!(
            classify_connect(Some(&credential), false),
            ConnectInterceptAction::Intercept
        );
        assert_eq!(
            classify_connect(Some(&dlp), false),
            ConnectInterceptAction::Intercept
        );
        assert_eq!(
            classify_connect(Some(&both), false),
            ConnectInterceptAction::Intercept
        );
        assert_eq!(
            classify_connect(Some(&plain), true),
            ConnectInterceptAction::Intercept,
            "method/path policy must force inspection even without credentials or DLP"
        );
    }

    #[test]
    fn intercepted_h1_rejects_host_mismatch_h2_and_upgrade() {
        let host_mismatch = match parse_intercepted_h1_request(
            b"GET /ok HTTP/1.1\r\nHost: other.test\r\n\r\n",
            "allowed.test",
            443,
        ) {
            Ok(_) => panic!("Host mismatch should fail closed"),
            Err(response) => response,
        };
        assert!(host_mismatch.body().contains("mismatch"));

        let h2 = match parse_intercepted_h1_request(
            b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
            "allowed.test",
            443,
        ) {
            Ok(_) => panic!("h2 preface should fail closed"),
            Err(response) => response,
        };
        assert!(h2.body().contains("origin-form") || h2.body().contains("HTTP/1.1"));

        let upgrade = match parse_intercepted_h1_request(
            b"GET /ws HTTP/1.1\r\nHost: allowed.test\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
            "allowed.test",
            443,
        ) {
            Ok(_) => panic!("websocket upgrade should fail closed"),
            Err(response) => response,
        };
        assert!(upgrade.body().contains("WebSocket"));
    }

    #[test]
    fn intercepted_response_strips_alt_svc_to_prevent_quic_upgrade() {
        let response = b"HTTP/1.1 200 OK\r\nAlt-Svc: h3=\":443\"\r\nContent-Length: 2\r\n\r\nok";
        let filtered = String::from_utf8(strip_alt_svc_response_headers(response))
            .expect("filtered response should remain utf8");

        assert!(filtered.starts_with("HTTP/1.1 200 OK"));
        assert!(!filtered.to_ascii_lowercase().contains("alt-svc"));
        assert!(filtered.ends_with("\r\n\r\nok"));
    }
}

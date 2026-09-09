//! Session channel routes: the NDJSON stream of one channel of an open
//! session and the input that flows back to it.
//!
//! `GET /api/sessions/{session_id}/channels/{channel}/stream` answers
//! `application/x-ndjson`, one frame per line: `opened` first, the source
//! frames (`stdout`, `stderr`, `exit`) as they arrive, and `closed` last.
//! `POST .../input` hands `{ "data": "..." }` to the attached channel. The
//! frame contract lives in `nimbus_services::SessionChannelFrame`; this
//! module owns only the wire shape and the per-frame byte accounting.

use std::sync::Arc;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use nimbus_core::TenantId;
use nimbus_services::{ServiceManager, SessionChannelFrame, SessionChannelStream};
use serde::{Deserialize, Serialize};

use super::authz::{OperatorAuthScope, record_operator_authorization_audit};
use super::resource_control::sessions::{
    SessionAction, authorize_session_resource_lookup, authorize_session_resource_target,
};
use super::sessions::{service_manager, session_not_found};
use super::{AppError, AppState, parse_user_tenant_id};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionChannelQuery {
    tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionChannelInputRequest {
    data: String,
}

/// One NDJSON line of the stream.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SessionChannelWireFrame<'a> {
    #[serde(rename_all = "camelCase")]
    Opened {
        channel: &'a str,
        target_generation: u64,
    },
    Stdout {
        data: &'a str,
    },
    Stderr {
        data: &'a str,
    },
    Exit {
        code: i32,
    },
    Closed {
        reason: &'a str,
    },
}

pub(crate) async fn stream_session_channel(
    State(state): State<Arc<AppState>>,
    Path((session_id, channel)): Path<(String, String)>,
    QueryParams(query): QueryParams<SessionChannelQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (manager, tenant_id) = authorize_channel(
        &state,
        &headers,
        &session_id,
        query.tenant_id.as_deref(),
        &channel,
        "stream",
    )
    .await?;
    let stream = manager.attach_session_channel(&tenant_id, &session_id, &channel)?;
    let body = Body::from_stream(session_channel_body(
        manager, tenant_id, session_id, channel, stream,
    ));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-accel-buffering", "no")
        .body(body)
        .map_err(|error| {
            AppError::from(nimbus_core::Error::Internal(format!(
                "session channel response: {error}"
            )))
        })
}

pub(crate) async fn write_session_channel(
    State(state): State<Arc<AppState>>,
    Path((session_id, channel)): Path<(String, String)>,
    QueryParams(query): QueryParams<SessionChannelQuery>,
    headers: HeaderMap,
    Json(request): Json<SessionChannelInputRequest>,
) -> Result<StatusCode, AppError> {
    let (manager, tenant_id) = authorize_channel(
        &state,
        &headers,
        &session_id,
        query.tenant_id.as_deref(),
        &channel,
        "input",
    )
    .await?;
    manager
        .write_session_channel(&tenant_id, &session_id, &channel, request.data)
        .await?;
    Ok(StatusCode::ACCEPTED)
}

/// Both routes read the session the way `get_session` does: the lookup and
/// the target are authorized as a session get, and the audit line names the
/// channel and the verb.
async fn authorize_channel(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    session_id: &str,
    route_tenant_id: Option<&str>,
    channel: &str,
    verb: &str,
) -> Result<(Arc<ServiceManager>, TenantId), AppError> {
    let route_tenant_id = route_tenant_id.map(parse_user_tenant_id).transpose()?;
    let lookup_authorization = authorize_session_resource_lookup(
        state,
        headers,
        SessionAction::Get,
        session_id,
        route_tenant_id.as_ref(),
    )
    .await?;
    let manager = service_manager(state)?;
    let session = manager
        .get_session(&lookup_authorization.tenant_id, session_id)
        .ok_or_else(|| session_not_found(session_id))?;
    let authorization = authorize_session_resource_target(
        state,
        headers,
        &lookup_authorization,
        &session,
        SessionAction::Get,
    )
    .await?;
    record_operator_authorization_audit(
        state,
        headers,
        authorization.tenant_id.as_str(),
        OperatorAuthScope::Session,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("session channel {verb} authorized id={session_id} channel={channel}"),
    );
    Ok((manager, authorization.tenant_id))
}

/// Ends the attachment when the response body is dropped, whether the
/// source finished or the client went away.
struct ChannelAttachmentGuard {
    manager: Arc<ServiceManager>,
    tenant_id: TenantId,
    session_id: String,
    channel: String,
}

impl Drop for ChannelAttachmentGuard {
    fn drop(&mut self) {
        // A session closed underneath the stream already dropped the channel
        // state; that answer is not an error here.
        let _ = self.manager.detach_session_channel(
            &self.tenant_id,
            &self.session_id,
            &self.channel,
            "stream_ended",
        );
    }
}

struct ChannelBodyState {
    guard: ChannelAttachmentGuard,
    stream: SessionChannelStream,
    opened_sent: bool,
    closed_sent: bool,
}

fn session_channel_body(
    manager: Arc<ServiceManager>,
    tenant_id: TenantId,
    session_id: String,
    channel: String,
    stream: SessionChannelStream,
) -> impl futures::Stream<Item = Result<Bytes, std::convert::Infallible>> {
    let state = ChannelBodyState {
        guard: ChannelAttachmentGuard {
            manager,
            tenant_id,
            session_id,
            channel,
        },
        stream,
        opened_sent: false,
        closed_sent: false,
    };
    futures::stream::unfold(state, |mut state| async move {
        if !state.opened_sent {
            state.opened_sent = true;
            let line = encode(&SessionChannelWireFrame::Opened {
                channel: &state.guard.channel,
                target_generation: state.stream.target_generation,
            });
            return Some((Ok(line), state));
        }
        if state.closed_sent {
            return None;
        }
        let frame = match state.stream.frames.recv().await {
            Some(frame) => frame,
            None => SessionChannelFrame::Closed {
                reason: "source_closed".to_owned(),
            },
        };
        let wire = match &frame {
            SessionChannelFrame::Stdout(data) => SessionChannelWireFrame::Stdout { data },
            SessionChannelFrame::Stderr(data) => SessionChannelWireFrame::Stderr { data },
            SessionChannelFrame::Exit { code } => SessionChannelWireFrame::Exit { code: *code },
            SessionChannelFrame::Closed { reason } => SessionChannelWireFrame::Closed { reason },
        };
        let line = encode(&wire);
        if let Err(error) = account_frame(&state.guard, line.len()) {
            // The channel's high watermark refused the frame: end the stream
            // with the reason instead of dropping bytes silently.
            state.closed_sent = true;
            let reason = error.to_string();
            let line = encode(&SessionChannelWireFrame::Closed { reason: &reason });
            return Some((Ok(line), state));
        }
        if matches!(frame, SessionChannelFrame::Closed { .. }) {
            state.closed_sent = true;
        }
        Some((Ok(line), state))
    })
}

/// Runs the frame through the channel's byte accounting: enqueue, then drain
/// once the line is handed to the body. A frame that would cross the high
/// watermark is refused.
fn account_frame(guard: &ChannelAttachmentGuard, bytes: usize) -> Result<(), nimbus_core::Error> {
    guard
        .manager
        .enqueue_session_channel_target_to_client_bytes(
            &guard.tenant_id,
            &guard.session_id,
            &guard.channel,
            bytes,
        )?;
    guard.manager.drain_session_channel_target_to_client_bytes(
        &guard.tenant_id,
        &guard.session_id,
        &guard.channel,
        bytes,
    )
}

fn encode(frame: &SessionChannelWireFrame<'_>) -> Bytes {
    let mut line = serde_json::to_vec(frame).expect("session channel frame should serialize");
    line.push(b'\n');
    Bytes::from(line)
}

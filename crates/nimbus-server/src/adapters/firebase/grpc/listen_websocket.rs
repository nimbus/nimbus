#![allow(clippy::result_large_err)]

use axum::extract::Extension;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures::channel::mpsc;
use futures::{Sink, SinkExt, StreamExt};
use prost::Message as ProstMessage;
use tonic::{Code, Status};

use super::FirestoreGrpcService;
use super::generated::google::firestore::v1::ListenRequest;
use super::listen_stream;
use crate::application_auth::{grpc_status_from_app_error, resolve_application_auth_from_bearer};
use crate::state::record_authenticated_usage;

const FIRESTORE_LISTEN_AUTH_SUBPROTOCOL_PREFIX: &str = "nimbus.firebase.auth.";
pub(crate) const FIRESTORE_LISTEN_WEBSOCKET_PROTOCOL: &str = "nimbus.firebase.listen.v1";

pub(crate) async fn listen_websocket(
    Extension(service): Extension<FirestoreGrpcService>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let upgrade_auth = resolve_upgrade_bearer(&headers);
    ws.protocols([FIRESTORE_LISTEN_WEBSOCKET_PROTOCOL])
        .on_upgrade(move |socket| handle_listen_websocket(socket, service, upgrade_auth))
}

async fn handle_listen_websocket(
    socket: WebSocket,
    service: FirestoreGrpcService,
    upgrade_auth: Result<Option<String>, Status>,
) {
    let (mut sender, mut receiver) = socket.split();
    let bearer = match upgrade_auth {
        Ok(bearer) => bearer,
        Err(status) => {
            close_with_status(&mut sender, &status).await;
            return;
        }
    };
    let state = match service.app_state() {
        Ok(state) => state,
        Err(status) => {
            close_with_status(&mut sender, &status).await;
            return;
        }
    };
    let auth = match resolve_application_auth_from_bearer(&state, bearer.as_deref())
        .await
        .map_err(grpc_status_from_app_error)
    {
        Ok(auth) => auth,
        Err(status) => {
            close_with_status(&mut sender, &status).await;
            return;
        }
    };
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let (request_tx, request_rx) = mpsc::unbounded::<Result<ListenRequest, Status>>();
    let mut responses =
        match listen_stream::listen_response_stream(&service, request_rx, auth.principal) {
            Ok(responses) => responses,
            Err(status) => {
                close_with_status(&mut sender, &status).await;
                return;
            }
        };
    let mut request_tx = Some(request_tx);

    loop {
        tokio::select! {
            inbound = receiver.next() => {
                match inbound {
                    Some(Ok(Message::Binary(payload))) => {
                        let request = match ListenRequest::decode(payload.as_ref()) {
                            Ok(request) => request,
                            Err(_) => {
                                close_with_status(
                                    &mut sender,
                                    &Status::invalid_argument(
                                        "Listen WebSocket frames must contain a valid protobuf ListenRequest",
                                    ),
                                )
                                .await;
                                return;
                            }
                        };
                        if request_tx
                            .as_mut()
                            .and_then(|tx| tx.unbounded_send(Ok(request)).err())
                            .is_some()
                        {
                            return;
                        }
                    }
                    Some(Ok(Message::Text(_))) => {
                        close_with_code(
                            &mut sender,
                            close_code::UNSUPPORTED,
                            "Listen WebSocket requires binary protobuf frames",
                        )
                        .await;
                        return;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            return;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Err(_)) => return,
                }
            }
            outbound = responses.next() => {
                match outbound {
                    Some(Ok(response)) => {
                        if sender
                            .send(Message::Binary(response.encode_to_vec().into()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Some(Err(status)) => {
                        close_with_status(&mut sender, &status).await;
                        return;
                    }
                    None => {
                        let _ = request_tx.take();
                        let _ = sender.close().await;
                        return;
                    }
                }
            }
        }
    }
}

async fn close_with_status<S>(sender: &mut S, status: &Status)
where
    S: Sink<Message> + Unpin,
{
    let code = match status.code() {
        Code::InvalidArgument
        | Code::PermissionDenied
        | Code::Unauthenticated
        | Code::FailedPrecondition
        | Code::OutOfRange
        | Code::Aborted
        | Code::Cancelled
        | Code::Unimplemented => close_code::POLICY,
        Code::ResourceExhausted
        | Code::Internal
        | Code::Unavailable
        | Code::DeadlineExceeded
        | Code::DataLoss
        | Code::Unknown => close_code::ERROR,
        Code::Ok | Code::NotFound | Code::AlreadyExists => close_code::POLICY,
    };
    close_with_code(sender, code, status.message()).await;
}

async fn close_with_code<S>(sender: &mut S, code: u16, reason: impl Into<String>)
where
    S: Sink<Message> + Unpin,
{
    let _ = sender
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into().into(),
        })))
        .await;
}

fn resolve_upgrade_bearer(headers: &HeaderMap) -> Result<Option<String>, Status> {
    let header_bearer = extract_authorization_bearer(headers)?;
    let offered_bearer = extract_subprotocol_bearer(headers)?;
    match (header_bearer, offered_bearer) {
        (Some(header), Some(offered)) if header != offered => Err(Status::unauthenticated(
            "Firestore Listen WebSocket auth offer does not match Authorization header.",
        )),
        (Some(header), _) => Ok(Some(header)),
        (None, offered) => Ok(offered),
    }
}

fn extract_authorization_bearer(headers: &HeaderMap) -> Result<Option<String>, Status> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| {
        Status::invalid_argument("Firestore Listen Authorization header is invalid.")
    })?;
    let (scheme, token) = value.split_once(' ').ok_or_else(|| {
        Status::invalid_argument(
            "Firestore Listen Authorization header must use the Bearer scheme.",
        )
    })?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(Status::invalid_argument(
            "Firestore Listen Authorization header must use the Bearer scheme.",
        ));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(Status::invalid_argument(
            "Firestore Listen Authorization header is missing a token.",
        ));
    }
    Ok(Some(token.to_string()))
}

fn extract_subprotocol_bearer(headers: &HeaderMap) -> Result<Option<String>, Status> {
    let Some(value) = headers.get(header::SEC_WEBSOCKET_PROTOCOL) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| {
        Status::invalid_argument("Firestore Listen WebSocket protocols header is invalid.")
    })?;
    let Some(encoded) = value.split(',').find_map(|entry| {
        entry
            .trim()
            .strip_prefix(FIRESTORE_LISTEN_AUTH_SUBPROTOCOL_PREFIX)
            .map(str::to_string)
    }) else {
        return Ok(None);
    };
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
        Status::invalid_argument("Firestore Listen WebSocket auth offer must be base64url encoded.")
    })?;
    let token = String::from_utf8(bytes).map_err(|_| {
        Status::invalid_argument("Firestore Listen WebSocket auth offer must decode to UTF-8 text.")
    })?;
    if token.is_empty() {
        return Err(Status::invalid_argument(
            "Firestore Listen WebSocket auth offer is missing a token.",
        ));
    }
    Ok(Some(token))
}

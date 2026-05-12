use std::collections::BTreeMap;
use std::io;
use std::sync::RwLock;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::audit::{LocalServerAuditEvent, LocalServerAuditLog};
use super::paths::LocalServerPaths;
use super::token::{
    LocalAdminTokenRecord, generate_local_admin_token, with_token_file_lock,
    write_local_admin_token_file,
};

pub(crate) const LOCAL_SESSION_COOKIE_NAME: &str = "nimbus_session";
const LOCAL_SESSION_COOKIE_VERSION: &str = "v1";
const LOCAL_SESSION_ID_PREFIX: &str = "sess_";
const LOCAL_LAUNCH_TICKET_PREFIX: &str = "nimbus_lt_";
const DEFAULT_SESSION_TTL: Duration = Duration::hours(12);
const DEFAULT_LAUNCH_TICKET_TTL: Duration = Duration::seconds(60);

#[derive(Debug)]
pub struct LocalServerSecurityState {
    paths: LocalServerPaths,
    audit_log: LocalServerAuditLog,
    inner: RwLock<LocalServerSecurityInner>,
}

#[derive(Debug, Clone)]
struct LocalServerSecurityInner {
    token: LocalAdminTokenRecord,
    sessions: BTreeMap<String, StoredSession>,
    launch_tickets: BTreeMap<String, StoredLaunchTicket>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct StoredSession {
    generation: u64,
    issued_at: OffsetDateTime,
    expires_at: OffsetDateTime,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct StoredLaunchTicket {
    expires_at: OffsetDateTime,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct IssuedSessionCookie {
    pub session_id: String,
    pub generation: u64,
    pub issued_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub value: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SessionValidationResult {
    Authorized(AuthorizedSession),
    Missing,
    Invalid,
    Expired,
    Revoked,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthorizedSession {
    pub session_id: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct LocalAdminTokenRotation {
    pub(crate) token: LocalAdminTokenRecord,
    pub(crate) invalidated_sessions: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SessionBootstrapFailure {
    InvalidToken,
    InvalidLaunchTicket,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionCookiePayload {
    session_id: String,
    generation: u64,
    issued_at: String,
    expires_at: String,
}

impl LocalServerSecurityState {
    pub fn new(paths: LocalServerPaths, token: LocalAdminTokenRecord) -> Self {
        Self {
            audit_log: LocalServerAuditLog::new(&paths),
            paths,
            inner: RwLock::new(LocalServerSecurityInner {
                token,
                sessions: BTreeMap::new(),
                launch_tickets: BTreeMap::new(),
            }),
        }
    }

    pub fn paths(&self) -> &LocalServerPaths {
        &self.paths
    }

    pub fn current_token(&self) -> LocalAdminTokenRecord {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .token
            .clone()
    }

    pub fn authorize_bearer(&self, bearer: &str) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .token
            .authorize(bearer)
    }

    pub(crate) fn record_audit_event(&self, event: LocalServerAuditEvent) -> io::Result<()> {
        self.audit_log.append(&self.paths, event)
    }

    pub fn create_session_for_local_admin_token(
        &self,
        bearer: &str,
    ) -> Result<IssuedSessionCookie, SessionBootstrapFailure> {
        if !self.authorize_bearer(bearer) {
            return Err(SessionBootstrapFailure::InvalidToken);
        }
        Ok(self.create_session(DEFAULT_SESSION_TTL))
    }

    pub fn mint_launch_ticket(&self) -> io::Result<String> {
        let now = OffsetDateTime::now_utc();
        let expires_at = now + DEFAULT_LAUNCH_TICKET_TTL;
        let ticket = generate_prefixed_token(LOCAL_LAUNCH_TICKET_PREFIX)?;
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        prune_expired_state(&mut guard, now);
        guard
            .launch_tickets
            .insert(ticket.clone(), StoredLaunchTicket { expires_at });
        Ok(ticket)
    }

    pub fn create_session_for_launch_ticket(
        &self,
        launch_ticket: &str,
    ) -> Result<IssuedSessionCookie, SessionBootstrapFailure> {
        let now = OffsetDateTime::now_utc();
        {
            let mut guard = self
                .inner
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            prune_expired_state(&mut guard, now);
            let Some(ticket) = guard.launch_tickets.remove(launch_ticket) else {
                return Err(SessionBootstrapFailure::InvalidLaunchTicket);
            };
            if ticket.expires_at <= now {
                return Err(SessionBootstrapFailure::InvalidLaunchTicket);
            }
        }
        Ok(self.create_session(DEFAULT_SESSION_TTL))
    }

    pub fn authorize_session_cookie(&self, cookie_value: Option<&str>) -> SessionValidationResult {
        let Some(cookie_value) = cookie_value else {
            return SessionValidationResult::Missing;
        };
        let Ok((payload_segment, signature_segment, payload)) = parse_session_cookie(cookie_value)
        else {
            return SessionValidationResult::Invalid;
        };

        let now = OffsetDateTime::now_utc();
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        prune_expired_state(&mut guard, now);

        if payload.generation != guard.token.generation {
            return SessionValidationResult::Revoked;
        }
        let Some(stored) = guard.sessions.get(&payload.session_id) else {
            return SessionValidationResult::Invalid;
        };
        if stored.generation != guard.token.generation {
            return SessionValidationResult::Revoked;
        }
        if stored.expires_at <= now {
            return SessionValidationResult::Expired;
        }
        if !verify_session_signature(&guard.token, payload_segment, signature_segment) {
            return SessionValidationResult::Invalid;
        }
        SessionValidationResult::Authorized(AuthorizedSession {
            session_id: payload.session_id,
            generation: payload.generation,
        })
    }

    pub fn rotate_and_persist_token(&self) -> io::Result<LocalAdminTokenRecord> {
        Ok(self.rotate_and_persist_token_with_outcome()?.token)
    }

    pub(crate) fn rotate_and_persist_token_with_outcome(
        &self,
    ) -> io::Result<LocalAdminTokenRotation> {
        with_token_file_lock(&self.paths, || {
            let (previous_inner, rotated, invalidated_sessions) = {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let previous = guard.clone();
                let invalidated_sessions = guard.sessions.len();
                let rotated =
                    generate_local_admin_token(previous.token.generation.saturating_add(1))?;
                guard.token = rotated.clone();
                guard.sessions.clear();
                guard.launch_tickets.clear();
                (previous, rotated, invalidated_sessions)
            };

            if let Err(error) = write_local_admin_token_file(&self.paths.auth_token_path, &rotated)
            {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *guard = previous_inner;
                return Err(error);
            }
            Ok(LocalAdminTokenRotation {
                token: rotated,
                invalidated_sessions,
            })
        })
    }

    fn create_session(&self, ttl: Duration) -> IssuedSessionCookie {
        let now = OffsetDateTime::now_utc();
        let expires_at = now + ttl;
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        prune_expired_state(&mut guard, now);
        let session_id =
            generate_prefixed_token(LOCAL_SESSION_ID_PREFIX).expect("session id should generate");
        let issued_at_string = now
            .format(&Rfc3339)
            .expect("session issue time should format");
        let expires_at_string = expires_at
            .format(&Rfc3339)
            .expect("session expiry time should format");
        let payload = SessionCookiePayload {
            session_id: session_id.clone(),
            generation: guard.token.generation,
            issued_at: issued_at_string,
            expires_at: expires_at_string,
        };
        let value = sign_session_cookie(&guard.token, &payload)
            .expect("session cookie payload should serialize");
        let generation = guard.token.generation;
        guard.sessions.insert(
            session_id.clone(),
            StoredSession {
                generation,
                issued_at: now,
                expires_at,
            },
        );
        IssuedSessionCookie {
            session_id,
            generation,
            issued_at: now,
            expires_at,
            value,
        }
    }

    #[cfg(test)]
    pub(crate) fn register_session_for_test(&self, session_id: &str) {
        let now = OffsetDateTime::now_utc();
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let generation = guard.token.generation;
        guard.sessions.insert(
            session_id.to_string(),
            StoredSession {
                generation,
                issued_at: now,
                expires_at: now + DEFAULT_SESSION_TTL,
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn active_session_count(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .sessions
            .len()
    }
}

fn prune_expired_state(inner: &mut LocalServerSecurityInner, now: OffsetDateTime) {
    inner.sessions.retain(|_, session| session.expires_at > now);
    inner
        .launch_tickets
        .retain(|_, ticket| ticket.expires_at > now);
}

fn sign_session_cookie(
    token: &LocalAdminTokenRecord,
    payload: &SessionCookiePayload,
) -> io::Result<String> {
    let payload_json = serde_json::to_vec(payload).map_err(|error| {
        io::Error::other(format!(
            "failed to serialize session cookie payload: {error}"
        ))
    })?;
    let payload_segment = URL_SAFE_NO_PAD.encode(payload_json);
    let key = session_signing_key(token);
    let signature = hmac::sign(&key, payload_segment.as_bytes());
    Ok(format!(
        "{LOCAL_SESSION_COOKIE_VERSION}.{payload_segment}.{}",
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}

fn parse_session_cookie(cookie_value: &str) -> io::Result<(&str, &str, SessionCookiePayload)> {
    let mut parts = cookie_value.split('.');
    let Some(version) = parts.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session cookie is missing the version",
        ));
    };
    if version != LOCAL_SESSION_COOKIE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session cookie version is unsupported",
        ));
    }
    let Some(payload_segment) = parts.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session cookie is missing the payload",
        ));
    };
    let Some(signature_segment) = parts.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session cookie is missing the signature",
        ));
    };
    if parts.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session cookie contains too many parts",
        ));
    }
    let payload_json = URL_SAFE_NO_PAD.decode(payload_segment).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("session cookie payload is not valid base64url: {error}"),
        )
    })?;
    let payload =
        serde_json::from_slice::<SessionCookiePayload>(&payload_json).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("session cookie payload is not valid JSON: {error}"),
            )
        })?;
    Ok((payload_segment, signature_segment, payload))
}

fn verify_session_signature(
    token: &LocalAdminTokenRecord,
    payload_segment: &str,
    signature_segment: &str,
) -> bool {
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature_segment) else {
        return false;
    };
    let key = session_signing_key(token);
    hmac::verify(&key, payload_segment.as_bytes(), &signature).is_ok()
}

fn session_signing_key(token: &LocalAdminTokenRecord) -> hmac::Key {
    hmac::Key::new(hmac::HMAC_SHA256, token.token.as_bytes())
}

fn generate_prefixed_token(prefix: &str) -> io::Result<String> {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .map_err(|_| io::Error::other("failed to generate random bytes"))?;
    Ok(format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_server::token::{load_local_admin_token, load_or_create_local_admin_token};

    fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    #[test]
    fn live_rotation_clears_sessions_and_persists_new_generation() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let token = load_or_create_local_admin_token(&paths).expect("token should exist");
        let state = LocalServerSecurityState::new(paths.clone(), token.clone());
        state.register_session_for_test("session-a");

        let rotated = state
            .rotate_and_persist_token_with_outcome()
            .expect("live rotation should succeed");

        assert_eq!(rotated.invalidated_sessions, 1);
        assert_eq!(rotated.token.generation, token.generation + 1);
        assert_eq!(state.current_token(), rotated.token);
        assert_eq!(state.active_session_count(), 0);
        assert_eq!(
            load_local_admin_token(&paths).expect("rotated token should persist"),
            rotated.token
        );
    }

    #[test]
    fn session_cookie_round_trips_and_revokes_on_rotation() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let token = load_or_create_local_admin_token(&paths).expect("token should exist");
        let state = LocalServerSecurityState::new(paths.clone(), token.clone());

        let session = state
            .create_session_for_local_admin_token(&token.token)
            .expect("token should create a session");
        assert!(matches!(
            state.authorize_session_cookie(Some(&session.value)),
            SessionValidationResult::Authorized(_)
        ));

        state
            .rotate_and_persist_token()
            .expect("rotation should succeed");
        assert_eq!(
            state.authorize_session_cookie(Some(&session.value)),
            SessionValidationResult::Revoked
        );
    }

    #[test]
    fn launch_ticket_is_single_use() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let token = load_or_create_local_admin_token(&paths).expect("token should exist");
        let state = LocalServerSecurityState::new(paths, token);

        let ticket = state
            .mint_launch_ticket()
            .expect("launch ticket should mint");
        let first = state
            .create_session_for_launch_ticket(&ticket)
            .expect("launch ticket should redeem");
        assert!(matches!(
            state.authorize_session_cookie(Some(&first.value)),
            SessionValidationResult::Authorized(_)
        ));
        assert_eq!(
            state
                .create_session_for_launch_ticket(&ticket)
                .expect_err("launch ticket should be single use"),
            SessionBootstrapFailure::InvalidLaunchTicket
        );
    }
}

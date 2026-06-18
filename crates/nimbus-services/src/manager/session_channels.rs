use std::collections::BTreeMap;

use nimbus_core::{Error, TenantId};

use crate::{SessionResource, SessionTargetSnapshot};

use super::clock::now_millis;
use super::types::ServiceManagerState;

pub(super) const DEFAULT_SESSION_CHANNEL_HIGH_WATERMARK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SessionChannelKey {
    pub(super) session_id: String,
    pub(super) channel: String,
}

impl SessionChannelKey {
    pub(super) fn new(session_id: &str, channel: &str) -> Self {
        Self {
            session_id: session_id.to_owned(),
            channel: channel.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionChannelHalfState {
    Open,
    HalfClosed,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionChannelAuditRecord {
    pub(super) kind: SessionChannelAuditKind,
    pub(super) reason: String,
    pub(super) at_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionChannelAuditKind {
    Opened,
    ClientHalfClosed,
    Backpressure,
    Drained,
    Disconnected,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionChannelState {
    pub(super) tenant_id: TenantId,
    pub(super) session_id: String,
    pub(super) channel: String,
    pub(super) target_generation: u64,
    pub(super) client_to_target: SessionChannelHalfState,
    pub(super) target_to_client: SessionChannelHalfState,
    pub(super) pending_target_to_client_bytes: usize,
    pub(super) high_watermark_bytes: usize,
    pub(super) audit: Vec<SessionChannelAuditRecord>,
}

impl SessionChannelState {
    fn opened(session: &SessionResource, channel: &str, high_watermark_bytes: usize) -> Self {
        let now = now_millis();
        Self {
            tenant_id: session.tenant_id.clone(),
            session_id: session.id.clone(),
            channel: channel.to_owned(),
            target_generation: target_snapshot_generation(&session.target_snapshot),
            client_to_target: SessionChannelHalfState::Open,
            target_to_client: SessionChannelHalfState::Open,
            pending_target_to_client_bytes: 0,
            high_watermark_bytes,
            audit: vec![SessionChannelAuditRecord {
                kind: SessionChannelAuditKind::Opened,
                reason: "session_open".to_owned(),
                at_millis: now,
            }],
        }
    }

    pub(super) fn ensure_target_generation(&self, current_generation: u64) -> Result<(), Error> {
        if current_generation != self.target_generation {
            return Err(Error::Conflict(format!(
                "session channel `{}` for session `{}` was opened against target generation {}, but the current target generation is {}; reopen the session before attaching the channel",
                self.channel, self.session_id, self.target_generation, current_generation
            )));
        }
        Ok(())
    }

    pub(super) fn half_close_client_write(&mut self, reason: impl Into<String>) {
        if self.client_to_target == SessionChannelHalfState::Open {
            self.client_to_target = SessionChannelHalfState::HalfClosed;
            self.audit(
                SessionChannelAuditKind::ClientHalfClosed,
                reason.into(),
                now_millis(),
            );
        }
    }

    pub(super) fn enqueue_target_to_client_bytes(&mut self, bytes: usize) -> Result<(), Error> {
        let next = self
            .pending_target_to_client_bytes
            .checked_add(bytes)
            .ok_or_else(|| {
                Error::ResourceExhausted(format!(
                    "session channel `{}` pending byte count overflowed",
                    self.channel
                ))
            })?;
        if next > self.high_watermark_bytes {
            self.audit(
                SessionChannelAuditKind::Backpressure,
                format!(
                    "pending bytes {next} would exceed high watermark {}",
                    self.high_watermark_bytes
                ),
                now_millis(),
            );
            return Err(Error::ResourceExhausted(format!(
                "session channel `{}` backpressure: pending bytes {next} exceeds high watermark {}",
                self.channel, self.high_watermark_bytes
            )));
        }
        self.pending_target_to_client_bytes = next;
        Ok(())
    }

    pub(super) fn drain_target_to_client_bytes(&mut self, bytes: usize) {
        self.pending_target_to_client_bytes =
            self.pending_target_to_client_bytes.saturating_sub(bytes);
        self.audit(
            SessionChannelAuditKind::Drained,
            format!("drained {bytes} bytes"),
            now_millis(),
        );
    }

    pub(super) fn disconnect(&mut self, reason: impl Into<String>) {
        self.client_to_target = SessionChannelHalfState::Closed;
        self.target_to_client = SessionChannelHalfState::Closed;
        self.pending_target_to_client_bytes = 0;
        self.audit(
            SessionChannelAuditKind::Disconnected,
            reason.into(),
            now_millis(),
        );
    }

    pub(super) fn close(&mut self, reason: impl Into<String>) {
        self.client_to_target = SessionChannelHalfState::Closed;
        self.target_to_client = SessionChannelHalfState::Closed;
        self.pending_target_to_client_bytes = 0;
        self.audit(SessionChannelAuditKind::Closed, reason.into(), now_millis());
    }

    fn audit(&mut self, kind: SessionChannelAuditKind, reason: String, at_millis: u64) {
        self.audit.push(SessionChannelAuditRecord {
            kind,
            reason,
            at_millis,
        });
    }
}

pub(super) fn initialize_session_channels(
    state: &mut ServiceManagerState,
    session: &SessionResource,
) -> Result<(), Error> {
    let mut channels = BTreeMap::new();
    for channel in &session.channels {
        let key = SessionChannelKey::new(&session.id, channel);
        if state.session_channels.contains_key(&key) || channels.contains_key(&key) {
            return Err(Error::Internal(format!(
                "session channel `{channel}` for session `{}` already exists",
                session.id
            )));
        }
        channels.insert(
            key,
            SessionChannelState::opened(
                session,
                channel,
                DEFAULT_SESSION_CHANNEL_HIGH_WATERMARK_BYTES,
            ),
        );
    }
    state.session_channels.extend(channels);
    Ok(())
}

pub(super) fn close_session_channels(
    state: &mut ServiceManagerState,
    session_id: &str,
    reason: &str,
) {
    for channel in state
        .session_channels
        .values_mut()
        .filter(|channel| channel.session_id == session_id)
    {
        channel.close(reason.to_owned());
    }
}

fn target_snapshot_generation(snapshot: &SessionTargetSnapshot) -> u64 {
    match snapshot {
        SessionTargetSnapshot::Service { generation, .. }
        | SessionTargetSnapshot::Sandbox { generation, .. } => *generation,
    }
}

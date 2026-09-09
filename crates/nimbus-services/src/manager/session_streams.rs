//! Session channel streams: the seam that moves channel frames between an
//! open session's target and the client that attached to it.
//!
//! Sessions and their channel bookkeeping (`session_channels`) already
//! exist; this module owns the transport-free contract for the bytes. A
//! [`SessionChannelSource`] turns a session target and a channel name into a
//! frame receiver plus an input sender. The manager validates the session,
//! fences the target generation, and keeps the input sender so a later write
//! reaches the same attachment. The server owns the wire encoding.

use std::sync::Arc;

use nimbus_core::{Error, TenantId};
use tokio::sync::mpsc;

use crate::{SessionLifecycleState, SessionResource, SessionTargetSnapshot};

use super::ServiceManager;
use super::session_channels::SessionChannelKey;

/// Bound on frames buffered between a source and the attached client.
pub const SESSION_CHANNEL_FRAME_BUFFER: usize = 64;

/// One frame a session channel yields toward the client, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionChannelFrame {
    /// Bytes the target wrote to its standard output.
    Stdout(String),
    /// Bytes the target wrote to its standard error.
    Stderr(String),
    /// The target process ended with this code.
    Exit { code: i32 },
    /// The source ended the channel with this reason.
    Closed { reason: String },
}

/// A live attachment to one session channel.
pub struct SessionChannelAttachment {
    /// Frames from the target toward the client.
    pub frames: mpsc::Receiver<SessionChannelFrame>,
    /// Client input toward the target. Dropping it ends the attachment.
    pub input: mpsc::Sender<String>,
}

/// Turns a session target and channel into a live attachment.
///
/// A backend that can run a process behind a sandbox implements this; the
/// default [`UnsupportedSessionChannelSource`] answers with a closed frame so
/// the client learns the backend cannot stream yet instead of hanging.
pub trait SessionChannelSource: Send + Sync {
    fn attach(
        &self,
        target: &SessionTargetSnapshot,
        channel: &str,
    ) -> Result<SessionChannelAttachment, Error>;
}

/// The default source: no configured backend streams channel bytes yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedSessionChannelSource;

impl SessionChannelSource for UnsupportedSessionChannelSource {
    fn attach(
        &self,
        target: &SessionTargetSnapshot,
        channel: &str,
    ) -> Result<SessionChannelAttachment, Error> {
        let backend = match target {
            SessionTargetSnapshot::Service { backend, .. }
            | SessionTargetSnapshot::Sandbox { backend, .. } => backend.as_str(),
        };
        let (frames, frames_rx) = mpsc::channel(1);
        let (input, _input_rx) = mpsc::channel(1);
        frames
            .try_send(SessionChannelFrame::Closed {
                reason: format!(
                    "backend `{backend}` does not stream channel `{channel}` bytes yet"
                ),
            })
            .map_err(|_| Error::Internal("unsupported session channel frame buffer".to_owned()))?;
        Ok(SessionChannelAttachment {
            frames: frames_rx,
            input,
        })
    }
}

/// An attached channel: the session as it was when the attachment began and
/// the frames the target yields.
pub struct SessionChannelStream {
    pub session: SessionResource,
    pub target_generation: u64,
    pub frames: mpsc::Receiver<SessionChannelFrame>,
}

impl ServiceManager {
    /// Replaces the source that streams session channel bytes.
    pub fn with_session_channel_source(mut self, source: Arc<dyn SessionChannelSource>) -> Self {
        self.session_channel_source = source;
        self
    }

    /// Attaches a client to one channel of an open session.
    ///
    /// The session must be open and own the channel, and the target must
    /// still run the generation the session was opened against. One
    /// attachment per channel is live at a time; a second attach replaces the
    /// first and ends its input.
    pub fn attach_session_channel(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        channel: &str,
    ) -> Result<SessionChannelStream, Error> {
        let session = self
            .get_session(tenant_id, session_id)
            .ok_or_else(|| Error::NotFound(format!("session `{session_id}` was not found")))?;
        if session.lifecycle_state != SessionLifecycleState::Open {
            return Err(Error::conflict(format!(
                "session `{session_id}` is {}; attach requires an open session",
                lifecycle_wire(session.lifecycle_state)
            )));
        }
        if !session.channels.iter().any(|name| name == channel) {
            return Err(Error::InvalidInput(format!(
                "session `{session_id}` was not opened with channel `{channel}`"
            )));
        }
        let target_generation = self.current_target_generation(tenant_id, &session)?;
        self.ensure_session_channel_target_generation(
            tenant_id,
            session_id,
            channel,
            target_generation,
        )?;
        let attachment = self
            .session_channel_source
            .attach(&session.target_snapshot, channel)?;
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        state.session_channel_inputs.insert(
            SessionChannelKey::new(session_id, channel),
            attachment.input,
        );
        Ok(SessionChannelStream {
            session,
            target_generation,
            frames: attachment.frames,
        })
    }

    /// Sends client input to the attached channel of an open session.
    pub async fn write_session_channel(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        channel: &str,
        data: String,
    ) -> Result<(), Error> {
        let session = self
            .get_session(tenant_id, session_id)
            .ok_or_else(|| Error::NotFound(format!("session `{session_id}` was not found")))?;
        if session.lifecycle_state != SessionLifecycleState::Open {
            return Err(Error::conflict(format!(
                "session `{session_id}` is {}; input requires an open session",
                lifecycle_wire(session.lifecycle_state)
            )));
        }
        let input = self
            .state
            .lock()
            .expect("manager lock should not be poisoned")
            .session_channel_inputs
            .get(&SessionChannelKey::new(session_id, channel))
            .cloned()
            .ok_or_else(|| {
                Error::conflict(format!(
                    "session channel `{channel}` for session `{session_id}` is not attached"
                ))
            })?;
        input.send(data).await.map_err(|_| {
            Error::conflict(format!(
                "session channel `{channel}` for session `{session_id}` no longer accepts input"
            ))
        })
    }

    /// Ends the attachment on one channel and records the disconnect.
    pub fn detach_session_channel(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        channel: &str,
        reason: impl Into<String>,
    ) -> Result<(), Error> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .session_channel_inputs
            .remove(&SessionChannelKey::new(session_id, channel));
        self.disconnect_session_channel(tenant_id, session_id, channel, reason)
    }

    fn current_target_generation(
        &self,
        tenant_id: &TenantId,
        session: &SessionResource,
    ) -> Result<u64, Error> {
        match &session.target_snapshot {
            SessionTargetSnapshot::Sandbox { id, generation, .. } => Ok(self
                .sandbox_resource_snapshot_for_tenant(tenant_id, id)?
                .and_then(|snapshot| snapshot.observation)
                .map(|observation| observation.observed_execution_generation)
                .unwrap_or(*generation)),
            SessionTargetSnapshot::Service {
                name, generation, ..
            } => Ok(self
                .service_definition_observation_for_tenant(tenant_id, name)
                .map(|observation| observation.observed_execution_generation)
                .unwrap_or(*generation)),
        }
    }
}

fn lifecycle_wire(state: SessionLifecycleState) -> &'static str {
    match state {
        SessionLifecycleState::Open => "open",
        SessionLifecycleState::Closed => "closed",
        SessionLifecycleState::Expired => "expired",
    }
}

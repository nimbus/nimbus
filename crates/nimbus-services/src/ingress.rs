//! CB4: inbound WS-upgrade ingress — the default-ALLOW layer.
//!
//! Net-new inbound path: terminate an external WebSocket, resolve which
//! tenant+service instance it addresses, and drive the broker (CB1) +
//! per-frame invoker (CB2). This is a SEPARATE layer from egress: ingress and
//! the isolate's reply on an accepted socket are **default-ALLOW** (writes to
//! an already-accepted socket are not `check_net`-gated), whereas outbound
//! egress is default-NONE (CB5/CB6). The only ingress gate today is an
//! operator opt-in for non-loopback exposure (loopback-default), so a fresh
//! node never accepts public WS by accident.
//!
//! This module is transport-agnostic on purpose: it owns resolution, policy,
//! and broker/invoker wiring, not socket bytes. The axum WS-upgrade handler in
//! `nimbus-server` fills [`UpgradeRequest`] and pumps frames through
//! [`WsIngress`]; it deliberately does NOT reuse `nimbus-server/src/ws/`,
//! which is the Convex `nimbus.v2` sync protocol — a different transport.

use crate::broker::{ConnId, ConnectionBroker, HostFrame, InstanceKey, Residency};
use crate::frame::{FrameHandler, FrameInput, FrameInvoker, FrameState};

/// A transport-agnostic inbound upgrade request. The server's WS layer fills
/// this from the HTTP upgrade; CB4 never sees raw sockets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeRequest {
    /// Request path (the server routes tenant/service out of this).
    pub path: String,
    /// Host header (used for host-based routing).
    pub host: String,
    /// Whether the peer is loopback (drives the operator exposure gate).
    pub is_loopback: bool,
}

/// The instance an upgrade addresses, plus its residency class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIngress {
    pub key: InstanceKey,
    pub residency: Residency,
}

/// Resolves an upgrade to its instance. A seam (like CB1's `PlacementLookup`
/// and CB2's `FrameHandler`): the server supplies the tenant/service routing.
pub trait IngressResolver: Send + Sync {
    fn resolve(&self, request: &UpgradeRequest) -> Result<ResolvedIngress, IngressError>;
}

/// Ingress errors are host-side; they never carry socket payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngressError {
    /// Policy refused the upgrade (e.g. non-loopback without operator opt-in).
    Denied(String),
    /// The upgrade did not resolve to a known instance.
    Unresolved(String),
    /// Broker/invoker lifecycle error.
    Broker(String),
}

impl IngressError {
    fn denied(reason: impl Into<String>) -> Self {
        Self::Denied(reason.into())
    }
}

impl std::fmt::Display for IngressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(m) => write!(f, "ingress denied: {m}"),
            Self::Unresolved(m) => write!(f, "ingress unresolved: {m}"),
            Self::Broker(m) => write!(f, "ingress broker error: {m}"),
        }
    }
}

impl std::error::Error for IngressError {}

impl From<crate::broker::BrokerError> for IngressError {
    fn from(error: crate::broker::BrokerError) -> Self {
        Self::Broker(error.message)
    }
}

/// The default-ALLOW ingress policy layer. Distinct from egress: the only gate
/// is an operator opt-in for non-loopback exposure.
#[derive(Debug, Clone, Copy)]
pub struct IngressPolicy {
    allow_non_loopback: bool,
}

impl IngressPolicy {
    /// Loopback-only exposure (the default a fresh node ships with).
    pub fn loopback_only() -> Self {
        Self {
            allow_non_loopback: false,
        }
    }

    /// Operator has opted into non-loopback (public) WS ingress.
    pub fn allow_non_loopback() -> Self {
        Self {
            allow_non_loopback: true,
        }
    }

    /// Authorize an upgrade. Default-ALLOW; the sole refusal is non-loopback
    /// exposure without operator opt-in.
    pub fn authorize(&self, request: &UpgradeRequest) -> Result<(), IngressError> {
        if !request.is_loopback && !self.allow_non_loopback {
            return Err(IngressError::denied(
                "non-loopback WS ingress requires operator opt-in (loopback-default)",
            ));
        }
        Ok(())
    }
}

/// A held ingress connection: the broker connection id, the instance it
/// serves, and the outbound frame receiver (the transport task drains it onto
/// the real socket).
#[derive(Debug)]
pub struct AcceptedIngress {
    pub conn: ConnId,
    pub key: InstanceKey,
    pub residency: Residency,
    pub outbound: tokio::sync::mpsc::Receiver<HostFrame>,
}

/// CB4 ingress driver: policy + resolution + broker + per-frame invoke.
pub struct WsIngress<R: IngressResolver, H: FrameHandler> {
    resolver: R,
    policy: IngressPolicy,
    broker: ConnectionBroker,
    invoker: FrameInvoker<H>,
}

impl<R: IngressResolver, H: FrameHandler> WsIngress<R, H> {
    pub fn new(resolver: R, policy: IngressPolicy, invoker: FrameInvoker<H>) -> Self {
        Self {
            resolver,
            policy,
            broker: ConnectionBroker::new(),
            invoker,
        }
    }

    /// The underlying broker (for placement/epoch inspection).
    pub fn broker(&self) -> &ConnectionBroker {
        &self.broker
    }

    /// Accept an inbound WS upgrade: authorize (default-ALLOW), resolve the
    /// instance, register the held connection, and stamp a fresh activation
    /// epoch. Returns the connection handle + the outbound receiver.
    ///
    /// Fail-closed ordering: policy first, then resolution, then registration
    /// — a denied or unresolved upgrade never touches the broker registry.
    pub fn accept(
        &self,
        request: &UpgradeRequest,
        outbound_capacity: usize,
    ) -> Result<AcceptedIngress, IngressError> {
        self.policy.authorize(request)?;
        let resolved = self.resolver.resolve(request)?;
        let (conn, outbound) =
            self.broker
                .register(resolved.key.clone(), resolved.residency, outbound_capacity)?;
        // A new accepted connection is a fresh activation for the instance.
        self.broker.bump_epoch(&resolved.key)?;
        Ok(AcceptedIngress {
            conn,
            key: resolved.key,
            residency: resolved.residency,
            outbound,
        })
    }

    /// Drive one inbound frame: invoke the isolate (per-frame, CB2) and push
    /// each outbound frame back down the held connection. Returns the
    /// instance's new state (CB3 persists it). Fail-closed: an invoke or send
    /// error releases the connection and propagates.
    pub async fn on_inbound(
        &self,
        accepted: &AcceptedIngress,
        inbound: HostFrame,
        state: FrameState,
    ) -> Result<FrameState, IngressError> {
        let invocation = match self.invoker.per_frame_invoke(
            &accepted.key,
            accepted.residency,
            FrameInput { inbound, state },
        ) {
            Ok(invocation) => invocation,
            Err(error) => {
                let _ = self.broker.release(accepted.conn);
                return Err(error.into());
            }
        };
        for frame in invocation.output.outbound {
            if let Err(error) = self.broker.send(accepted.conn, frame).await {
                let _ = self.broker.release(accepted.conn);
                return Err(error.into());
            }
        }
        Ok(invocation.output.state)
    }

    /// Release a held ingress connection (socket closed).
    pub fn close(&self, accepted: &AcceptedIngress) -> Result<(), IngressError> {
        self.broker.release(accepted.conn)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::BrokerError;
    use crate::frame::{FrameHandler, FrameOutput};
    use nimbus_core::TenantId;

    /// Resolver: path `/ws/<instance>` → instance under tenant-a/chat.
    struct PathResolver;
    impl IngressResolver for PathResolver {
        fn resolve(&self, request: &UpgradeRequest) -> Result<ResolvedIngress, IngressError> {
            let instance = request
                .path
                .strip_prefix("/ws/")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    IngressError::Unresolved(format!("no instance in {}", request.path))
                })?;
            Ok(ResolvedIngress {
                key: InstanceKey::new(TenantId::new("tenant-a").expect("tenant"), "chat", instance),
                residency: Residency::Hibernated,
            })
        }
    }

    /// Echo handler: replies with the inbound text, uppercased.
    struct EchoHandler;
    impl FrameHandler for EchoHandler {
        fn invoke(
            &self,
            _key: &InstanceKey,
            input: FrameInput,
        ) -> Result<FrameOutput, BrokerError> {
            let reply = match input.inbound {
                HostFrame::Text(t) => HostFrame::Text(t.to_uppercase()),
                other => other,
            };
            Ok(FrameOutput {
                outbound: vec![reply],
                state: input.state,
            })
        }
    }

    fn ingress(policy: IngressPolicy) -> WsIngress<PathResolver, EchoHandler> {
        WsIngress::new(PathResolver, policy, FrameInvoker::new(EchoHandler, 16))
    }

    fn loopback(path: &str) -> UpgradeRequest {
        UpgradeRequest {
            path: path.to_owned(),
            host: "localhost".to_owned(),
            is_loopback: true,
        }
    }

    #[tokio::test]
    async fn default_allow_loopback_accept_and_frame_round_trip() {
        let ingress = ingress(IngressPolicy::loopback_only());
        let mut accepted = ingress
            .accept(&loopback("/ws/room-1"), 8)
            .expect("loopback upgrade is default-allowed");
        assert_eq!(accepted.key.instance_id(), "room-1");
        // A fresh accept stamped epoch 1.
        assert_eq!(ingress.broker().epoch(&accepted.key).unwrap(), 1);

        let state = ingress
            .on_inbound(&accepted, HostFrame::Text("hello".into()), vec![])
            .await
            .expect("drive frame");
        assert_eq!(state, Vec::<u8>::new());
        assert_eq!(
            accepted.outbound.recv().await,
            Some(HostFrame::Text("HELLO".into())),
            "the isolate's reply is pushed back down the held connection"
        );
    }

    #[tokio::test]
    async fn non_loopback_denied_by_default_allowed_under_operator_optin() {
        let public = UpgradeRequest {
            path: "/ws/room-1".to_owned(),
            host: "example.com".to_owned(),
            is_loopback: false,
        };

        let err = ingress(IngressPolicy::loopback_only())
            .accept(&public, 8)
            .expect_err("non-loopback denied by default");
        assert!(matches!(err, IngressError::Denied(_)));

        let accepted = ingress(IngressPolicy::allow_non_loopback())
            .accept(&public, 8)
            .expect("operator opt-in allows non-loopback");
        assert_eq!(accepted.key.instance_id(), "room-1");
    }

    #[tokio::test]
    async fn unresolved_upgrade_never_registers_a_connection() {
        let ingress = ingress(IngressPolicy::loopback_only());
        let err = ingress
            .accept(&loopback("/bogus"), 8)
            .expect_err("no instance in path");
        assert!(matches!(err, IngressError::Unresolved(_)));
        // Fail-closed: nothing was registered.
        let key = InstanceKey::new(TenantId::new("tenant-a").unwrap(), "chat", "room-1");
        assert!(ingress.broker().connections_for(&key).unwrap().is_empty());
    }

    #[tokio::test]
    async fn policy_is_checked_before_resolution() {
        // A non-loopback upgrade with an unresolvable path is DENIED (policy),
        // not Unresolved — policy gates before resolution touches routing.
        let public_bogus = UpgradeRequest {
            path: "/bogus".to_owned(),
            host: "example.com".to_owned(),
            is_loopback: false,
        };
        let err = ingress(IngressPolicy::loopback_only())
            .accept(&public_bogus, 8)
            .expect_err("denied");
        assert!(
            matches!(err, IngressError::Denied(_)),
            "policy must gate before resolution: {err}"
        );
    }
}

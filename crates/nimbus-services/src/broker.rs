//! CB1: the connection-broker substrate — host-owned connection residency.
//!
//! Long-lived sockets and channels are held by the HOST, never inside an
//! isolate's resource table: the isolate can be evicted between frames while
//! the host keeps the socket alive. This module lands the substrate only
//! (registry + residency states + placement seam); per-frame isolate
//! invocation is CB2, hibernation persistence is CB3, ingress/egress binding
//! are CB4/CB5.
//!
//! Day-one HS-shaped seams (deliberate, per the plan's HS5 amendment):
//! - The placement key is the INSTANCE ([`InstanceKey`]: tenant + namespace +
//!   instance id — a session/DO identity), never the bare tenant.
//! - Placement lookup sits behind the [`PlacementLookup`] trait
//!   (`ClusterTransport`-shaped); the single-node default resolves to self.
//! - Every instance carries an epoch stamp ([`ConnectionBroker::epoch`]);
//!   wakes are gated on it so a stale activation can be fenced when
//!   placement moves (single-activation discipline, enforced fully in CB3+).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::TenantId;
use tokio::sync::mpsc;

/// Where an instance's connection state lives between frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Residency {
    /// Isolate evicted between frames; host holds the socket, serialized
    /// state rehydrates on wake (the beats-Vercel cost class).
    Hibernated,
    /// Isolate stays warm (CDP sessions, app WS servers — cannot evict).
    Resident,
}

/// The placement identity: a session/DO instance, NOT a tenant. Keying
/// placement on the instance is what lets HS5 later shard instances of one
/// tenant across nodes without re-cutting this seam.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceKey {
    tenant: TenantId,
    namespace: String,
    instance_id: String,
}

impl InstanceKey {
    pub fn new(
        tenant: TenantId,
        namespace: impl Into<String>,
        instance_id: impl Into<String>,
    ) -> Self {
        Self {
            tenant,
            namespace: namespace.into(),
            instance_id: instance_id.into(),
        }
    }

    pub fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
}

/// Host-side handle to one held connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnId(u64);

impl ConnId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A frame the host pushes down a held connection (the `ws_commands` lane:
/// the isolate never owns the socket; it asks the host to send).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostFrame {
    Text(String),
    Binary(Vec<u8>),
    Close { code: u16, reason: String },
}

/// Where an instance is placed. Single-node resolves to self; HS5 replaces
/// the implementation, not the seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    SelfNode,
    Remote(String),
}

/// `ClusterTransport`-shaped placement seam (resolve-to-self in v1).
pub trait PlacementLookup: Send + Sync {
    fn place(&self, key: &InstanceKey) -> Placement;
}

/// Single-node placement: every instance lives here.
pub struct ResolveToSelf;

impl PlacementLookup for ResolveToSelf {
    fn place(&self, _key: &InstanceKey) -> Placement {
        Placement::SelfNode
    }
}

struct ConnectionEntry {
    key: InstanceKey,
    sender: mpsc::Sender<HostFrame>,
    residency: Residency,
}

/// The host-owned connection registry: the socket map lives OUTSIDE any
/// isolate resource table, keyed by [`ConnId`] with per-instance grouping.
pub struct ConnectionRegistry {
    connections: Mutex<HashMap<ConnId, ConnectionEntry>>,
    next_id: AtomicU64,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<ConnId, ConnectionEntry>>, BrokerError> {
        self.connections.lock().map_err(|_| BrokerError {
            message: "connection registry lock is poisoned".to_owned(),
        })
    }
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Broker errors are host-side lifecycle errors; they never carry socket
/// payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerError {
    pub message: String,
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BrokerError {}

/// The CB1 substrate: registry + residency + placement + epoch fencing.
pub struct ConnectionBroker {
    registry: ConnectionRegistry,
    placement: Arc<dyn PlacementLookup>,
    /// Per-instance epoch stamps (monotonic; bumped on every wake). CB3 uses
    /// the stamp to fence stale activations against persisted state; CB8
    /// hands the fence to the cluster lease. Laying the field now is the
    /// day-one seam.
    epochs: Mutex<HashMap<InstanceKey, u64>>,
}

impl ConnectionBroker {
    /// Single-node broker (placement resolves to self).
    pub fn new() -> Self {
        Self::with_placement(Arc::new(ResolveToSelf))
    }

    pub fn with_placement(placement: Arc<dyn PlacementLookup>) -> Self {
        Self {
            registry: ConnectionRegistry::new(),
            placement,
            epochs: Mutex::new(HashMap::new()),
        }
    }

    /// Where `key`'s instance is placed (resolve-to-self on single node).
    pub fn place(&self, key: &InstanceKey) -> Placement {
        self.placement.place(key)
    }

    /// Register a held connection for `key` with the given residency class.
    /// Returns the host-side id plus the receiver end of the `ws_commands`
    /// lane (the transport task drains it onto the real socket).
    pub fn register(
        &self,
        key: InstanceKey,
        residency: Residency,
        outbound_capacity: usize,
    ) -> Result<(ConnId, mpsc::Receiver<HostFrame>), BrokerError> {
        let (sender, receiver) = mpsc::channel(outbound_capacity.max(1));
        let id = ConnId(self.registry.next_id.fetch_add(1, Ordering::Relaxed));
        let mut connections = self.registry.lock()?;
        connections.insert(
            id,
            ConnectionEntry {
                key,
                sender,
                residency,
            },
        );
        Ok((id, receiver))
    }

    /// Push a frame down a held connection (host → socket lane). Fails
    /// closed if the connection is gone or its transport lane is full or
    /// closed — the broker never silently drops a frame.
    pub async fn send(&self, conn: ConnId, frame: HostFrame) -> Result<(), BrokerError> {
        let sender = {
            let connections = self.registry.lock()?;
            let entry = connections.get(&conn).ok_or_else(|| BrokerError {
                message: format!("connection {} is not held by this broker", conn.get()),
            })?;
            entry.sender.clone()
        };
        sender.send(frame).await.map_err(|_| BrokerError {
            message: format!(
                "connection {}'s transport lane is closed (socket task gone)",
                conn.get()
            ),
        })
    }

    /// The residency class of a held connection.
    pub fn residency(&self, conn: ConnId) -> Result<Residency, BrokerError> {
        let connections = self.registry.lock()?;
        connections
            .get(&conn)
            .map(|entry| entry.residency)
            .ok_or_else(|| BrokerError {
                message: format!("connection {} is not held by this broker", conn.get()),
            })
    }

    /// Reclassify a held connection (e.g. a handler pin forces Resident).
    pub fn set_residency(&self, conn: ConnId, residency: Residency) -> Result<(), BrokerError> {
        let mut connections = self.registry.lock()?;
        let entry = connections.get_mut(&conn).ok_or_else(|| BrokerError {
            message: format!("connection {} is not held by this broker", conn.get()),
        })?;
        entry.residency = residency;
        Ok(())
    }

    /// All held connections for an instance (fan-out order unspecified).
    pub fn connections_for(&self, key: &InstanceKey) -> Result<Vec<ConnId>, BrokerError> {
        let connections = self.registry.lock()?;
        let mut ids: Vec<ConnId> = connections
            .iter()
            .filter(|(_, entry)| &entry.key == key)
            .map(|(id, _)| *id)
            .collect();
        ids.sort_by_key(|id| id.get());
        Ok(ids)
    }

    /// Release a held connection (socket closed or instance torn down).
    pub fn release(&self, conn: ConnId) -> Result<(), BrokerError> {
        let mut connections = self.registry.lock()?;
        connections.remove(&conn);
        Ok(())
    }

    /// Bump and return the instance's epoch (called on every wake). A stale
    /// activation holding an older epoch must not win against state stamped
    /// with a newer one — CB3 enforces this against persisted state; the
    /// stamp discipline starts here.
    pub fn bump_epoch(&self, key: &InstanceKey) -> Result<u64, BrokerError> {
        let mut epochs = self.epochs.lock().map_err(|_| BrokerError {
            message: "broker epoch map lock is poisoned".to_owned(),
        })?;
        let epoch = epochs.entry(key.clone()).or_insert(0);
        *epoch += 1;
        Ok(*epoch)
    }

    /// The instance's current epoch (0 = never woken).
    pub fn epoch(&self, key: &InstanceKey) -> Result<u64, BrokerError> {
        let epochs = self.epochs.lock().map_err(|_| BrokerError {
            message: "broker epoch map lock is poisoned".to_owned(),
        })?;
        Ok(epochs.get(key).copied().unwrap_or(0))
    }
}

impl Default for ConnectionBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(instance: &str) -> InstanceKey {
        InstanceKey::new(
            TenantId::new("tenant-a").expect("test tenant"),
            "chat",
            instance,
        )
    }

    #[tokio::test]
    async fn register_send_drain_round_trip() {
        let broker = ConnectionBroker::new();
        let (conn, mut receiver) = broker
            .register(key("room-1"), Residency::Hibernated, 8)
            .expect("register");

        broker
            .send(conn, HostFrame::Text("hello".into()))
            .await
            .expect("send");
        broker
            .send(conn, HostFrame::Binary(vec![1, 2, 3]))
            .await
            .expect("send binary");

        assert_eq!(receiver.recv().await, Some(HostFrame::Text("hello".into())));
        assert_eq!(
            receiver.recv().await,
            Some(HostFrame::Binary(vec![1, 2, 3])),
            "frames drain in order on the ws_commands lane"
        );
    }

    #[tokio::test]
    async fn send_fails_closed_when_connection_unknown_or_lane_closed() {
        let broker = ConnectionBroker::new();
        let err = broker
            .send(ConnId(999), HostFrame::Text("x".into()))
            .await
            .expect_err("unknown connection");
        assert!(err.message.contains("not held"));

        let (conn, receiver) = broker
            .register(key("room-1"), Residency::Resident, 1)
            .expect("register");
        drop(receiver); // transport task gone
        let err = broker
            .send(conn, HostFrame::Text("x".into()))
            .await
            .expect_err("closed lane");
        assert!(
            err.message.contains("closed"),
            "a dead transport lane must surface, never silently drop: {err}"
        );
    }

    #[tokio::test]
    async fn residency_classes_and_reclassification() {
        let broker = ConnectionBroker::new();
        let (conn, _rx) = broker
            .register(key("room-1"), Residency::Hibernated, 4)
            .expect("register");
        assert_eq!(broker.residency(conn).unwrap(), Residency::Hibernated);

        broker
            .set_residency(conn, Residency::Resident)
            .expect("pin resident");
        assert_eq!(
            broker.residency(conn).unwrap(),
            Residency::Resident,
            "a handler pin forces Resident"
        );
    }

    #[tokio::test]
    async fn instance_fanout_groups_connections_by_key_not_tenant() {
        let broker = ConnectionBroker::new();
        let (a1, _r1) = broker
            .register(key("room-1"), Residency::Hibernated, 4)
            .unwrap();
        let (a2, _r2) = broker
            .register(key("room-1"), Residency::Hibernated, 4)
            .unwrap();
        let (_b1, _r3) = broker
            .register(key("room-2"), Residency::Hibernated, 4)
            .unwrap();

        let room1 = broker.connections_for(&key("room-1")).unwrap();
        assert_eq!(
            room1,
            vec![a1, a2],
            "fan-out is per INSTANCE (same tenant, different room excluded)"
        );

        broker.release(a1).unwrap();
        assert_eq!(broker.connections_for(&key("room-1")).unwrap(), vec![a2]);
    }

    #[tokio::test]
    async fn placement_resolves_to_self_on_single_node() {
        let broker = ConnectionBroker::new();
        assert_eq!(
            broker.place(&key("anything")),
            Placement::SelfNode,
            "resolve_to_self is the single-node placement seam"
        );
    }

    #[tokio::test]
    async fn epoch_stamps_are_per_instance_and_monotonic() {
        let broker = ConnectionBroker::new();
        assert_eq!(broker.epoch(&key("room-1")).unwrap(), 0, "never woken");
        assert_eq!(broker.bump_epoch(&key("room-1")).unwrap(), 1);
        assert_eq!(broker.bump_epoch(&key("room-1")).unwrap(), 2);
        assert_eq!(
            broker.epoch(&key("room-2")).unwrap(),
            0,
            "epochs are per instance — one room's wakes never fence another"
        );
    }
}

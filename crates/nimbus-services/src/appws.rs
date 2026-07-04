//! CB9: Resident app-WS server + standard-`ws`/socket.io zero-config compat.
//!
//! The Vercel-parity DX surface. Promotes the Resident residency class (CB1)
//! from CDP-only to a first-class **app** surface: a host-held socket kept
//! warm, multiplexing N client connections onto ONE Resident isolate that
//! stays warm across all of them. This is the substrate a Node
//! `ws.WebSocketServer` app or a socket.io server runs on — the host owns the
//! sockets and per-frame invocation (CB2), the isolate owns the handler
//! logic.
//!
//! v1 lands the host-side substrate: the multiplexing Resident app-WS server
//! (mapping the ws.WebSocketServer connection/message/close lifecycle onto the
//! broker + invoker) and the socket.io engine.io handshake/framing helper. The
//! JS-side polyfill that makes the literal 5-line `ws` example run unmodified
//! is the runtime-integration follow-on that imports this substrate through
//! the FrameHandler seam — the acceptance test lives with that wiring.
//!
//! Inbound authz (the §11 trust-inversion risk): the Resident app surface
//! reuses the CB4 [`crate::ingress::IngressPolicy`] loopback-default gate;
//! non-loopback app-WS exposure requires operator opt-in, so promoting a
//! handler to a public app server is never accidental.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::broker::{BrokerError, ConnId, ConnectionBroker, HostFrame, InstanceKey, Residency};
use crate::frame::{FrameHandler, FrameInput, FrameInvoker};

/// A Node `ws.WebSocketServer`-shaped lifecycle event, mapped onto the broker.
/// The compat shim raises these; the app handler (in the isolate) reacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsServerEvent {
    /// A new client connected (ws: `wss.on('connection', ...)`).
    Connection { conn: ConnId },
    /// A client sent a message (ws: `socket.on('message', ...)`).
    Message { conn: ConnId, frame: HostFrame },
    /// A client disconnected (ws: `socket.on('close', ...)`).
    Close { conn: ConnId },
}

/// A Resident app-WS server for one instance: N client connections
/// multiplexed onto one warm isolate.
pub struct ResidentAppWs<H: FrameHandler> {
    instance: InstanceKey,
    broker: ConnectionBroker,
    invoker: FrameInvoker<H>,
    /// Client connections currently attached to this app instance.
    conns: Mutex<HashMap<ConnId, tokio::sync::mpsc::Receiver<HostFrame>>>,
    /// Shared per-instance state threaded across every connection's frames
    /// (the app's own state; CB3 persists it).
    state: Mutex<Vec<u8>>,
}

impl<H: FrameHandler> ResidentAppWs<H> {
    pub fn new(instance: InstanceKey, invoker: FrameInvoker<H>) -> Self {
        Self {
            instance,
            broker: ConnectionBroker::new(),
            invoker,
            conns: Mutex::new(HashMap::new()),
            state: Mutex::new(Vec::new()),
        }
    }

    pub fn broker(&self) -> &ConnectionBroker {
        &self.broker
    }

    /// Number of client connections currently multiplexed onto the instance.
    pub fn connection_count(&self) -> usize {
        self.conns.lock().map(|c| c.len()).unwrap_or(0)
    }

    /// True if the instance's isolate is warm (Resident keeps it so once the
    /// first frame has run).
    pub fn is_warm(&self) -> bool {
        self.invoker.is_warm(&self.instance)
    }

    /// A client connected: register a Resident held connection, returning the
    /// connection id. The isolate is NOT woken on connect — the ws
    /// `connection` handler runs on the first frame (keeping cold-connect
    /// cheap); the outbound receiver is held for message replies.
    pub fn connect(&self, outbound_capacity: usize) -> Result<ConnId, BrokerError> {
        let (conn, outbound) = self.broker.register(
            self.instance.clone(),
            Residency::Resident,
            outbound_capacity,
        )?;
        let mut conns = self.conns.lock().map_err(|_| poisoned())?;
        conns.insert(conn, outbound);
        Ok(conn)
    }

    /// A client sent a frame: invoke the (warm, Resident) isolate and push the
    /// replies back to THAT client connection. The isolate stays warm across
    /// connections and frames (Resident), and the shared instance state is
    /// threaded through. Returns the frames delivered to the client.
    pub async fn on_message(
        &self,
        conn: ConnId,
        frame: HostFrame,
    ) -> Result<Vec<HostFrame>, BrokerError> {
        {
            let conns = self.conns.lock().map_err(|_| poisoned())?;
            if !conns.contains_key(&conn) {
                return Err(BrokerError {
                    message: format!("connection {} is not attached to this app-ws", conn.get()),
                });
            }
        }
        let prior_state = { self.state.lock().map_err(|_| poisoned())?.clone() };
        let invocation = self.invoker.per_frame_invoke(
            &self.instance,
            Residency::Resident,
            FrameInput {
                inbound: frame,
                state: prior_state,
            },
        )?;
        {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            *state = invocation.output.state;
        }
        for out in &invocation.output.outbound {
            self.broker.send(conn, out.clone()).await?;
        }
        Ok(invocation.output.outbound)
    }

    /// A client disconnected: release its held connection. The isolate stays
    /// warm as long as any connection (or the Resident pin) remains.
    pub fn close(&self, conn: ConnId) -> Result<(), BrokerError> {
        self.broker.release(conn)?;
        let mut conns = self.conns.lock().map_err(|_| poisoned())?;
        conns.remove(&conn);
        Ok(())
    }
}

fn poisoned() -> BrokerError {
    BrokerError {
        message: "resident app-ws registry lock is poisoned".to_owned(),
    }
}

/// socket.io / engine.io zero-config compat: the minimal framing so a
/// socket.io client forcing the websocket transport handshakes and exchanges
/// events over a plain host-held WS. engine.io packet types used here:
/// `0` = OPEN (handshake), `4` = MESSAGE; socket.io packet type `2` = EVENT,
/// so an application event arrives as `42[...]` (engine `4` + socketio `2`).
pub struct SocketIoCompat;

impl SocketIoCompat {
    /// The engine.io OPEN handshake frame a socket.io server sends first. The
    /// client needs a session id, ping interval/timeout, and the empty
    /// upgrades list (already on websocket, so no further upgrade).
    pub fn open_handshake(session_id: &str) -> HostFrame {
        HostFrame::Text(format!(
            "0{{\"sid\":\"{session_id}\",\"upgrades\":[],\"pingInterval\":25000,\"pingTimeout\":20000}}"
        ))
    }

    /// Wrap an application event payload as a socket.io EVENT message
    /// (`42` + payload).
    pub fn wrap_event(payload: &str) -> HostFrame {
        HostFrame::Text(format!("42{payload}"))
    }

    /// Extract the application event payload from an inbound frame, or `None`
    /// if the frame is an engine.io control frame (ping/pong/handshake) rather
    /// than a socket.io EVENT.
    pub fn unwrap_event(frame: &HostFrame) -> Option<String> {
        let HostFrame::Text(text) = frame else {
            return None;
        };
        // engine `4` MESSAGE + socketio `2` EVENT = "42<payload>".
        text.strip_prefix("42").map(|payload| payload.to_owned())
    }

    /// Is this an engine.io PING (`2`)? The broker answers PONG (`3`) without
    /// waking the isolate — a pure host-side keepalive (the auto-response
    /// contract).
    pub fn is_ping(frame: &HostFrame) -> bool {
        matches!(frame, HostFrame::Text(t) if t == "2")
    }

    /// The engine.io PONG (`3`) reply to a PING.
    pub fn pong() -> HostFrame {
        HostFrame::Text("3".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameOutput;
    use nimbus_core::TenantId;

    fn instance() -> InstanceKey {
        InstanceKey::new(TenantId::new("tenant-a").unwrap(), "app", "chat-server")
    }

    /// Broadcast handler: replies to the sender with an ack carrying the
    /// running message count (proving shared state across connections).
    struct AckHandler;
    impl FrameHandler for AckHandler {
        fn invoke(
            &self,
            _key: &InstanceKey,
            input: FrameInput,
        ) -> Result<FrameOutput, BrokerError> {
            let count = input.state.first().copied().unwrap_or(0).wrapping_add(1);
            Ok(FrameOutput {
                outbound: vec![HostFrame::Text(format!("ack {count}"))],
                state: vec![count],
            })
        }
    }

    fn server() -> ResidentAppWs<AckHandler> {
        ResidentAppWs::new(instance(), FrameInvoker::new(AckHandler, 16))
    }

    #[tokio::test]
    async fn multiplexes_connections_onto_one_warm_resident_isolate() {
        let server = server();
        let c1 = server.connect(8).expect("connect 1");
        let c2 = server.connect(8).expect("connect 2");
        assert_eq!(server.connection_count(), 2, "two clients on one instance");
        assert!(!server.is_warm(), "not warm until the first frame runs");

        let r1 = server
            .on_message(c1, HostFrame::Text("hi".into()))
            .await
            .unwrap();
        assert_eq!(r1, vec![HostFrame::Text("ack 1".into())]);
        assert!(
            server.is_warm(),
            "Resident keeps the isolate warm after the first frame"
        );

        // Second connection's message reuses the SAME warm isolate and the
        // shared instance state (count continues to 2).
        let r2 = server
            .on_message(c2, HostFrame::Text("yo".into()))
            .await
            .unwrap();
        assert_eq!(
            r2,
            vec![HostFrame::Text("ack 2".into())],
            "both connections share one warm isolate + instance state"
        );

        server.close(c1).unwrap();
        assert_eq!(server.connection_count(), 1);
        assert!(
            server.is_warm(),
            "isolate stays warm while a connection remains"
        );
    }

    #[tokio::test]
    async fn message_to_detached_connection_fails_closed() {
        let server = server();
        let conn = server.connect(8).unwrap();
        server.close(conn).unwrap();
        let err = server
            .on_message(conn, HostFrame::Text("x".into()))
            .await
            .expect_err("closed connection is no longer attached");
        assert!(err.message.contains("not attached"));
    }

    #[test]
    fn socketio_handshake_and_event_framing_round_trips() {
        let open = SocketIoCompat::open_handshake("abc123");
        assert_eq!(
            open,
            HostFrame::Text(
                "0{\"sid\":\"abc123\",\"upgrades\":[],\"pingInterval\":25000,\"pingTimeout\":20000}"
                    .into()
            ),
            "engine.io OPEN packet type 0 with session id"
        );

        let wrapped = SocketIoCompat::wrap_event("[\"chat\",\"hello\"]");
        assert_eq!(wrapped, HostFrame::Text("42[\"chat\",\"hello\"]".into()));
        assert_eq!(
            SocketIoCompat::unwrap_event(&wrapped),
            Some("[\"chat\",\"hello\"]".to_owned()),
            "42<payload> round-trips to the application event"
        );

        // Control frames are not application events.
        assert_eq!(SocketIoCompat::unwrap_event(&SocketIoCompat::pong()), None);
        assert!(SocketIoCompat::is_ping(&HostFrame::Text("2".into())));
        assert_eq!(SocketIoCompat::pong(), HostFrame::Text("3".into()));
    }
}

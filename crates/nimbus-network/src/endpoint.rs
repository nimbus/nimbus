use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// Transport semantics of an admitted or published application endpoint.
///
/// This is protocol vocabulary, not a protocol parser or listener
/// implementation. The effect-owning crate remains responsible for binding,
/// serving, forwarding, and TLS termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointProtocol {
    /// Opaque TCP bytes.
    Tcp,
    /// Cleartext HTTP application traffic.
    Http,
    /// HTTP application traffic protected by TLS at the ingress owner.
    Https,
}

/// An actual reachable endpoint reported by an effect provider.
///
/// This address is observed location, never resource or workload identity.
/// The desired/durable/observed state model composes it with a stable
/// [`crate::PublishedEndpointId`] and [`crate::NetworkResourceGeneration`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedEndpoint {
    /// Provider-neutral endpoint label within its owning plan.
    pub name: String,
    /// Application protocol exposed by the endpoint.
    pub protocol: EndpointProtocol,
    /// Actual reachable socket address.
    pub address: SocketAddr,
    /// Guest-side port when the provider maps a different host-side port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_port: Option<u16>,
}

impl PublishedEndpoint {
    /// Construct an observed endpoint without a guest-port mapping.
    pub fn new(name: impl Into<String>, protocol: EndpointProtocol, address: SocketAddr) -> Self {
        Self {
            name: name.into(),
            protocol,
            address,
            guest_port: None,
        }
    }

    /// Record the guest-side port behind the observed address.
    pub fn with_guest_port(mut self, guest_port: u16) -> Self {
        self.guest_port = Some(guest_port);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_wire_values_are_pinned() {
        assert_eq!(
            serde_json::to_string(&EndpointProtocol::Tcp).expect("serialize TCP"),
            r#""tcp""#
        );
        assert_eq!(
            serde_json::to_string(&EndpointProtocol::Http).expect("serialize HTTP"),
            r#""http""#
        );
        assert_eq!(
            serde_json::to_string(&EndpointProtocol::Https).expect("serialize HTTPS"),
            r#""https""#
        );
        assert!(
            serde_json::from_str::<EndpointProtocol>(r#""udp""#).is_err(),
            "unknown protocols must fail closed"
        );
    }

    #[test]
    fn endpoint_without_guest_port_preserves_the_existing_wire_shape() {
        let endpoint = PublishedEndpoint::new(
            "api",
            EndpointProtocol::Http,
            "127.0.0.1:8080".parse().expect("valid endpoint"),
        );
        let json = serde_json::to_string(&endpoint).expect("serialize endpoint");

        assert_eq!(
            json,
            r#"{"name":"api","protocol":"http","address":"127.0.0.1:8080"}"#
        );
        assert_eq!(
            serde_json::from_str::<PublishedEndpoint>(&json).expect("deserialize endpoint"),
            endpoint
        );
    }

    #[test]
    fn endpoint_with_guest_port_and_ipv6_round_trips_exactly() {
        let endpoint = PublishedEndpoint::new(
            "secure-api",
            EndpointProtocol::Https,
            "[::1]:443".parse().expect("valid endpoint"),
        )
        .with_guest_port(8443);
        let json = serde_json::to_string(&endpoint).expect("serialize endpoint");

        assert_eq!(
            json,
            r#"{"name":"secure-api","protocol":"https","address":"[::1]:443","guest_port":8443}"#
        );
        assert_eq!(
            serde_json::from_str::<PublishedEndpoint>(&json).expect("deserialize endpoint"),
            endpoint
        );
    }

    #[test]
    fn omitted_or_null_guest_port_deserializes_as_none() {
        let omitted: PublishedEndpoint =
            serde_json::from_str(r#"{"name":"tcp","protocol":"tcp","address":"127.0.0.1:9000"}"#)
                .expect("deserialize omitted guest port");
        let explicit_null: PublishedEndpoint = serde_json::from_str(
            r#"{"name":"tcp","protocol":"tcp","address":"127.0.0.1:9000","guest_port":null}"#,
        )
        .expect("deserialize null guest port");

        assert_eq!(omitted.guest_port, None);
        assert_eq!(explicit_null, omitted);
    }
}

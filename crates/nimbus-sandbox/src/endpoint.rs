use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishedEndpointProtocol {
    Tcp,
    Http,
    Https,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedEndpoint {
    pub name: String,
    pub protocol: PublishedEndpointProtocol,
    pub address: SocketAddr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_port: Option<u16>,
}

impl PublishedEndpoint {
    pub fn new(
        name: impl Into<String>,
        protocol: PublishedEndpointProtocol,
        address: SocketAddr,
    ) -> Self {
        Self {
            name: name.into(),
            protocol,
            address,
            guest_port: None,
        }
    }

    pub fn with_guest_port(mut self, guest_port: u16) -> Self {
        self.guest_port = Some(guest_port);
        self
    }
}

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
        }
    }
}

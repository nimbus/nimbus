use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

pub(crate) type Resolver =
    Arc<dyn Fn(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + Sync + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsCacheConfig {
    pub max_hosts: usize,
    pub max_addresses_per_host: usize,
    /// Seam for NEG's planned bounded dynamic DNS/FQDN state: the lower TTL
    /// clamp a future DNS cache would apply. Deliberately unimplemented today —
    /// the resolver does not cache, which is the safer DNS-rebind posture — so
    /// this bound is not yet enforced.
    pub min_ttl: Duration,
    /// Seam for NEG's planned bounded dynamic DNS/FQDN state: the upper TTL
    /// clamp a future DNS cache would apply. Deliberately unimplemented today
    /// (no caching, the safer DNS-rebind posture), so this bound is not yet
    /// enforced.
    pub max_ttl: Duration,
}

impl Default for DnsCacheConfig {
    fn default() -> Self {
        Self {
            max_hosts: 1024,
            max_addresses_per_host: 16,
            min_ttl: Duration::from_secs(1),
            max_ttl: Duration::from_secs(300),
        }
    }
}

impl DnsCacheConfig {
    pub fn with_max_addresses_per_host(mut self, max_addresses_per_host: usize) -> Self {
        self.max_addresses_per_host = max_addresses_per_host;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsResolution {
    pub canonical_host: String,
    /// Seam for NEG's planned DNS alias-chain handling: the CNAME/alias chain a
    /// future resolver would walk so policy can be enforced against every name
    /// in the chain, not just the queried host. Deliberately unimplemented today
    /// — `resolve_dns` records only the canonical host as a single-element chain
    /// — so alias-chain policy is not yet enforced.
    pub alias_chain: Vec<String>,
    pub addresses: Vec<SocketAddr>,
}

pub(crate) fn resolve_socket_addrs(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    (host, port).to_socket_addrs().map(|addrs| addrs.collect())
}

pub(crate) fn resolve_dns(
    resolver: &Resolver,
    dns_cache: &DnsCacheConfig,
    host: &str,
    port: u16,
) -> io::Result<DnsResolution> {
    if dns_cache.max_hosts == 0 || dns_cache.max_addresses_per_host == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "DNS cache caps must be nonzero",
        ));
    }
    let canonical_host = host.to_ascii_lowercase();
    let addresses = resolver(&canonical_host, port)?;
    if addresses.len() > dns_cache.max_addresses_per_host {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} addresses exceeds max_addresses_per_host {}",
                addresses.len(),
                dns_cache.max_addresses_per_host
            ),
        ));
    }
    Ok(DnsResolution {
        canonical_host: canonical_host.clone(),
        alias_chain: vec![canonical_host],
        addresses,
    })
}

use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};

/// Refuse a listener bind address unless it is loopback-only.
///
/// This is a pure address-shape guard. It performs no socket I/O, so low-level
/// listener owners can call it before binding and still keep `nimbus-core` free
/// of host operations.
pub fn refuse_non_loopback_bind(bind_addr: SocketAddr) -> io::Result<()> {
    if bind_addr.ip().is_loopback() {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "refusing non-loopback bind address {bind_addr}; use 127.0.0.1 or ::1 for local dev listeners"
        ),
    ))
}

/// True when `host` is a syntactically valid DNS hostname.
///
/// Each dot-separated label must be 1..=63 ASCII alphanumerics or `-`, must not
/// start or end with `-`, the whole name must be <= 253 chars, and the name must
/// have no leading or trailing dot. This is a pure *shape* check: it never
/// resolves the name and makes no judgement about whether the name points at an
/// internal/non-global address — callers (the egress PDP, the operator-policy
/// validator) layer their own SSRF / bind-target rules on top. It is the single
/// canonical hostname validator shared by `nimbus-egress` and `nimbus-tenant`
/// so the two can no longer drift apart. (egress audit M2.)
pub fn is_valid_dns_hostname(host: &str) -> bool {
    if host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

/// Error from constructing or parsing a [`Cidr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CidrError {
    /// The string was not `a.b.c.d/prefix`.
    Malformed(String),
    /// The prefix length was outside `0..=32`.
    BadPrefix(String),
    /// The address had host bits set — it is not the network base for its prefix.
    NotNetworkBase(String),
}

impl fmt::Display for CidrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CidrError::Malformed(s) => {
                write!(f, "malformed CIDR {s:?}: expected a.b.c.d/prefix")
            }
            CidrError::BadPrefix(s) => write!(f, "invalid CIDR {s:?}: prefix must be 0..=32"),
            CidrError::NotNetworkBase(s) => write!(
                f,
                "invalid CIDR {s:?}: address must be the network base for its prefix"
            ),
        }
    }
}

impl std::error::Error for CidrError {}

/// A validated IPv4 CIDR block: a network base address plus a prefix length.
///
/// Zero-I/O pure vocabulary shared between the sandbox network allocator (the
/// consumer) and the future cluster super-net allocator (the producer): the same
/// [`Cidr::nth_subnet`] carve divides a cluster pool into per-node super-nets and
/// a per-node super-net into per-tenant subnets. IPv4 today; an IPv6-ULA variant
/// is the planned exhaustion escape hatch, so callers should treat the type as
/// address-family-generic in spirit and route all bit math through its methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    base: Ipv4Addr,
    prefix: u8,
}

impl Cidr {
    /// Build a CIDR from an explicit base + prefix. The base MUST be the network
    /// address for `prefix` (no host bits set), matching the sandbox bridge rule.
    pub fn new(base: Ipv4Addr, prefix: u8) -> Result<Self, CidrError> {
        if prefix > 32 {
            return Err(CidrError::BadPrefix(format!("{base}/{prefix}")));
        }
        if u32::from(base) & !prefix_mask(prefix) != 0 {
            return Err(CidrError::NotNetworkBase(format!("{base}/{prefix}")));
        }
        Ok(Self { base, prefix })
    }

    /// Parse `a.b.c.d/prefix`.
    pub fn parse(s: &str) -> Result<Self, CidrError> {
        let (addr, prefix) = s
            .split_once('/')
            .ok_or_else(|| CidrError::Malformed(s.to_owned()))?;
        let base = addr
            .parse::<Ipv4Addr>()
            .map_err(|_| CidrError::Malformed(s.to_owned()))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| CidrError::BadPrefix(s.to_owned()))?;
        // Re-key any construction error to the original input string.
        Self::new(base, prefix).map_err(|error| match error {
            CidrError::BadPrefix(_) => CidrError::BadPrefix(s.to_owned()),
            CidrError::NotNetworkBase(_) => CidrError::NotNetworkBase(s.to_owned()),
            CidrError::Malformed(_) => CidrError::Malformed(s.to_owned()),
        })
    }

    pub fn base(&self) -> Ipv4Addr {
        self.base
    }

    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    /// The number of addresses in the block (`2^(32-prefix)`).
    pub fn address_count(&self) -> u64 {
        1u64 << (32 - self.prefix as u32)
    }

    /// True when `addr` falls inside this block.
    pub fn contains(&self, addr: Ipv4Addr) -> bool {
        u32::from(addr) & prefix_mask(self.prefix) == u32::from(self.base)
    }

    /// True when two blocks share any address.
    pub fn overlaps(&self, other: &Cidr) -> bool {
        self.contains(other.base) || other.contains(self.base)
    }

    /// Carve the `index`-th child subnet of length `child_prefix` from this block.
    ///
    /// Returns `None` if `child_prefix` is shorter than this prefix, exceeds /32,
    /// or `index` overflows the number of children this block holds. This is the
    /// single pure sub-division used both to carve per-node super-nets from the
    /// cluster pool AND per-tenant subnets from a node super-net.
    pub fn nth_subnet(&self, child_prefix: u8, index: u64) -> Option<Cidr> {
        if child_prefix < self.prefix || child_prefix > 32 {
            return None;
        }
        let child_bits = child_prefix as u32 - self.prefix as u32;
        let child_count = 1u64 << child_bits;
        if index >= child_count {
            return None;
        }
        let child_size = 1u64 << (32 - child_prefix as u32);
        let base = u64::from(u32::from(self.base)) + index * child_size;
        if base > u64::from(u32::MAX) {
            return None;
        }
        Cidr::new(Ipv4Addr::from(base as u32), child_prefix).ok()
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.prefix)
    }
}

fn prefix_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix as u32)
    }
}

/// A stable, collision-free netavark network identity (64 hex chars).
///
/// Derived from the allocator's collision-free INDEX, never a truncated hash, so
/// two concurrent tenants can never alias onto one netavark network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkId(String);

impl NetworkId {
    /// Derive the id from a collision-free allocation index.
    pub fn from_index(index: u32) -> Self {
        NetworkId(format!("{index:064x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NetworkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A per-tenant network segment: the subnet plus the netavark bridge identity.
///
/// Every field derives from a single collision-free allocation index, so
/// concurrent tenants can never share a subnet, a bridge interface (the
/// IFNAMSIZ-safe `nb-<index>`, never a truncated hash), or a network id. The
/// tenant↔index mapping is owned by the allocator's persisted state, not baked
/// into the identity, so a released index is cleanly reusable after teardown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSegment {
    cidr: Cidr,
    network_name: String,
    network_interface: String,
    network_id: NetworkId,
}

impl NetworkSegment {
    /// Build a segment from a subnet + its collision-free index. `nb-<index>` is
    /// `<= 15` chars (IFNAMSIZ) for every `u32` index, so it never truncates.
    pub fn from_index(subnet: Cidr, index: u32) -> Self {
        NetworkSegment {
            cidr: subnet,
            network_name: format!("nimbus-t-{index}"),
            network_interface: format!("nb-{index}"),
            network_id: NetworkId::from_index(index),
        }
    }

    pub fn cidr(&self) -> Cidr {
        self.cidr
    }

    pub fn network_name(&self) -> &str {
        &self.network_name
    }

    pub fn network_interface(&self) -> &str {
        &self.network_interface
    }

    pub fn network_id(&self) -> &NetworkId {
        &self.network_id
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use super::*;

    #[test]
    fn refuse_non_loopback_bind_allows_ipv4_loopback() {
        let addr = "127.0.0.1:6380".parse().expect("addr parses");
        refuse_non_loopback_bind(addr).expect("loopback should be allowed");
    }

    #[test]
    fn refuse_non_loopback_bind_allows_ipv6_loopback() {
        let addr = "[::1]:6380".parse().expect("addr parses");
        refuse_non_loopback_bind(addr).expect("loopback should be allowed");
    }

    #[test]
    fn refuse_non_loopback_bind_rejects_wildcard() {
        let addr = "0.0.0.0:6380".parse().expect("addr parses");
        let error = refuse_non_loopback_bind(addr).expect_err("wildcard should be rejected");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn is_valid_dns_hostname_accepts_concrete_names() {
        for host in [
            "api.stripe.com",
            "a",
            "a-b.example.com",
            "xn--bcher-kva.example",
            "host123.sub-domain.example-1.com",
        ] {
            assert!(
                is_valid_dns_hostname(host),
                "{host:?} should be a valid DNS hostname"
            );
        }
    }

    #[test]
    fn is_valid_dns_hostname_rejects_malformed_names() {
        // Each case must FAIL the validator; if the corresponding guard inside
        // `is_valid_dns_hostname` is deleted, the matching case starts passing
        // and this test fails, so none of these assertions is vacuous.
        for host in [
            "",                 // empty
            ".example.com",     // leading dot
            "example.com.",     // trailing dot
            "exam ple.com",     // whitespace is not alphanumeric/-
            "bad_host",         // underscore is not allowed
            "-bad.example.com", // label starts with -
            "bad-.example.com", // label ends with -
            "a..b",             // empty interior label
        ] {
            assert!(
                !is_valid_dns_hostname(host),
                "{host:?} must be rejected as a DNS hostname"
            );
        }
        // A 64-char label (one over the per-label limit) must be rejected.
        let over_label = format!("{}.com", "a".repeat(64));
        assert!(
            !is_valid_dns_hostname(&over_label),
            "a 64-char label must be rejected"
        );
        // A 63-char label is exactly at the limit and is accepted.
        let at_label = format!("{}.com", "a".repeat(63));
        assert!(
            is_valid_dns_hostname(&at_label),
            "a 63-char label is within the limit"
        );
        // A name longer than 253 chars must be rejected.
        let over_total = vec!["a"; 200].join(".") + ".example.com";
        assert!(
            over_total.len() > 253,
            "fixture must exceed the 253-char limit"
        );
        assert!(
            !is_valid_dns_hostname(&over_total),
            "a name longer than 253 chars must be rejected"
        );
    }

    #[test]
    fn cidr_parses_valid_blocks_and_rejects_bad_ones() {
        let c = Cidr::parse("10.0.0.0/16").expect("valid /16 should parse");
        assert_eq!(c.base(), "10.0.0.0".parse::<Ipv4Addr>().unwrap());
        assert_eq!(c.prefix(), 16);
        assert_eq!(c.address_count(), 65_536);
        assert_eq!(c.to_string(), "10.0.0.0/16");

        // Host bits set → not the network base.
        assert_eq!(
            Cidr::parse("10.0.0.1/24"),
            Err(CidrError::NotNetworkBase("10.0.0.1/24".to_owned()))
        );
        // Prefix out of range.
        assert!(matches!(
            Cidr::parse("10.0.0.0/33"),
            Err(CidrError::BadPrefix(_))
        ));
        // Missing prefix.
        assert!(matches!(
            Cidr::parse("10.0.0.0"),
            Err(CidrError::Malformed(_))
        ));
    }

    #[test]
    fn cidr_nth_subnet_carves_node_and_tenant_blocks() {
        // Cluster pool /8 → per-node /16: node 0 = 10.0.0.0/16, node 1 = 10.1.0.0/16.
        let pool = Cidr::parse("10.0.0.0/8").unwrap();
        assert_eq!(pool.nth_subnet(16, 0).unwrap().to_string(), "10.0.0.0/16");
        assert_eq!(pool.nth_subnet(16, 1).unwrap().to_string(), "10.1.0.0/16");
        assert_eq!(
            pool.nth_subnet(16, 255).unwrap().to_string(),
            "10.255.0.0/16"
        );

        // Node super-net /16 → per-tenant /24: 256 tenants, then exhaustion.
        let node = Cidr::parse("10.0.0.0/16").unwrap();
        assert_eq!(node.nth_subnet(24, 0).unwrap().to_string(), "10.0.0.0/24");
        assert_eq!(node.nth_subnet(24, 1).unwrap().to_string(), "10.0.1.0/24");
        assert_eq!(
            node.nth_subnet(24, 255).unwrap().to_string(),
            "10.0.255.0/24"
        );
        assert_eq!(
            node.nth_subnet(24, 256),
            None,
            "the 257th /24 must not fit in a /16"
        );

        // A child prefix shorter than the parent is rejected.
        assert_eq!(node.nth_subnet(8, 0), None);
    }

    #[test]
    fn cidr_overlaps_and_contains() {
        let a = Cidr::parse("10.0.0.0/24").unwrap();
        let b = Cidr::parse("10.0.1.0/24").unwrap();
        let super_net = Cidr::parse("10.0.0.0/16").unwrap();
        assert!(a.contains("10.0.0.2".parse().unwrap()));
        assert!(!a.contains("10.0.1.2".parse().unwrap()));
        assert!(!a.overlaps(&b), "adjacent /24s must not overlap");
        assert!(super_net.overlaps(&a), "a /16 overlaps its own /24");
    }

    #[test]
    fn network_segment_from_index_is_collision_free_and_ifnamsiz_safe() {
        let node = Cidr::parse("10.0.0.0/16").unwrap();
        let seg0 = NetworkSegment::from_index(node.nth_subnet(24, 0).unwrap(), 0);
        let seg1 = NetworkSegment::from_index(node.nth_subnet(24, 1).unwrap(), 1);

        // Distinct subnet, interface, name, and id per index — no aliasing.
        assert_eq!(seg0.cidr().to_string(), "10.0.0.0/24");
        assert_eq!(seg1.cidr().to_string(), "10.0.1.0/24");
        assert_ne!(seg0.network_interface(), seg1.network_interface());
        assert_ne!(seg0.network_name(), seg1.network_name());
        assert_ne!(seg0.network_id().as_str(), seg1.network_id().as_str());
        assert_eq!(seg0.network_interface(), "nb-0");
        assert_eq!(
            seg0.network_id().as_str().len(),
            64,
            "network id is 64 hex chars"
        );

        // The interface name stays within IFNAMSIZ (<=15) even for the largest u32
        // index, so it never truncates and never aliases.
        let seg_max = NetworkSegment::from_index(node.nth_subnet(24, 0).unwrap(), u32::MAX);
        assert!(
            seg_max.network_interface().len() <= 15,
            "nb-<index> must fit IFNAMSIZ, got {:?}",
            seg_max.network_interface()
        );
    }
}

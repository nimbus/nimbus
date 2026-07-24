//! Transport-free connectivity-resource control-plane primitives.
//!
//! This crate owns portable network intent, identity, leases, state
//! transitions, and reconciliation contracts. Provider effects such as socket
//! binding, packet forwarding, namespaces, bridges, firewalls, TLS
//! termination, and protocol parsing remain in their effect-owning crates.
//!
//! `nimbus-core` is this crate's only workspace dependency. Upper-layer crates
//! inject provider capabilities without creating reverse dependencies.

#[cfg(test)]
mod tests {
    use nimbus_core::Cidr;

    #[test]
    fn core_network_vocabulary_is_available_at_the_dependency_boundary() {
        let cidr =
            Cidr::new("10.89.0.0".parse().expect("valid IPv4 address"), 24).expect("valid CIDR");

        assert_eq!(cidr.to_string(), "10.89.0.0/24");
    }
}

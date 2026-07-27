//! Per-sandbox egress pin: lock a sandbox's network namespace so the only
//! reachable egress destination is its own policy-enforcing proxy (PEP).
//!
//! The shared bridge places every execute-mode sandbox's PEP on the same
//! gateway address at a distinct port, and the deny-by-default posture is
//! *route-based* (netavark's `no_default_route`): off-link traffic fails with
//! `ENETUNREACH`, but the bridge gateway is on-link and therefore reachable on
//! *any* port. Without this pin a guest could open a connection to a sibling
//! sandbox's PEP (`gateway:other_port`) and egress under that tenant's policy
//! and injected credentials — a cross-tenant credential-theft path (audit H1).
//!
//! This installs an nftables `output`-hook chain *inside* the sandbox's netns
//! with `policy drop`, accepting only loopback, established/related return
//! traffic, and new TCP connections to this sandbox's own PEP. Every other
//! on-link destination (sibling PEPs, peer guests, the gateway on other ports,
//! direct DNS) is dropped. Under libkrun TSI the guest's outbound sockets are
//! issued by the host VMM process *in this netns*, so an `output`-hook chain
//! governs the guest's egress exactly as it governs an ordinary container's.
//!
//! Shared by every OCI-family backend (container + krun microVM) so the
//! isolation is defined once, not forked per backend. Teardown is implicit:
//! destroying the netns (`remove_persistent_network_namespace`) destroys its
//! nftables state, so there is no separate unpin step.

use std::net::IpAddr;

use crate::backends::oci::egress::EgressProxyAssignment;
use crate::error::{Result, SandboxError};

use super::OciNetworkLayout;

/// Pin `layout`'s network namespace so the only permitted egress is to `proxy`
/// (this sandbox's own PEP). Fail-closed: any error (non-IP proxy host, missing
/// `nsenter`/`nft`, a non-zero `nft` exit) returns `Err`, and the caller tears
/// the namespace down so the workload never launches into an unpinned netns.
pub(crate) fn pin_netns_egress_to_own_proxy(
    layout: &OciNetworkLayout,
    proxy: &EgressProxyAssignment,
) -> Result<()> {
    let ruleset = render_pin_ruleset(proxy)?;
    apply_netns_nftables(layout, &ruleset)
}

/// Render the nftables ruleset that drops every egress except this sandbox's
/// own PEP. Split out so the deny/permit shape is unit-testable without a Linux
/// host or `/dev/kvm`. Validates the proxy host as an IP literal (fail-closed:
/// a non-IP host is rejected rather than rendered into a rule that matches
/// nothing and silently drops all traffic, or worse, all traffic to the PEP).
fn render_pin_ruleset(proxy: &EgressProxyAssignment) -> Result<String> {
    let addr = proxy.bind_addr()?;
    let daddr_match = match addr.ip() {
        IpAddr::V4(v4) => format!("ip daddr {v4}"),
        IpAddr::V6(v6) => format!("ip6 daddr {v6}"),
    };
    let port = addr.port();
    Ok(format!(
        "add table inet nimbus_egress_pin\n\
         flush table inet nimbus_egress_pin\n\
         add chain inet nimbus_egress_pin output {{ type filter hook output priority 0; policy drop; }}\n\
         add rule inet nimbus_egress_pin output oifname \"lo\" accept\n\
         add rule inet nimbus_egress_pin output ct state established,related accept\n\
         add rule inet nimbus_egress_pin output {daddr_match} tcp dport {port} accept\n"
    ))
}

#[cfg(target_os = "linux")]
fn apply_netns_nftables(layout: &OciNetworkLayout, ruleset: &str) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let netns_path = &layout.netns_path;
    let mut child = Command::new("nsenter")
        .arg(format!("--net={}", netns_path.display()))
        .arg("--")
        .arg("nft")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to spawn nsenter/nft to pin egress in netns {}: {error}",
                netns_path.display()
            ),
        })?;
    // Take, write, and DROP stdin so `nft` sees EOF before we wait for it.
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "failed to open stdin for the egress-pin nft process".to_owned(),
            })?;
        stdin
            .write_all(ruleset.as_bytes())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to feed the egress-pin nft ruleset: {error}"),
            })?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to await the egress-pin nft process for netns {}: {error}",
                netns_path.display()
            ),
        })?;
    if !output.status.success() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "egress-pin nft rules were rejected for netns {}: {}",
                netns_path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn apply_netns_nftables(_layout: &OciNetworkLayout, _ruleset: &str) -> Result<()> {
    Err(SandboxError::BackendUnavailable {
        message: "per-sandbox egress pin requires Linux network namespaces".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment(host: &str, port: u16) -> EgressProxyAssignment {
        EgressProxyAssignment::for_test(host, port)
    }

    #[test]
    fn ruleset_default_drops_and_allows_only_own_ipv4_proxy() {
        let ruleset = render_pin_ruleset(&assignment("10.89.0.1", 15000)).unwrap();
        // The base chain must default-drop so an unmatched destination is denied.
        assert!(
            ruleset.contains("type filter hook output priority 0; policy drop;"),
            "ruleset must install a default-drop output chain: {ruleset}"
        );
        // Exactly the own PEP (gateway:port) is permitted, by IPv4 daddr + dport.
        assert!(
            ruleset.contains("ip daddr 10.89.0.1 tcp dport 15000 accept"),
            "ruleset must permit the own PEP: {ruleset}"
        );
        // Loopback and established return traffic are permitted.
        assert!(ruleset.contains("oifname \"lo\" accept"));
        assert!(ruleset.contains("ct state established,related accept"));
    }

    #[test]
    fn ruleset_does_not_permit_a_sibling_pep_port() {
        // A sibling PEP on the same gateway at a different port (the H1 reach)
        // must NOT appear as an accept rule.
        let ruleset = render_pin_ruleset(&assignment("10.89.0.1", 15000)).unwrap();
        assert!(
            !ruleset.contains("15001"),
            "no sibling port may be permitted: {ruleset}"
        );
        assert!(
            !ruleset.contains("tcp dport 15001"),
            "no sibling PEP port may be permitted: {ruleset}"
        );
    }

    #[test]
    fn ruleset_uses_ip6_match_for_an_ipv6_proxy() {
        let ruleset = render_pin_ruleset(&assignment("fd00::1", 15000)).unwrap();
        assert!(
            ruleset.contains("ip6 daddr fd00::1 tcp dport 15000 accept"),
            "IPv6 proxy must match via ip6 daddr: {ruleset}"
        );
        assert!(
            !ruleset.contains("ip daddr fd00"),
            "IPv6 proxy must not be rendered as an ip (v4) match: {ruleset}"
        );
    }

    #[test]
    fn render_fails_closed_for_a_non_ip_proxy_host() {
        // A non-IP host cannot be rendered into a sound match; fail closed
        // rather than emit a rule that silently mismatches.
        let error = render_pin_ruleset(&assignment("gateway.local", 15000)).unwrap_err();
        assert!(
            matches!(error, SandboxError::InvalidSpec { .. }),
            "non-IP proxy host must be an InvalidSpec, was: {error:?}"
        );
    }
}

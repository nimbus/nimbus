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

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
#[cfg(any(test, feature = "test-hooks"))]
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use crate::backends::oci::command::run_bounded_command_output;
#[cfg(target_os = "linux")]
use crate::backends::oci::command::run_bounded_command_with_input;
use crate::backends::oci::egress::EgressProxyAssignment;
use crate::error::{Result, SandboxError};

use super::OciNetworkLayout;

const NFT_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

/// Honest observation of the exact deny-by-default namespace pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OciEgressPinObservation {
    Ready,
    NotReady { reason: String },
    Unknown { reason: String },
}

/// Read-only capability consumed by complete attachment readiness.
pub(crate) trait OciEgressPinObserver: Send + Sync {
    fn inspect(
        &self,
        layout: &OciNetworkLayout,
        proxy: &EgressProxyAssignment,
    ) -> OciEgressPinObservation;
}

/// Mutating capability retained by attachment reconciliation.
pub(crate) trait OciEgressPinProvider: OciEgressPinObserver {
    fn apply(&self, layout: &OciNetworkLayout, proxy: &EgressProxyAssignment) -> Result<()>;
}

#[derive(Debug, Default)]
pub(crate) struct RealOciEgressPinProvider;

impl OciEgressPinObserver for RealOciEgressPinProvider {
    fn inspect(
        &self,
        layout: &OciNetworkLayout,
        proxy: &EgressProxyAssignment,
    ) -> OciEgressPinObservation {
        let expected = match proxy.bind_addr() {
            Ok(expected) => expected,
            Err(error) => {
                return OciEgressPinObservation::NotReady {
                    reason: error.to_string(),
                };
            }
        };
        inspect_netns_nftables(&layout.netns_path, expected)
    }
}

impl OciEgressPinProvider for RealOciEgressPinProvider {
    fn apply(&self, layout: &OciNetworkLayout, proxy: &EgressProxyAssignment) -> Result<()> {
        let ruleset = render_pin_ruleset(proxy)?;
        apply_netns_nftables(layout, &ruleset)
    }
}

/// Deterministic substitute for lifecycle and backend contract tests.
///
/// Tests retain the concrete handle while production consumers receive it as
/// `Arc<dyn OciEgressPinProvider>`, proving the real composition seam supports
/// false, unknown, recovery, and call-count assertions without namespace
/// privilege.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug)]
pub(crate) struct FixedOciEgressPinProvider {
    observation: Mutex<OciEgressPinObservation>,
    apply_count: AtomicUsize,
}

#[cfg(any(test, feature = "test-hooks"))]
impl FixedOciEgressPinProvider {
    pub(crate) fn new(observation: OciEgressPinObservation) -> Self {
        Self {
            observation: Mutex::new(observation),
            apply_count: AtomicUsize::new(0),
        }
    }

    pub(crate) fn ready() -> Self {
        Self::new(OciEgressPinObservation::Ready)
    }

    #[cfg(test)]
    pub(crate) fn set_observation(&self, observation: OciEgressPinObservation) {
        *self
            .observation
            .lock()
            .expect("fixed egress-pin observation lock should not be poisoned") = observation;
    }

    #[cfg(test)]
    pub(crate) fn apply_count(&self) -> usize {
        self.apply_count.load(Ordering::SeqCst)
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl OciEgressPinObserver for FixedOciEgressPinProvider {
    fn inspect(
        &self,
        _layout: &OciNetworkLayout,
        _proxy: &EgressProxyAssignment,
    ) -> OciEgressPinObservation {
        self.observation
            .lock()
            .expect("fixed egress-pin observation lock should not be poisoned")
            .clone()
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl OciEgressPinProvider for FixedOciEgressPinProvider {
    fn apply(&self, _layout: &OciNetworkLayout, _proxy: &EgressProxyAssignment) -> Result<()> {
        self.apply_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
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
    use std::process::Command;

    let netns_path = &layout.netns_path;
    let mut command = Command::new("nsenter");
    command
        .arg(format!("--net={}", netns_path.display()))
        .arg("--")
        .arg("nft")
        .arg("-f")
        .arg("-");
    let output =
        run_bounded_command_with_input(&mut command, ruleset.as_bytes(), NFT_COMMAND_TIMEOUT)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to apply the egress pin within its deadline for netns {}: {error}",
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

fn inspect_netns_nftables(
    netns_path: &Path,
    expected_proxy: SocketAddr,
) -> OciEgressPinObservation {
    use std::process::Command;

    match std::fs::symlink_metadata(netns_path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => {
            return OciEgressPinObservation::NotReady {
                reason: format!(
                    "egress-pin namespace {} is not an exact persistent file",
                    netns_path.display()
                ),
            };
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return OciEgressPinObservation::NotReady {
                reason: format!("egress-pin namespace {} is absent", netns_path.display()),
            };
        }
        Err(error) => {
            return OciEgressPinObservation::Unknown {
                reason: format!(
                    "cannot inspect egress-pin namespace {}: {error}",
                    netns_path.display()
                ),
            };
        }
    }

    if !cfg!(target_os = "linux") {
        return OciEgressPinObservation::Unknown {
            reason: format!(
                "egress-pin inspection for {} requires Linux network namespaces",
                netns_path.display()
            ),
        };
    }

    let mut command = Command::new("nsenter");
    command
        .arg(format!("--net={}", netns_path.display()))
        .arg("--")
        .arg("nft")
        .arg("-j")
        .arg("-nn")
        .arg("list")
        .arg("table")
        .arg("inet")
        .arg("nimbus_egress_pin");
    let output = match run_bounded_command_output(&mut command, NFT_COMMAND_TIMEOUT) {
        Ok(output) => output,
        Err(error) => {
            return OciEgressPinObservation::Unknown {
                reason: format!(
                    "nsenter/nft inspection did not complete within its deadline for {}: {error}",
                    netns_path.display()
                ),
            };
        }
    };
    inspect_pin_command_output(
        output.status.success(),
        &output.stdout,
        &output.stderr,
        netns_path,
        expected_proxy,
    )
}

fn inspect_pin_command_output(
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
    netns_path: &Path,
    expected_proxy: SocketAddr,
) -> OciEgressPinObservation {
    if !success {
        let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
        let reason = format!(
            "nft could not inspect the egress-pin table in {}: {stderr}",
            netns_path.display()
        );
        return if stderr.contains("No such file or directory")
            || stderr.contains("No such file")
            || stderr.contains("does not exist")
        {
            OciEgressPinObservation::NotReady { reason }
        } else {
            OciEgressPinObservation::Unknown { reason }
        };
    }
    let document = match serde_json::from_slice::<serde_json::Value>(stdout) {
        Ok(document) => document,
        Err(error) => {
            return OciEgressPinObservation::Unknown {
                reason: format!(
                    "nft returned malformed JSON egress-pin evidence for {}: {error}",
                    netns_path.display()
                ),
            };
        }
    };
    inspect_nft_json_rules(&document, expected_proxy)
}

fn inspect_nft_json_rules(
    document: &serde_json::Value,
    expected_proxy: SocketAddr,
) -> OciEgressPinObservation {
    let Some(entries) = document
        .get("nftables")
        .and_then(serde_json::Value::as_array)
    else {
        return pin_not_ready("nft JSON has no nftables command array");
    };

    let mut exact_table = false;
    let mut exact_chain = false;
    let mut loopback = false;
    let mut established = false;
    let mut own_pep = false;
    for entry in entries {
        let Some(object) = entry.as_object() else {
            return pin_not_ready("nft JSON contains a non-object command");
        };
        if object.contains_key("metainfo") {
            if object.len() != 1 {
                return pin_not_ready("nft metainfo command carries executable siblings");
            }
            continue;
        }
        if let Some(table) = object.get("table") {
            if object.len() != 1 || exact_table || !is_exact_active_pin_table(table) {
                return pin_not_ready(
                    "nft JSON does not contain exactly one active egress-pin table",
                );
            }
            exact_table = true;
            continue;
        }
        if let Some(chain) = object.get("chain") {
            if object.len() != 1 || exact_chain || !is_exact_pin_chain(chain) {
                return pin_not_ready(
                    "nft JSON does not contain exactly one default-drop output chain",
                );
            }
            exact_chain = true;
            continue;
        }
        let Some(rule) = object.get("rule") else {
            return pin_not_ready("nft JSON contains an unrecognized executable command");
        };
        if object.len() != 1 || !is_rule_for_exact_pin_chain(rule) {
            return pin_not_ready("nft JSON contains a rule outside the exact pin chain");
        }
        let Some(expressions) = rule.get("expr").and_then(serde_json::Value::as_array) else {
            return pin_not_ready("nft pin rule has no expression array");
        };
        match classify_exact_pin_rule(expressions, expected_proxy) {
            Some(ExactPinRule::Loopback) if !loopback => loopback = true,
            Some(ExactPinRule::Established) if !established => established = true,
            Some(ExactPinRule::OwnPep) if !own_pep => own_pep = true,
            _ => {
                return pin_not_ready(
                    "nft pin chain contains a duplicate, substituted, or unrecognized rule",
                );
            }
        }
    }
    if exact_table && exact_chain && loopback && established && own_pep {
        OciEgressPinObservation::Ready
    } else {
        pin_not_ready(
            "nft pin table is missing its exact active table, chain, loopback, established, or \
             own-PEP rule",
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExactPinRule {
    Loopback,
    Established,
    OwnPep,
}

fn is_exact_active_pin_table(table: &serde_json::Value) -> bool {
    table.get("family") == Some(&serde_json::json!("inet"))
        && table.get("name") == Some(&serde_json::json!("nimbus_egress_pin"))
        && match table.get("flags") {
            None => true,
            Some(flags) => flags.as_array().is_some_and(std::vec::Vec::is_empty),
        }
}

fn is_exact_pin_chain(chain: &serde_json::Value) -> bool {
    chain.get("family") == Some(&serde_json::json!("inet"))
        && chain.get("table") == Some(&serde_json::json!("nimbus_egress_pin"))
        && chain.get("name") == Some(&serde_json::json!("output"))
        && chain.get("type") == Some(&serde_json::json!("filter"))
        && chain.get("hook") == Some(&serde_json::json!("output"))
        && chain.get("prio") == Some(&serde_json::json!(0))
        && chain.get("policy") == Some(&serde_json::json!("drop"))
}

fn is_rule_for_exact_pin_chain(rule: &serde_json::Value) -> bool {
    rule.get("family") == Some(&serde_json::json!("inet"))
        && rule.get("table") == Some(&serde_json::json!("nimbus_egress_pin"))
        && rule.get("chain") == Some(&serde_json::json!("output"))
}

fn classify_exact_pin_rule(
    expressions: &[serde_json::Value],
    expected_proxy: SocketAddr,
) -> Option<ExactPinRule> {
    let accept = serde_json::json!({"accept": null});
    if expressions
        == [
            serde_json::json!({
                "match": {
                    "op": "==",
                    "left": {"meta": {"key": "oifname"}},
                    "right": "lo"
                }
            }),
            accept.clone(),
        ]
    {
        return Some(ExactPinRule::Loopback);
    }

    let established = serde_json::json!({
        "match": {
            "op": "in",
            "left": {"ct": {"key": "state"}},
            "right": {"set": ["established", "related"]}
        }
    });
    let established_reversed = serde_json::json!({
        "match": {
            "op": "in",
            "left": {"ct": {"key": "state"}},
            "right": {"set": ["related", "established"]}
        }
    });
    if expressions.len() == 2
        && (expressions[0] == established || expressions[0] == established_reversed)
        && expressions[1] == accept
    {
        return Some(ExactPinRule::Established);
    }

    let (address_family, address) = match expected_proxy.ip() {
        IpAddr::V4(address) => ("ip", address.to_string()),
        IpAddr::V6(address) => ("ip6", address.to_string()),
    };
    let destination = serde_json::json!({
        "match": {
            "op": "==",
            "left": {"payload": {"protocol": address_family, "field": "daddr"}},
            "right": address
        }
    });
    let port = serde_json::json!({
        "match": {
            "op": "==",
            "left": {"payload": {"protocol": "tcp", "field": "dport"}},
            "right": expected_proxy.port()
        }
    });
    if expressions == [destination, port, accept] {
        return Some(ExactPinRule::OwnPep);
    }
    None
}

fn pin_not_ready(reason: impl Into<String>) -> OciEgressPinObservation {
    OciEgressPinObservation::NotReady {
        reason: reason.into(),
    }
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

    fn inspected_rules(expected: SocketAddr) -> serde_json::Value {
        let (address_family, address) = match expected.ip() {
            IpAddr::V4(address) => ("ip", address.to_string()),
            IpAddr::V6(address) => ("ip6", address.to_string()),
        };
        serde_json::json!({
            "nftables": [
                {"metainfo": {"json_schema_version": 1}},
                {"chain": {
                    "family": "inet",
                    "table": "nimbus_egress_pin",
                    "name": "output",
                    "handle": 3,
                    "type": "filter",
                    "hook": "output",
                    "prio": 0,
                    "policy": "drop"
                }},
                {"rule": {
                    "family": "inet",
                    "table": "nimbus_egress_pin",
                    "chain": "output",
                    "handle": 4,
                    "expr": [
                        {"match": {
                            "op": "==",
                            "left": {"meta": {"key": "oifname"}},
                            "right": "lo"
                        }},
                        {"accept": null}
                    ]
                }},
                {"rule": {
                    "family": "inet",
                    "table": "nimbus_egress_pin",
                    "chain": "output",
                    "handle": 5,
                    "expr": [
                        {"match": {
                            "op": "in",
                            "left": {"ct": {"key": "state"}},
                            "right": ["established", "related"]
                        }},
                        {"accept": null}
                    ]
                }},
                {"rule": {
                    "family": "inet",
                    "table": "nimbus_egress_pin",
                    "chain": "output",
                    "handle": 6,
                    "expr": [
                        {"match": {
                            "op": "==",
                            "left": {
                                "payload": {"protocol": address_family, "field": "daddr"}
                            },
                            "right": address
                        }},
                        {"match": {
                            "op": "==",
                            "left": {"payload": {"protocol": "tcp", "field": "dport"}},
                            "right": expected.port()
                        }},
                        {"accept": null}
                    ]
                }}
            ]
        })
    }

    fn real_table_inspection(expected: SocketAddr) -> serde_json::Value {
        let mut document = inspected_rules(expected);
        document["nftables"][3]["rule"]["expr"][0]["match"]["right"] =
            serde_json::json!({"set": ["established", "related"]});
        document["nftables"]
            .as_array_mut()
            .expect("nft command array")
            .insert(
                1,
                serde_json::json!({
                    "table": {
                        "family": "inet",
                        "name": "nimbus_egress_pin",
                        "handle": 2
                    }
                }),
            );
        document
    }

    #[test]
    fn real_nft_anonymous_set_and_active_table_shape_is_ready() {
        let expected: SocketAddr = "10.89.0.1:15000".parse().expect("address should parse");
        assert_eq!(
            inspect_nft_json_rules(&real_table_inspection(expected), expected),
            OciEgressPinObservation::Ready,
            "the JSON emitted by list-table inspection must authenticate the installed ruleset"
        );
    }

    #[test]
    fn chain_only_json_cannot_authenticate_table_activation() {
        let expected: SocketAddr = "10.89.0.1:15000".parse().expect("address should parse");
        assert!(
            matches!(
                inspect_nft_json_rules(&inspected_rules(expected), expected),
                OciEgressPinObservation::NotReady { .. }
            ),
            "chain-only JSON cannot prove that the containing table is active"
        );
    }

    #[test]
    fn inspection_requires_exact_default_drop_own_pep_shape() {
        let expected: SocketAddr = "10.89.0.1:15000".parse().expect("address should parse");
        assert_eq!(
            inspect_nft_json_rules(&real_table_inspection(expected), expected),
            OciEgressPinObservation::Ready
        );

        let mut dormant = real_table_inspection(expected);
        dormant["nftables"][1]["table"]["flags"] = serde_json::json!(["dormant"]);

        let mut allow_all = real_table_inspection(expected);
        allow_all["nftables"][2]["chain"]["policy"] = serde_json::json!("accept");
        allow_all["nftables"][2]["chain"]["comment"] = serde_json::json!("policy drop");

        let mut sibling = real_table_inspection(expected);
        sibling["nftables"][5]["rule"]["expr"][1]["match"]["right"] = serde_json::json!(15001);

        let mut extra_tcp = real_table_inspection(expected);
        let mut sibling_rule = extra_tcp["nftables"][5].clone();
        sibling_rule["rule"]["expr"][1]["match"]["right"] = serde_json::json!(15001);
        extra_tcp["nftables"]
            .as_array_mut()
            .expect("nft command array")
            .push(sibling_rule);

        let mut missing_established = real_table_inspection(expected);
        missing_established["nftables"]
            .as_array_mut()
            .expect("nft command array")
            .remove(4);

        let mut jump = real_table_inspection(expected);
        jump["nftables"]
            .as_array_mut()
            .expect("nft command array")
            .push(serde_json::json!({
                "rule": {
                    "family": "inet",
                    "table": "nimbus_egress_pin",
                    "chain": "output",
                    "expr": [{"jump": {"target": "allow_all"}}]
                }
            }));

        for (label, document) in [
            ("dormant table", dormant),
            ("allow-all policy hidden by comment", allow_all),
            ("sibling PEP", sibling),
            ("extra TCP permit", extra_tcp),
            ("missing established return", missing_established),
            ("unrecognized jump verdict", jump),
        ] {
            assert!(
                matches!(
                    inspect_nft_json_rules(&document, expected),
                    OciEgressPinObservation::NotReady { .. }
                ),
                "{label} must fail closed"
            );
        }
    }

    #[test]
    fn command_and_namespace_inspection_classify_absence_malformed_and_unknown_exactly() {
        let expected: SocketAddr = "10.89.0.1:15000".parse().expect("address should parse");
        let temp = tempfile::TempDir::new().expect("temporary namespace root should exist");
        let namespace = temp.path().join("namespace");

        assert!(matches!(
            inspect_netns_nftables(&namespace, expected),
            OciEgressPinObservation::NotReady { .. }
        ));
        std::fs::create_dir(&namespace).expect("directory substitute should create");
        assert!(matches!(
            inspect_netns_nftables(&namespace, expected),
            OciEgressPinObservation::NotReady { .. }
        ));
        std::fs::remove_dir(&namespace).expect("directory substitute should remove");
        std::fs::write(temp.path().join("target"), b"namespace")
            .expect("symlink target should write");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(temp.path().join("target"), &namespace)
                .expect("symlink substitute should create");
            assert!(matches!(
                inspect_netns_nftables(&namespace, expected),
                OciEgressPinObservation::NotReady { .. }
            ));
        }

        assert!(matches!(
            inspect_pin_command_output(
                false,
                b"",
                b"Error: No such file or directory",
                &namespace,
                expected,
            ),
            OciEgressPinObservation::NotReady { .. }
        ));
        assert!(matches!(
            inspect_pin_command_output(false, b"", b"permission denied", &namespace, expected,),
            OciEgressPinObservation::Unknown { .. }
        ));
        assert!(matches!(
            inspect_pin_command_output(true, &[0xff], b"", &namespace, expected),
            OciEgressPinObservation::Unknown { .. }
        ));
        assert!(matches!(
            inspect_pin_command_output(true, br#"{"nftables":[]}"#, b"", &namespace, expected,),
            OciEgressPinObservation::NotReady { .. }
        ));
    }

    #[test]
    fn json_ipv6_inspection_requires_the_exact_own_pep() {
        let expected: SocketAddr = "[fd00::1]:15000".parse().expect("address should parse");
        assert_eq!(
            inspect_nft_json_rules(&real_table_inspection(expected), expected),
            OciEgressPinObservation::Ready
        );
        let mut substituted = real_table_inspection(expected);
        substituted["nftables"][5]["rule"]["expr"][0]["match"]["right"] =
            serde_json::json!("fd00::2");
        assert!(matches!(
            inspect_nft_json_rules(&substituted, expected),
            OciEgressPinObservation::NotReady { .. }
        ));
    }

    #[test]
    fn deterministic_provider_exposes_apply_and_observation_substitution() {
        let provider = FixedOciEgressPinProvider::ready();
        let layout = OciNetworkLayout::under_root(
            "/tmp/nnc53-fixed-pin",
            &nimbus_core::TenantId::new("nnc53-fixed-pin").expect("tenant id"),
            &crate::instance::SandboxId::new("nnc53-fixed-pin"),
        );
        let assignment = assignment("127.0.0.1", 15_000);

        assert_eq!(
            provider.inspect(&layout, &assignment),
            OciEgressPinObservation::Ready
        );
        provider
            .apply(&layout, &assignment)
            .expect("deterministic pin application should succeed");
        assert_eq!(provider.apply_count(), 1);

        provider.set_observation(OciEgressPinObservation::Unknown {
            reason: "injected inspection uncertainty".to_owned(),
        });
        assert!(matches!(
            provider.inspect(&layout, &assignment),
            OciEgressPinObservation::Unknown { .. }
        ));
    }
}

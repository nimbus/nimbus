use super::*;

fn scan_fixture(source: &str) -> ScanOutput {
    scan_fixture_with_exemptions(source, &BTreeSet::new())
}

fn scan_fixture_with_exemptions(source: &str, exempt_paths: &BTreeSet<String>) -> ScanOutput {
    let mut output = ScanOutput::default();
    scan_source(FIXTURE_PATH, source, &mut output, exempt_paths);
    finish_ordinals(&mut output.authorities);
    finish_ordinals(&mut output.risks);
    output
}

#[test]
fn convention_exempts_only_the_shared_test_support_crates() {
    for path in [
        "crates/nimbus-testing",
        "crates/nimbus-testing/src/server_fixture.rs",
        "crates/nimbus-process-harness",
        "crates/nimbus-process-harness/src/ports.rs",
    ] {
        assert!(is_convention_exempt(path), "{path} should be test-support-only");
    }
    assert!(!is_convention_exempt("crates/nimbus-server/src/listener.rs"));
}

fn boundary_kinds(output: &ScanOutput) -> Vec<&str> {
    output
        .boundaries
        .iter()
        .map(|boundary| boundary.kind.as_str())
        .collect()
}

#[test]
fn parsed_compiler_boundaries_cover_multiline_qself_globs_modules_and_macros() {
    let output = scan_fixture(
        r#"
use std::net::{self, *};
#[path = "unix.rs"]
#[cfg_attr(windows, path = "windows.rs")]
mod platform;
#[cfg(windows)]
mod windows_listener;
#[cfg_attr(unix, cfg_attr(windows, path = "nested.rs"))]
mod nested_platform;
fn qself<T: ListenerFactory>() {
    let _ = <T
        as ListenerFactory>::bind("127.0.0.1:0");
}
fn generated() {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
    bind_listener!();
    format!("TcpListener host_port");
}
"#,
    );

    assert_eq!(
        boundary_kinds(&output),
        vec![
            "network-glob-import",
            "module-path",
            "module-path",
            "conditional-module",
            "conditional-module",
            "module-path",
            "conditional-module",
            "qself-bind-adoption",
            "include-expansion",
            "authority-shaped-macro",
            "authority-shaped-macro",
        ]
    );
    assert_eq!(output.boundaries[0].detail, "std::net");
    assert_eq!(output.boundaries[1].detail, "unix.rs");
    assert_eq!(output.boundaries[2].detail, "windows.rs");
    assert!(output.boundaries[3].detail.contains("cfg_attr(windows"));
    assert!(output.boundaries[4].detail.contains("windows_listener.rs"));
    assert_eq!(output.boundaries[5].detail, "nested.rs");
    assert!(output.boundaries[6].detail.contains("cfg_attr(unix"));
    assert_eq!(output.boundaries[7].detail, "bind");
    assert!(output.boundaries[8].detail.contains("OUT_DIR"));
    assert!(output.boundaries[9].detail.starts_with("bind_listener|"));
    assert!(output.boundaries[10].detail.starts_with("format|"));
}

#[test]
fn parsed_generated_authority_covers_raw_and_instance_binds() {
    let output = scan_fixture(
        r#"
fn raw() {
    unsafe { libc::bind(0, std::ptr::null(), 0); }
}
fn instance(socket: socket2::Socket) {
    socket.bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert!(
        output
            .risks
            .iter()
            .any(|risk| risk.kind == "ambiguous-associated-bind")
    );
    assert!(
        output
            .risks
            .iter()
            .any(|risk| risk.kind == "ambiguous-instance-bind")
    );
}

fn authority_kinds(output: &ScanOutput) -> Vec<&str> {
    output
        .authorities
        .iter()
        .map(|occurrence| occurrence.kind.as_str())
        .collect()
}

fn composition_kinds(output: &ScanOutput) -> Vec<&str> {
    output
        .composition
        .iter()
        .map(|occurrence| occurrence.kind.as_str())
        .collect()
}

#[test]
fn machine_forwarder_mutations_are_authority_but_inspection_is_not() {
    let output = scan_fixture(
        r#"
fn effect(config: &Config, request: &[u8], deadline: Deadline) {
    send_machine_forwarder_request(config, "POST", "/expose", request, deadline);
    send_machine_forwarder_request(config, "GET", "/all", &[], deadline);
    send_machine_forwarder_request(config, "POST", "/unexpose", request, deadline);
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(
        authority_kinds(&output),
        vec![
            "machine-forwarder-port-request",
            "machine-forwarder-port-request",
        ]
    );
    assert!(
        output
            .authorities
            .iter()
            .all(|occurrence| occurrence.symbol == "effect")
    );
    assert_eq!(
        output
            .authorities
            .iter()
            .map(|occurrence| occurrence.line)
            .collect::<Vec<_>>(),
        vec![3, 5]
    );
}

#[test]
fn cfg_test_macro_item_does_not_hide_following_production_bind() {
    let output = scan_fixture(
        r#"
#[cfg(test)]
fixture! {
fn hidden() {
    let _ = std::net::TcpListener::bind("127.0.0.1:0");
}
}

fn production() {
let _ = std::net::TcpListener::bind("127.0.0.1:0");
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(authority_kinds(&output), vec!["tcp-bind"]);
    assert_eq!(output.authorities[0].symbol, "production");
}

#[test]
fn unix_datagram_effects_and_owned_tuple_fields_are_structural() {
    let output = scan_fixture(
        r#"
struct Held(pub std::os::unix::net::UnixDatagram);

fn bind() -> std::os::unix::net::UnixDatagram {
std::os::unix::net::UnixDatagram::bind("/tmp/nimbus.sock").unwrap()
}

unsafe fn adopt(fd: std::os::fd::RawFd) -> std::os::unix::net::UnixDatagram {
std::os::unix::net::UnixDatagram::from_raw_fd(fd)
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(
        authority_kinds(&output),
        vec![
            "listener-ownership-slot",
            "listener-return-handoff",
            "unix-datagram-bind",
            "listener-return-handoff",
            "unix-datagram-from-raw-fd",
        ]
    );
}

#[test]
fn shorthand_provider_request_and_multiline_return_are_structural() {
    let output = scan_fixture(
        r#"
struct GenericRequest { host_port: u16 }

fn request(host_port: u16) {
let _ = GenericRequest { host_port };
}

fn handoff()
-> Result<
    std::net::TcpListener,
    std::io::Error,
>
{
std::net::TcpListener::bind("127.0.0.1:0")
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(
        authority_kinds(&output),
        vec![
            "provider-port-request",
            "listener-return-handoff",
            "tcp-bind",
        ]
    );
    assert!(
        output
            .declarations
            .iter()
            .any(|declaration| { declaration.name == "handoff" && declaration.line == 8 })
    );
}

#[test]
fn referenced_socket_does_not_hide_owned_socket_in_same_type() {
    let output = scan_fixture(
        r#"
struct Mixed<'a>(&'a std::net::TcpListener, Option<std::net::TcpListener>);
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(authority_kinds(&output), vec!["listener-ownership-slot"]);
}

#[test]
fn production_reference_to_test_exempt_path_fails_closed() {
    let output = scan_fixture(
        r#"
#[path = "tests/fixture.rs"]
mod fixture;
"#,
    );
    assert!(
        output
            .errors
            .iter()
            .any(|error| error.contains("production module/include references test-exempt source")),
        "{:?}",
        output.errors
    );

    let cfg_only = scan_fixture(
        r#"
#[cfg(test)]
#[path = "tests/fixture.rs"]
mod fixture;
"#,
    );
    assert!(cfg_only.errors.is_empty(), "{:?}", cfg_only.errors);

    let exempt_paths = BTreeSet::from([format!(
        "{}/excluded.rs",
        Path::new(FIXTURE_PATH)
            .parent()
            .unwrap_or(Path::new(""))
            .display()
    )]);
    let second_inclusion = scan_fixture_with_exemptions(
        r#"
#[path = "./excluded.rs"]
mod excluded;
"#,
        &exempt_paths,
    );
    assert!(
        second_inclusion
            .errors
            .iter()
            .any(|error| error.contains("production module/include references test-exempt source")),
        "{:?}",
        second_inclusion.errors
    );
}

#[test]
fn bind_function_values_imports_and_bare_calls_fail_closed() {
    let output = scan_fixture(
        r#"
use libc::bind;

fn indirect() {
let socket_bind = std::net::TcpListener::bind;
let _ = socket_bind("127.0.0.1:0");
}

fn bare() {
unsafe { bind(0, std::ptr::null(), 0); }
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(authority_kinds(&output), vec!["tcp-bind"]);
    assert!(
        output
            .risks
            .iter()
            .any(|risk| risk.kind == "ambiguous-bind-function-import")
    );
    assert!(
        output
            .risks
            .iter()
            .any(|risk| risk.kind == "ambiguous-bare-bind-call")
    );
}

#[test]
fn associated_socket_aliases_fail_closed() {
    let output = scan_fixture(
        r#"
trait ListenerKind {
type Listener = std::net::TcpListener;
}

struct Host;
impl ListenerKind for Host {
type Listener = std::net::TcpListener;
}
"#,
    );

    assert_eq!(
        output
            .errors
            .iter()
            .filter(|error| {
                error.contains("associated socket authority type alias is forbidden")
            })
            .count(),
        2,
        "{:?}",
        output.errors
    );
}

#[test]
fn cfg_test_associated_and_statement_nodes_are_skipped_exactly() {
    let output = scan_fixture(
        r#"
struct Fixture;

impl Fixture {
#[cfg(test)]
fixture! { TcpListener host_port }
}

fn production() {
#[cfg(test)]
fixture! { TcpListener host_port }
let _ = match 1 {
    #[cfg(test)]
    0 => TcpListener::bind("127.0.0.1:0"),
    _ => TcpListener::bind("127.0.0.1:0"),
};
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(authority_kinds(&output), vec!["tcp-bind"]);
    assert!(output.risks.is_empty(), "{:?}", output.risks);
}

#[test]
fn composition_constructors_roots_and_reconstructions_are_structural() {
    let output = scan_fixture(
        r#"
impl LocalNetworkManager {
fn bootstrap() {}
}

fn compose() {
let _ = LocalNodeNetworkRoot::resolve_for_current_platform(None);
let _ = LocalNetworkManager::open("root", registry());
let _ = LocalNetworkStateStore::open("root");
let _ = LocalPortLeaseAuthority::open("root");
let _ = ConfiguredSegmentAllocator::reconstruct_for_runner("root", "cidr", 24);
let _ = HostMachineNetworkAuthority::injected(authority());
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(
        composition_kinds(&output),
        vec![
            "manager-bootstrap-declaration",
            "local-node-root-resolver",
            "manager-direct-open",
            "primitive-state-store-open",
            "primitive-port-authority-open",
            "segment-runner-reconstruction",
            "manager-derived-parent-machine-authority",
        ]
    );
}

#[test]
fn guest_identity_mint_is_visible_but_cfg_test_mint_is_not() {
    let output = scan_fixture(
        r#"
#[cfg(test)]
fn fixture_mint() {
let _ = MachineForwarderAuthority::new(provider(), generation());
}

fn guest_mint_parent() {
let _ = MachineForwarderAuthority::new(provider(), generation());
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(
        composition_kinds(&output),
        vec!["machine-forwarder-authority-mint"]
    );
    assert_eq!(output.composition[0].symbol, "guest_mint_parent");
}

#[test]
fn compound_cfg_that_requires_test_is_skipped_without_hiding_production() {
    let output = scan_fixture(
        r#"
#[cfg(all(test, unix))]
mod test_only {
fn hidden() {
    let _ = LocalNetworkStateStore::open("fixture");
}
}

#[cfg(not(test))]
fn production() {
let _ = LocalNetworkStateStore::open("production");
}
"#,
    );

    assert!(output.errors.is_empty(), "{:?}", output.errors);
    assert_eq!(
        composition_kinds(&output),
        vec!["primitive-state-store-open"]
    );
    assert_eq!(output.composition[0].symbol, "production");
}

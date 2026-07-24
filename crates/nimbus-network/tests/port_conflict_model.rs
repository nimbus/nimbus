use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::num::NonZeroU16;

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, PortBindRealm, PortBindTarget, PortBindingSpec,
    PortExposure, PortIpv6Overlap, PortLeaseBinding, PortLeaseError, PortLeaseId, PortLeasePhase,
    PortLeaseRequest, PortProtocol, PortRequestMode,
};

const PORT: u16 = 41_473;

#[test]
fn positive_and_negative_conflict_matrix_is_explicit() {
    let cases = vec![
        case(
            "same TCP host wildcard",
            tcp(host(), v4_wildcard(), exact(PORT)),
            tcp(host(), v4_wildcard(), exact(PORT)),
            true,
        ),
        case(
            "TCP and UDP are separate",
            tcp(host(), v4_wildcard(), exact(PORT)),
            udp(host(), v4_wildcard(), exact(PORT)),
            false,
        ),
        case(
            "same isolated realm overlaps",
            tcp(isolated("realm-a"), v4_wildcard(), exact(PORT)),
            tcp(isolated("realm-a"), v4_wildcard(), exact(PORT)),
            true,
        ),
        case(
            "proven isolated realms are separate",
            tcp(isolated("realm-a"), v4_wildcard(), exact(PORT)),
            tcp(isolated("realm-b"), v4_wildcard(), exact(PORT)),
            false,
        ),
        case(
            "unknown realm overlaps host",
            tcp(PortBindRealm::Unknown, v4_wildcard(), exact(PORT)),
            tcp(host(), v4_wildcard(), exact(PORT)),
            true,
        ),
        case(
            "unknown realm overlaps isolated",
            tcp(PortBindRealm::Unknown, v4_wildcard(), exact(PORT)),
            tcp(isolated("realm-a"), v4_wildcard(), exact(PORT)),
            true,
        ),
        case(
            "IPv4 wildcard overlaps specific",
            tcp(host(), v4_wildcard(), exact(PORT)),
            tcp(host(), v4_specific(Ipv4Addr::LOCALHOST), exact(PORT)),
            true,
        ),
        case(
            "same IPv4 specific overlaps",
            tcp(host(), v4_specific(Ipv4Addr::LOCALHOST), exact(PORT)),
            tcp(host(), v4_specific(Ipv4Addr::LOCALHOST), exact(PORT)),
            true,
        ),
        case(
            "different IPv4 specifics are separate",
            tcp(
                host(),
                v4_specific(Ipv4Addr::new(127, 0, 0, 1)),
                exact(PORT),
            ),
            tcp(
                host(),
                v4_specific(Ipv4Addr::new(127, 0, 0, 2)),
                exact(PORT),
            ),
            false,
        ),
        case(
            "IPv6 wildcard overlaps specific",
            tcp(host(), v6_wildcard(PortIpv6Overlap::Unknown), exact(PORT)),
            tcp(
                host(),
                v6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::Unknown),
                exact(PORT),
            ),
            true,
        ),
        case(
            "different IPv6 specifics are separate",
            tcp(
                host(),
                v6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::Unknown),
                exact(PORT),
            ),
            tcp(
                host(),
                v6_specific(
                    "2001:db8::1".parse().expect("fixture IPv6 should parse"),
                    PortIpv6Overlap::Unknown,
                ),
                exact(PORT),
            ),
            false,
        ),
        case(
            "unknown IPv6 host semantics overlap IPv4",
            tcp(host(), v4_specific(Ipv4Addr::LOCALHOST), exact(PORT)),
            tcp(
                host(),
                v6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::Unknown),
                exact(PORT),
            ),
            true,
        ),
        case(
            "known dual-stack IPv6 overlaps IPv4",
            tcp(host(), v4_wildcard(), exact(PORT)),
            tcp(
                host(),
                v6_wildcard(PortIpv6Overlap::OverlapsIpv4),
                exact(PORT),
            ),
            true,
        ),
        case(
            "proven V6-only binding is separate from IPv4",
            tcp(host(), v4_wildcard(), exact(PORT)),
            tcp(
                host(),
                v6_wildcard(PortIpv6Overlap::ProvenDisjoint),
                exact(PORT),
            ),
            false,
        ),
        case(
            "unknown bind target overlaps a specific target",
            tcp(host(), PortBindTarget::unknown(), exact(PORT)),
            tcp(host(), v4_specific(Ipv4Addr::LOCALHOST), exact(PORT)),
            true,
        ),
        case(
            "different ports are separate",
            tcp(host(), v4_wildcard(), exact(PORT)),
            tcp(host(), v4_wildcard(), exact(PORT + 1)),
            false,
        ),
        case(
            "exposure metadata cannot weaken kernel overlap",
            PortBindingSpec::new(
                PortProtocol::Tcp,
                host(),
                v4_wildcard(),
                PortExposure::Loopback,
                exact(PORT),
            ),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                host(),
                v4_wildcard(),
                PortExposure::Public,
                exact(PORT),
            ),
            true,
        ),
        case(
            "proven realm separation survives unknown address",
            tcp(isolated("realm-a"), PortBindTarget::unknown(), exact(PORT)),
            tcp(isolated("realm-b"), PortBindTarget::unknown(), exact(PORT)),
            false,
        ),
    ];

    for (index, case) in cases.into_iter().enumerate() {
        let root = tempfile::tempdir().expect("state root should exist");
        let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
        let first = request(index * 2, case.first);
        let second = request(index * 2 + 1, case.second);
        let first_record = authority
            .reserve(first)
            .unwrap_or_else(|error| panic!("{} first reserve failed: {error}", case.name));
        assert_eq!(first_record.phase(), PortLeasePhase::Reserved);

        let second_result = authority.reserve(second);
        if case.conflicts {
            assert!(
                matches!(
                    second_result,
                    Err(PortLeaseError::PortConflict {
                        conflicting_port,
                        ..
                    }) if conflicting_port.get() == PORT
                ),
                "{} should conflict, got {second_result:?}",
                case.name
            );
            assert_eq!(
                authority.list().expect("authority should list").len(),
                1,
                "{} conflict must not mutate authority",
                case.name
            );
        } else {
            let second_record = second_result
                .unwrap_or_else(|error| panic!("{} should not conflict: {error}", case.name));
            assert_eq!(second_record.phase(), PortLeasePhase::Reserved);
            assert_eq!(
                authority.list().expect("authority should list").len(),
                2,
                "{} independent domains may coexist",
                case.name
            );
        }
    }
}

#[test]
fn overlapping_ranges_choose_lowest_free_slot_and_exhaust_atomically() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let range = PortRequestMode::range(port(PORT), port(PORT + 1))
        .expect("ordered fixture range should validate");
    let spec = tcp(host(), v4_wildcard(), range.clone());

    let first = authority
        .reserve(request(0, spec.clone()))
        .expect("first range should reserve");
    let second = authority
        .reserve(request(1, spec.clone()))
        .expect("second range should reserve remaining slot");
    assert_eq!(first.reserved_port().map(NonZeroU16::get), Some(PORT));
    assert_eq!(second.reserved_port().map(NonZeroU16::get), Some(PORT + 1));

    let exhausted = authority
        .reserve(request(2, spec))
        .expect_err("third overlapping range must be exhausted");
    assert!(matches!(
        exhausted,
        PortLeaseError::PortRangeExhausted {
            requested_range,
            ..
        } if requested_range.start().get() == PORT
            && requested_range.end().get() == PORT + 1
    ));
    assert_eq!(
        authority.list().expect("authority should list").len(),
        2,
        "exhaustion must publish no partial lease"
    );

    drop(authority);
    let restarted = LocalPortLeaseAuthority::open(root.path()).expect("authority should restart");
    let reserved = restarted
        .list()
        .expect("authority should list")
        .into_iter()
        .map(|record| {
            record
                .reserved_port()
                .expect("range record should retain selected slot")
                .get()
        })
        .collect::<Vec<_>>();
    assert_eq!(reserved, vec![PORT, PORT + 1]);
}

#[test]
fn exact_range_protocol_and_realm_allocation_compose() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");

    authority
        .reserve(request(0, tcp(host(), v4_wildcard(), exact(PORT))))
        .expect("exact TCP claim should reserve");
    let range = PortRequestMode::range(port(PORT), port(PORT + 1))
        .expect("ordered fixture range should validate");
    let same_domain = authority
        .reserve(request(1, tcp(host(), v4_wildcard(), range.clone())))
        .expect("range should skip exact claim");
    let udp_domain = authority
        .reserve(request(2, udp(host(), v4_wildcard(), range.clone())))
        .expect("UDP may reuse TCP number");
    let isolated_domain = authority
        .reserve(request(3, tcp(isolated("realm-a"), v4_wildcard(), range)))
        .expect("proven isolated realm may reuse host number");

    assert_eq!(
        same_domain.reserved_port().map(NonZeroU16::get),
        Some(PORT + 1)
    );
    assert_eq!(udp_domain.reserved_port().map(NonZeroU16::get), Some(PORT));
    assert_eq!(
        isolated_domain.reserved_port().map(NonZeroU16::get),
        Some(PORT)
    );
}

#[test]
fn provider_assigned_port_is_fenced_atomically_at_adoption() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    authority
        .reserve(request(0, tcp(host(), v4_wildcard(), exact(PORT))))
        .expect("exact claim should reserve");

    let provider_spec = tcp(host(), v4_wildcard(), PortRequestMode::ProviderAssigned);
    let first_request = request(1, provider_spec.clone());
    let second_request = request(2, provider_spec);
    let first_pending = authority
        .reserve(first_request.clone())
        .expect("first provider request should reserve identity");
    let second_pending = authority
        .reserve(second_request.clone())
        .expect("second provider request should reserve identity");
    assert_eq!(first_pending.reserved_port(), None);
    assert_eq!(second_pending.reserved_port(), None);

    let conflict = authority
        .adopt(&first_request, binding(PORT, "provider-conflict"))
        .expect_err("provider result must conflict with exact claim");
    assert!(matches!(
        conflict,
        PortLeaseError::PortConflict {
            conflicting_port,
            ..
        } if conflicting_port.get() == PORT
    ));
    assert_eq!(
        authority
            .inspect(first_request.lease_id())
            .expect("pending request should inspect")
            .expect("pending request should exist")
            .reserved_port(),
        None,
        "failed adoption must not claim the provider port"
    );

    let first_binding = authority
        .adopt(&first_request, binding(PORT + 1, "provider-a"))
        .expect("unused provider result should adopt");
    assert_eq!(first_binding.phase(), PortLeasePhase::Binding);
    assert_eq!(
        first_binding.reserved_port().map(NonZeroU16::get),
        Some(PORT + 1)
    );

    assert!(matches!(
        authority.adopt(&second_request, binding(PORT + 1, "provider-b")),
        Err(PortLeaseError::PortConflict {
            conflicting_port,
            ..
        }) if conflicting_port.get() == PORT + 1
    ));
    let second_binding = authority
        .adopt(&second_request, binding(PORT + 2, "provider-b"))
        .expect("a distinct provider result should adopt");
    assert_eq!(
        second_binding.reserved_port().map(NonZeroU16::get),
        Some(PORT + 2)
    );
}

#[test]
fn validated_wire_types_fail_closed_and_round_trip_canonically() {
    assert!(PortBindRealm::proven_isolated("").is_err());
    assert!(PortBindRealm::proven_isolated("contains whitespace").is_err());
    assert!(PortBindRealm::proven_isolated("contains/slash").is_err());
    assert!(PortBindRealm::proven_isolated("a".repeat(129)).is_err());
    assert!(PortRequestMode::range(port(PORT + 1), port(PORT)).is_err());

    let mapped = Ipv4Addr::LOCALHOST.to_ipv6_mapped();
    assert!(
        PortBindTarget::ipv6_specific(mapped, PortIpv6Overlap::ProvenDisjoint).is_err(),
        "IPv4-mapped IPv6 must not claim proven cross-family disjointness"
    );

    let spec = PortBindingSpec::new(
        PortProtocol::Udp,
        isolated("realm-a"),
        v6_specific(Ipv6Addr::LOCALHOST, PortIpv6Overlap::ProvenDisjoint),
        PortExposure::Private,
        PortRequestMode::range(port(PORT), port(PORT + 1)).expect("fixture range should validate"),
    );
    let json = serde_json::to_string(&spec).expect("spec should serialize");
    assert_eq!(
        json,
        concat!(
            r#"{"protocol":"udp","realm":{"proven_isolated":"realm-a"},"target":{"kind":"ipv6_specific","#,
            r#""address":"::1","ipv4_overlap":"proven_disjoint"},"exposure":"private","#,
            r#""port":{"range":{"start":41473,"end":41474}}}"#
        )
    );
    assert_eq!(
        serde_json::from_str::<PortBindingSpec>(&json).expect("spec should deserialize"),
        spec
    );

    let invalid_range = json.replace(
        r#""start":41473,"end":41474"#,
        r#""start":41474,"end":41473"#,
    );
    assert!(
        serde_json::from_str::<PortBindingSpec>(&invalid_range).is_err(),
        "wire data cannot bypass ordered range validation"
    );

    let mapped_wire = json.replace(r#""address":"::1""#, r#""address":"::ffff:127.0.0.1""#);
    assert!(
        serde_json::from_str::<PortBindingSpec>(&mapped_wire).is_err(),
        "wire data cannot bypass mapped-address validation"
    );

    let unknown_field = json.replacen('{', r#"{"unexpected":true,"#, 1);
    assert!(
        serde_json::from_str::<PortBindingSpec>(&unknown_field).is_err(),
        "unknown portable request fields must fail closed"
    );
}

#[derive(Debug)]
struct ConflictCase {
    name: &'static str,
    first: PortBindingSpec,
    second: PortBindingSpec,
    conflicts: bool,
}

fn case(
    name: &'static str,
    first: PortBindingSpec,
    second: PortBindingSpec,
    conflicts: bool,
) -> ConflictCase {
    ConflictCase {
        name,
        first,
        second,
        conflicts,
    }
}

fn request(index: usize, binding: PortBindingSpec) -> PortLeaseRequest {
    const PAYLOADS: [&str; 40] = [
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01ARZ3NDEKTSV4RRFFQ69G5FAW",
        "01ARZ3NDEKTSV4RRFFQ69G5FAX",
        "01ARZ3NDEKTSV4RRFFQ69G5FAY",
        "01ARZ3NDEKTSV4RRFFQ69G5FAZ",
        "01ARZ3NDEKTSV4RRFFQ69G5FB0",
        "01ARZ3NDEKTSV4RRFFQ69G5FB1",
        "01ARZ3NDEKTSV4RRFFQ69G5FB2",
        "01ARZ3NDEKTSV4RRFFQ69G5FB3",
        "01ARZ3NDEKTSV4RRFFQ69G5FB4",
        "01ARZ3NDEKTSV4RRFFQ69G5FB5",
        "01ARZ3NDEKTSV4RRFFQ69G5FB6",
        "01ARZ3NDEKTSV4RRFFQ69G5FB7",
        "01ARZ3NDEKTSV4RRFFQ69G5FB8",
        "01ARZ3NDEKTSV4RRFFQ69G5FB9",
        "01ARZ3NDEKTSV4RRFFQ69G5FBA",
        "01ARZ3NDEKTSV4RRFFQ69G5FBB",
        "01ARZ3NDEKTSV4RRFFQ69G5FBC",
        "01ARZ3NDEKTSV4RRFFQ69G5FBD",
        "01ARZ3NDEKTSV4RRFFQ69G5FBE",
        "01ARZ3NDEKTSV4RRFFQ69G5FBF",
        "01ARZ3NDEKTSV4RRFFQ69G5FBG",
        "01ARZ3NDEKTSV4RRFFQ69G5FBH",
        "01ARZ3NDEKTSV4RRFFQ69G5FBJ",
        "01ARZ3NDEKTSV4RRFFQ69G5FBK",
        "01ARZ3NDEKTSV4RRFFQ69G5FBM",
        "01ARZ3NDEKTSV4RRFFQ69G5FBN",
        "01ARZ3NDEKTSV4RRFFQ69G5FBP",
        "01ARZ3NDEKTSV4RRFFQ69G5FBQ",
        "01ARZ3NDEKTSV4RRFFQ69G5FBR",
        "01ARZ3NDEKTSV4RRFFQ69G5FBS",
        "01ARZ3NDEKTSV4RRFFQ69G5FBT",
        "01ARZ3NDEKTSV4RRFFQ69G5FBV",
        "01ARZ3NDEKTSV4RRFFQ69G5FBW",
        "01ARZ3NDEKTSV4RRFFQ69G5FBX",
        "01ARZ3NDEKTSV4RRFFQ69G5FBY",
        "01ARZ3NDEKTSV4RRFFQ69G5FBZ",
        "01ARZ3NDEKTSV4RRFFQ69G5FC0",
        "01ARZ3NDEKTSV4RRFFQ69G5FC1",
        "01ARZ3NDEKTSV4RRFFQ69G5FC2",
    ];
    let payload = PAYLOADS[index];
    let lease_id: PortLeaseId = format!("netportlease_{payload}")
        .parse()
        .expect("fixture lease ID should parse");
    let owner_id: ListenerId = format!("netlistener_{payload}")
        .parse()
        .expect("fixture owner ID should parse");
    PortLeaseRequest::new(
        lease_id,
        owner_id.into(),
        None,
        NetworkResourceGeneration::new(7),
        NetworkLeaseEpoch::new(11),
        binding,
    )
}

fn tcp(realm: PortBindRealm, target: PortBindTarget, port: PortRequestMode) -> PortBindingSpec {
    PortBindingSpec::new(
        PortProtocol::Tcp,
        realm,
        target,
        PortExposure::Unknown,
        port,
    )
}

fn udp(realm: PortBindRealm, target: PortBindTarget, port: PortRequestMode) -> PortBindingSpec {
    PortBindingSpec::new(
        PortProtocol::Udp,
        realm,
        target,
        PortExposure::Unknown,
        port,
    )
}

fn host() -> PortBindRealm {
    PortBindRealm::Host
}

fn isolated(value: impl Into<String>) -> PortBindRealm {
    PortBindRealm::proven_isolated(value).expect("fixture realm should validate")
}

fn v4_wildcard() -> PortBindTarget {
    PortBindTarget::ipv4_wildcard()
}

fn v4_specific(address: Ipv4Addr) -> PortBindTarget {
    PortBindTarget::ipv4_specific(address)
}

fn v6_wildcard(overlap: PortIpv6Overlap) -> PortBindTarget {
    PortBindTarget::ipv6_wildcard(overlap)
}

fn v6_specific(address: Ipv6Addr, overlap: PortIpv6Overlap) -> PortBindTarget {
    PortBindTarget::ipv6_specific(address, overlap).expect("fixture IPv6 address should validate")
}

fn exact(value: u16) -> PortRequestMode {
    PortRequestMode::Exact(port(value))
}

fn port(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).expect("fixture port should be non-zero")
}

fn binding(value: u16, opaque: &str) -> PortLeaseBinding {
    let provider_id: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider ID should parse");
    PortLeaseBinding::new(
        port(value),
        NetworkProviderHandle::new(provider_id, opaque)
            .expect("fixture provider handle should validate"),
    )
}

#[test]
fn address_accessors_preserve_location_without_promoting_it_to_identity() {
    let target = v4_specific(Ipv4Addr::LOCALHOST);
    assert_eq!(
        target.specific_address(),
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    );
    assert!(!target.is_wildcard());
    assert!(!target.is_unknown());
}

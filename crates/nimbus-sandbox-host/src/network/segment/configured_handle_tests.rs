use tempfile::tempdir;

use super::*;

#[test]
fn configured_allocator_retains_the_injected_store_handle() {
    let dir = tempdir().expect("temporary state root");
    let store = LocalNetworkStateStore::open(dir.path()).expect("state store should open");
    let authority_path = store.authority_path().to_path_buf();
    let allocator = ConfiguredSegmentAllocator::from_store(
        store,
        Cidr::parse(DEFAULT_NODE_SUPERNET).expect("fixture CIDR"),
        DEFAULT_TENANT_PREFIX,
    )
    .expect("typed topology should compose");

    assert_eq!(
        allocator
            .allocator()
            .expect("configured allocator should retain its store")
            .store
            .authority_path(),
        authority_path,
        "the configured adapter must retain the injected store"
    );
    let allocated = allocator
        .acquire(
            &TenantId::new("tenant-handle").expect("tenant id"),
            &NetworkAttachmentId::for_workload_attachment(
                "sandbox-handle",
                super::super::DEFAULT_ATTACHMENT_NAME,
            ),
        )
        .expect("retained allocator should allocate");
    assert_eq!(allocated.cidr().to_string(), "10.0.0.0/24");
}

#[test]
fn configured_allocator_rejects_invalid_typed_topology_without_mutating_state() {
    let dir = tempdir().expect("temporary state root");
    let store = LocalNetworkStateStore::open(dir.path()).expect("state store should open");
    let error = match ConfiguredSegmentAllocator::from_store(
        store.clone(),
        Cidr::parse("10.9.0.0/24").expect("fixture CIDR"),
        23,
    ) {
        Ok(_) => panic!("a tenant prefix shorter than its super-net must fail"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("/23") && error.to_string().contains("10.9.0.0/24"),
        "typed failure should preserve attempted and parent topology: {error}"
    );
    assert!(
        store
            .read::<SegmentState>(&NetworkStatePartition::SegmentAllocations)
            .expect("state should remain readable")
            .is_none(),
        "rejected topology must not write segment state"
    );
}

#[test]
fn direct_reconstruction_is_explicit_and_parses_before_opening_state() {
    let dir = tempdir().expect("temporary state root");
    let missing_root = dir.path().join("must-remain-missing");
    let error = match ConfiguredSegmentAllocator::reconstruct_from_state_root(
        &missing_root,
        "not-a-cidr",
        DEFAULT_TENANT_PREFIX,
    ) {
        Ok(_) => panic!("malformed direct topology must fail"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("invalid node network super-net"),
        "direct reconstruction should retain the parse diagnostic: {error}"
    );
    assert!(
        !missing_root.exists(),
        "malformed topology must fail before direct reconstruction opens a store"
    );
}

//! Atomic tenant-publication quota behavior for host-global port authority.

use super::*;

#[test]
fn tenant_limit_counts_every_live_phase_and_ignores_terminal_phases() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let tenant = TenantId::new("tenant-a").expect("tenant should parse");
    let limit = || TenantPublishedPortLimit::new(tenant.clone(), 1);
    let first = published_request("01ARZ3NDEKTSV4RRFFQ69G5FBA", PORT);
    let second = published_request("01ARZ3NDEKTSV4RRFFQ69G5FBB", PORT + 1);
    let third = published_request("01ARZ3NDEKTSV4RRFFQ69G5FBC", PORT + 2);

    authority
        .reserve_batch_with_tenant_limit(vec![first.clone()], limit())
        .expect("first published reservation should fit");
    assert_quota_rejects(&authority, &second, limit(), 1, 1);

    let first_binding = binding(PORT, "quota-first");
    let first_claim = PortBindClaim::new(first_binding.provider_handle().clone());
    claim_and_adopt(&authority, &first, None, first_binding)
        .expect("first reservation should enter Binding");
    assert_quota_rejects(&authority, &second, limit(), 1, 1);

    authority
        .activate_claimed(&first, &first_claim)
        .expect("first reservation should enter Active");
    assert_quota_rejects(&authority, &second, limit(), 1, 1);

    authority
        .withdraw(&first)
        .expect("first reservation should enter Withdrawing");
    assert_quota_rejects(&authority, &second, limit(), 1, 1);

    authority
        .release(&first)
        .expect("first reservation should become terminal");
    authority
        .reserve_batch_with_tenant_limit(vec![second.clone()], limit())
        .expect("Released usage must not consume tenant quota");

    let failed_claim = bind_claim("quota-failed");
    authority
        .claim_bind(&second, None, failed_claim.clone())
        .expect("second reservation should claim its bind attempt");
    authority
        .record_claimed_bind_failure_without_effect(
            &second,
            None,
            &failed_claim,
            bind_failure(PORT + 1, "quota-failed"),
        )
        .expect("second reservation should become terminal Failed");
    authority
        .reserve_batch_with_tenant_limit(vec![third], limit())
        .expect("Failed usage must not consume tenant quota");
}

#[test]
fn exact_tenant_limit_replay_adds_zero_usage_after_limit_reduction() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let tenant = TenantId::new("tenant-a").expect("tenant should parse");
    let request = published_request("01ARZ3NDEKTSV4RRFFQ69G5FBD", PORT);
    authority
        .reserve_batch_with_tenant_limit(
            vec![request.clone()],
            TenantPublishedPortLimit::new(tenant.clone(), 1),
        )
        .expect("initial reservation should fit");

    let replay = authority
        .reserve_batch_with_tenant_limit(
            vec![request.clone(), request.clone()],
            TenantPublishedPortLimit::new(tenant, 0),
        )
        .expect("exact duplicate replay adds no usage even below current live count");
    assert_eq!(replay.len(), 2);
    assert_eq!(replay[0], replay[1]);
}

#[test]
fn tenant_limit_rejects_missing_or_cross_tenant_attribution_without_mutation() {
    let root = tempfile::tempdir().expect("state root should exist");
    let authority = LocalPortLeaseAuthority::open(root.path()).expect("authority should open");
    let tenant_a = TenantId::new("tenant-a").expect("tenant should parse");
    let tenant_b = TenantId::new("tenant-b").expect("tenant should parse");
    let missing = request_with_accounting(
        "01ARZ3NDEKTSV4RRFFQ69G5FBE",
        "01ARZ3NDEKTSV4RRFFQ69G5FBE",
        None,
        7,
        11,
        PORT,
        PortLeaseAccounting::TenantPublished,
    );
    assert!(matches!(
        authority.reserve(missing),
        Err(PortLeaseError::TenantAttributionRequired { .. })
    ));

    let cross_tenant = request_with_accounting(
        "01ARZ3NDEKTSV4RRFFQ69G5FBF",
        "01ARZ3NDEKTSV4RRFFQ69G5FBF",
        Some(tenant_b.clone()),
        7,
        11,
        PORT + 1,
        PortLeaseAccounting::TenantPublished,
    );
    assert!(matches!(
        authority.reserve_batch_with_tenant_limit(
            vec![cross_tenant],
            TenantPublishedPortLimit::new(tenant_a.clone(), 1)
        ),
        Err(PortLeaseError::TenantLimitScopeMismatch {
            expected_tenant_id,
            actual_tenant_id,
            ..
        }) if expected_tenant_id == tenant_a && actual_tenant_id == tenant_b
    ));
    assert!(
        authority.list().expect("authority should list").is_empty(),
        "attribution failures must not mutate durable authority"
    );
}

fn assert_quota_rejects(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    limit: TenantPublishedPortLimit,
    expected_current_live: usize,
    expected_additional: usize,
) {
    assert!(matches!(
        authority.reserve_batch_with_tenant_limit(vec![request.clone()], limit),
        Err(PortLeaseError::TenantPublishedPortLimitExceeded {
            current_live,
            additional,
            ..
        }) if current_live == expected_current_live && additional == expected_additional
    ));
    assert!(
        authority
            .inspect(request.lease_id())
            .expect("rejected request should inspect")
            .is_none(),
        "quota rejection must not persist the new request"
    );
}

use nimbus_core::TenantId;
use std::fs;
use tempfile::tempdir;

use super::*;

fn fixture() -> (
    tempfile::TempDir,
    OciNetworkLayout,
    OciIpamAuthority,
    OciNetworkConfig,
    SandboxId,
) {
    let dir = tempdir().expect("temp dir");
    let tenant = TenantId::new("tenant-original").expect("tenant should parse");
    let sandbox = SandboxId::new("sandbox-original");
    let layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sandbox);
    let authority = OciIpamAuthority::reconstruct_for_direct_test(&layout)
        .expect("direct test authority should open");
    (dir, layout, authority, OciNetworkConfig::default(), sandbox)
}

#[test]
fn nnc5_2b_ipam_persists_reversible_provider_locator_before_effects() {
    let (_dir, layout, authority, config, sandbox) = fixture();
    allocate_container_ips(&authority, &layout, &config, &sandbox)
        .expect("IPAM reservation should commit before provider effects");

    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    let envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(authority_path).expect("authority should read"))
            .expect("authority should parse");
    let allocation = &envelope["body"]["records"]["tenant-ipam/tenant-original"]["allocations"]
        [config.attachment_id.as_str()];
    let locator = &allocation["provider_locator"];

    assert_eq!(
        locator["tenant_id"], "tenant-original",
        "the provider journal must authenticate its exact tenant partition"
    );
    assert_eq!(
        locator["sandbox_id"],
        serde_json::json!(sandbox),
        "the provider journal must retain the source sandbox needed to locate artifacts"
    );
    assert_eq!(
        locator["provider_kind"], "container",
        "the locator must authenticate the OCI provider family"
    );
    assert!(
        locator["artifact_realm_id"]
            .as_str()
            .is_some_and(|realm| realm.starts_with("oci-artifact-realm-v2-sha256:")),
        "the locator must carry a process-mappable artifact realm, not an inferred path: {locator}"
    );
}

#[test]
fn nnc5_2b_locator_replay_reopen_and_substitution_are_exact_and_byte_stable() {
    use super::super::provider_locator::OciAttachmentProviderKind;

    let (_dir, layout, authority, config, sandbox) = fixture();
    let first = allocate_container_ips(&authority, &layout, &config, &sandbox)
        .expect("first reservation should persist its locator");
    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    let committed = fs::read(&authority_path).expect("authority bytes should read");

    assert_eq!(
        allocate_container_ips(&authority, &layout, &config, &sandbox)
            .expect("exact replay should adopt"),
        first
    );
    assert_eq!(
        fs::read(&authority_path).expect("replayed authority should read"),
        committed,
        "exact replay must preserve locator and authority bytes"
    );

    let reopened = OciIpamAuthority::reconstruct_for_direct_test(&layout)
        .expect("fresh authority handle should reopen");
    let attachment_id = config.attachment_id.clone();
    let original_evidence = authority
        .get_attachment_provider_evidence(&layout.tenant_id, &attachment_id)
        .expect("original evidence should inspect")
        .expect("original evidence should exist");
    let reopened_evidence = reopened
        .get_attachment_provider_evidence(&layout.tenant_id, &attachment_id)
        .expect("reopened evidence should inspect")
        .expect("reopened evidence should exist");
    assert_eq!(
        reopened_evidence, original_evidence,
        "reopen must reconstruct the exact locator and provider attempt"
    );
    assert_eq!(
        fs::read(&authority_path).expect("reopened authority should read"),
        committed,
        "reopen must not rewrite authority"
    );

    let mut backend_substitution = config.clone();
    backend_substitution.provider_kind = OciAttachmentProviderKind::Krun;
    let backend_error =
        allocate_container_ips(&authority, &layout, &backend_substitution, &sandbox)
            .expect_err("backend substitution must fail");
    assert!(
        backend_error
            .to_string()
            .contains("different provider locator")
    );
    assert_eq!(
        fs::read(&authority_path).expect("backend rejection should preserve state"),
        committed
    );

    let substituted_workload_root = layout
        .workload_state_root
        .parent()
        .expect("fixture workload root should have a parent")
        .join("substituted-workload-root");
    fs::create_dir_all(&substituted_workload_root).expect("substituted workload root should exist");
    let substituted_layout = OciNetworkLayout::with_roots(
        &substituted_workload_root,
        &layout.network_state_root,
        &layout.tenant_id,
        &sandbox,
    );
    let root_error = allocate_container_ips(&authority, &substituted_layout, &config, &sandbox)
        .expect_err("artifact realm substitution must fail");
    assert!(
        root_error
            .to_string()
            .contains("different provider locator")
    );
    assert_eq!(
        fs::read(&authority_path).expect("realm rejection should preserve state"),
        committed
    );

    let mut segment_substitution = config.clone();
    segment_substitution.segment_id = NetworkSegmentId::generate().as_str().to_owned();
    let segment_error =
        allocate_container_ips(&authority, &layout, &segment_substitution, &sandbox)
            .expect_err("segment substitution must fail");
    assert!(
        segment_error
            .to_string()
            .contains("outside its current ordered tenant block set")
    );
    assert_eq!(
        fs::read(&authority_path).expect("segment rejection should preserve state"),
        committed
    );

    let mut claim_substitution = config.clone();
    claim_substitution.reservation_claim = test_reservation_claim("locator-foreign-claim");
    let claim_error = allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&claim_substitution),
        &sandbox,
        &claim_substitution.reservation_claim,
    )
    .expect_err("claim substitution must fail");
    assert!(
        claim_error
            .to_string()
            .contains("different launch coordinator")
    );
    assert_eq!(
        fs::read(&authority_path).expect("claim rejection should preserve state"),
        committed
    );
}

#[test]
fn terminal_ipam_release_rejects_a_substituted_artifact_realm_without_mutation() {
    let (dir, layout, authority, config, sandbox) = fixture();
    allocate_container_ips(&authority, &layout, &config, &sandbox)
        .expect("exact reserved IPAM should exist");
    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    let before = fs::read(&authority_path).expect("authority bytes should read");
    let foreign_workload_root = dir.path().join("foreign-workload-root");
    fs::create_dir_all(&foreign_workload_root).expect("foreign workload root should exist");
    let foreign_layout = OciNetworkLayout::with_roots(
        foreign_workload_root,
        &layout.network_state_root,
        &layout.tenant_id,
        &sandbox,
    );

    let error = deallocate_container_ips_after_confirmed_detach(
        &authority,
        &foreign_layout,
        &sandbox,
        &config.attachment_id,
        &config.reservation_claim,
        config.provider_kind(),
    )
    .expect_err("a foreign artifact realm must not publish terminal IPAM authority");
    assert!(
        error.to_string().contains("provider locator"),
        "terminal release must name the exact locator fence: {error}"
    );
    assert_eq!(
        fs::read(&authority_path).expect("authority bytes should reread"),
        before,
        "foreign-root rejection must not mutate live or terminal IPAM authority"
    );
}

#[test]
fn terminal_ipam_transition_replay_and_retirement_authenticate_realm_and_backend_for_both_safe_phases()
 {
    use super::super::provider_locator::OciAttachmentProviderKind;

    for detached in [false, true] {
        let phase = if detached { "detached" } else { "reserved" };
        let (dir, layout, authority, config, sandbox) = fixture();
        allocate_container_ips(&authority, &layout, &config, &sandbox)
            .expect("exact IPAM generation should reserve");
        if detached {
            let (_, setup) = begin_netavark_setup(&authority, &layout, &config, &sandbox)
                .expect("setup should prepare");
            begin_netavark_setup_execution(&authority, &layout, &config, &sandbox, &setup)
                .expect("setup execution should fence");
            complete_netavark_setup(&authority, &layout, &setup)
                .expect("setup should become ready");
            let teardown =
                match begin_netavark_teardown(&authority, &layout, &config, &sandbox, None)
                    .expect("teardown should prepare")
                {
                    NetavarkTeardownPlan::Run { claim, .. } => claim,
                    _ => panic!("ready provider should require teardown"),
                };
            begin_netavark_teardown_execution(&authority, &layout, &teardown)
                .expect("teardown execution should fence");
            confirm_netavark_provider_detached(&authority, &layout, &teardown)
                .expect("provider absence should confirm");
            complete_netavark_teardown(&authority, &layout, &teardown)
                .expect("teardown should become detached");
        }

        let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
        let foreign_workload_root = dir.path().join(format!("foreign-{phase}-workload-root"));
        fs::create_dir_all(&foreign_workload_root).expect("foreign workload root should exist");
        let foreign_layout = OciNetworkLayout::with_roots(
            foreign_workload_root,
            &layout.network_state_root,
            &layout.tenant_id,
            &sandbox,
        );

        for (label, candidate_layout, candidate_kind) in [
            (
                "foreign realm",
                &foreign_layout,
                OciAttachmentProviderKind::Container,
            ),
            ("foreign backend", &layout, OciAttachmentProviderKind::Krun),
        ] {
            let before = fs::read(&authority_path).expect("live authority bytes should read");
            let error = deallocate_container_ips_after_confirmed_detach(
                &authority,
                candidate_layout,
                &sandbox,
                &config.attachment_id,
                &config.reservation_claim,
                candidate_kind,
            )
            .expect_err("substituted terminal transition must fail");
            assert!(
                error.to_string().contains("provider locator"),
                "{phase} {label} transition must name its locator fence: {error}"
            );
            assert_eq!(
                fs::read(&authority_path).expect("live authority bytes should reread"),
                before,
                "{phase} {label} transition rejection must be byte-preserving"
            );
        }

        deallocate_container_ips_after_confirmed_detach(
            &authority,
            &layout,
            &sandbox,
            &config.attachment_id,
            &config.reservation_claim,
            config.provider_kind(),
        )
        .expect("exact terminal transition should succeed");

        for (label, candidate_layout, candidate_kind) in [
            (
                "foreign realm",
                &foreign_layout,
                OciAttachmentProviderKind::Container,
            ),
            ("foreign backend", &layout, OciAttachmentProviderKind::Krun),
        ] {
            let before = fs::read(&authority_path).expect("terminal authority bytes should read");
            let replay = deallocate_container_ips_after_confirmed_detach(
                &authority,
                candidate_layout,
                &sandbox,
                &config.attachment_id,
                &config.reservation_claim,
                candidate_kind,
            )
            .expect_err("substituted terminal replay must fail");
            assert!(
                replay.to_string().contains("provider locator"),
                "{phase} {label} replay must name its locator fence: {replay}"
            );
            let retirement = retire_terminal_container_ipam_release(
                &authority,
                candidate_layout,
                &sandbox,
                &config.attachment_id,
                &config.reservation_claim,
                candidate_kind,
            )
            .expect_err("substituted terminal retirement must fail");
            assert!(
                retirement.to_string().contains("provider locator"),
                "{phase} {label} retirement must name its locator fence: {retirement}"
            );
            assert_eq!(
                fs::read(&authority_path).expect("terminal authority bytes should reread"),
                before,
                "{phase} {label} replay/retirement rejection must be byte-preserving"
            );
        }

        assert!(
            retire_terminal_container_ipam_release(
                &authority,
                &layout,
                &sandbox,
                &config.attachment_id,
                &config.reservation_claim,
                config.provider_kind(),
            )
            .expect("exact terminal retirement should inspect"),
            "exact {phase} terminal witness should retire once"
        );
    }
}

#[test]
fn torn_ipam_state_fails_closed_with_the_authority_path() {
    let (_dir, layout, authority, config, sandbox) = fixture();
    allocate_container_ips(&authority, &layout, &config, &sandbox)
        .expect("original IP should allocate");
    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    fs::write(&authority_path, b"{").expect("torn state should be installed");

    let error = load_container_ips(&authority, &layout, &sandbox)
        .expect_err("torn IPAM JSON must fail closed");
    let rendered = error.to_string();
    assert!(
        rendered.contains("network authority state") && rendered.contains("corrupt"),
        "the failure must reach the checksummed authority boundary: {rendered}"
    );
    assert!(
        rendered.contains(&authority_path.display().to_string()),
        "the corruption diagnostic must name the affected authority path: {rendered}"
    );
}

#[test]
fn semantically_valid_ipam_state_corruption_must_not_reissue_a_live_ip() {
    let (_dir, layout, authority, config, original_sandbox) = fixture();
    let original = allocate_container_ips(&authority, &layout, &config, &original_sandbox)
        .expect("original IP should allocate");
    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&authority_path).expect("authority should read"))
            .expect("authority envelope should parse");
    envelope["body"]["records"]["tenant-ipam/tenant-original"]["allocations"] =
        serde_json::json!({});
    envelope["body"]["records"]["tenant-ipam/tenant-original"]["last_assigned_ip"] =
        serde_json::Value::Null;
    fs::write(
        &authority_path,
        serde_json::to_vec_pretty(&envelope).expect("tampered envelope should render"),
    )
    .expect("semantically corrupt IPAM state should be installed without checksum update");

    let replacement = allocate_container_ips(
        &authority,
        &layout,
        &config,
        &SandboxId::new("sandbox-replacement"),
    );
    match replacement.as_ref() {
        Ok(ips) => assert_eq!(
            ips, &original,
            "the unchecked corruption must expose the audited live-IP reuse"
        ),
        Err(error) => {
            let rendered = error.to_string();
            assert!(
                ["checksum", "corrupt", "integrity", "version"]
                    .iter()
                    .any(|needle| rendered.to_ascii_lowercase().contains(needle)),
                "a fixed store must reject corruption with a named integrity error: {rendered}"
            );
        }
    }
    assert!(
        replacement.is_err(),
        "semantically valid corruption must fail closed instead of reissuing a live IP"
    );
}

#[test]
fn stale_claim_cannot_load_or_delete_reallocated_same_attachment_ipam() {
    let (_dir, layout, authority, mut config, sandbox) = fixture();
    config.network_subnet = "10.89.0.0/30".to_owned();
    let first_claim = test_reservation_claim("first-generation");
    let second_claim = test_reservation_claim("second-generation");
    config.reservation_claim = first_claim.clone();

    let first = allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&config),
        &sandbox,
        &first_claim,
    )
    .expect("first generation should reserve IPAM");
    deallocate_container_ips_for_claim(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &first_claim,
        config.provider_kind(),
    )
    .expect("first generation should compare-delete its own IPAM");
    let mut replacement_config = config.clone();
    replacement_config.reservation_claim = second_claim.clone();
    let second = allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&replacement_config),
        &sandbox,
        &second_claim,
    )
    .expect("second generation should reserve replacement IPAM");

    let stale_load = load_container_ips_for_segment(&authority, &layout, &config, &sandbox)
        .expect_err("stale first-generation provider work must not load replacement IPAM");
    assert!(
        stale_load
            .to_string()
            .contains("different launch coordinator"),
        "the rejected provider observation must name its generation fence: {stale_load}"
    );
    let stale_error = deallocate_container_ips_for_claim(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &first_claim,
        config.provider_kind(),
    )
    .expect_err("stale first-generation cleanup must not delete replacement IPAM");
    assert!(
        stale_error.to_string().contains("stale launch coordinator"),
        "the rejected ABA cleanup must name its generation fence: {stale_error}"
    );
    let stale_confirmed_detach = deallocate_container_ips_after_confirmed_detach(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &first_claim,
        config.provider_kind(),
    )
    .expect_err("stale confirmed-detach cleanup must not delete replacement IPAM");
    assert!(
        stale_confirmed_detach
            .to_string()
            .contains("stale launch coordinator"),
        "confirmed-detach ABA rejection must name its generation fence: {stale_confirmed_detach}"
    );
    assert_eq!(
        load_container_ips_for_segment(&authority, &layout, &replacement_config, &sandbox)
            .expect("replacement IPAM should remain loadable"),
        second.ips,
        "stale cleanup must leave the replacement allocation byte-for-byte authoritative"
    );
    assert_eq!(
        first.ips, second.ips,
        "the ABA proof must reuse the same address so IP can never masquerade as generation identity"
    );

    deallocate_container_ips_after_confirmed_detach(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &second_claim,
        replacement_config.provider_kind(),
    )
    .expect("replacement generation should publish exact terminal evidence");
    assert!(
        authenticate_container_network_generation_for_cleanup(
            &authority,
            &layout,
            &replacement_config,
            &sandbox,
        )
        .expect("replacement terminal generation should authenticate")
        .is_none(),
        "terminal evidence must not imply that provider effects remain live"
    );
    let stale_terminal = authenticate_container_network_generation_for_cleanup(
        &authority, &layout, &config, &sandbox,
    )
    .expect_err("an old generation must not borrow a replacement's terminal tombstone");
    assert!(
        stale_terminal
            .to_string()
            .contains("different launch coordinator"),
        "terminal ABA rejection must name its generation fence: {stale_terminal}"
    );
    deallocate_container_ips_after_confirmed_detach(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &first_claim,
        config.provider_kind(),
    )
    .expect_err("stale cleanup must not accept a replacement terminal tombstone");
    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    let before_retirement = fs::read(&authority_path).expect("authority bytes should read");
    assert!(
        !retire_terminal_container_ipam_release(
            &authority,
            &layout,
            &sandbox,
            &config.attachment_id,
            &first_claim,
            config.provider_kind(),
        )
        .expect("stale retirement should inspect"),
        "a stale generation must not retire replacement terminal evidence"
    );
    assert_eq!(
        fs::read(&authority_path).expect("authority bytes should reread"),
        before_retirement,
        "rejected retirement must leave replacement authority byte-for-byte unchanged"
    );
    assert!(
        retire_terminal_container_ipam_release(
            &authority,
            &layout,
            &sandbox,
            &config.attachment_id,
            &second_claim,
            replacement_config.provider_kind(),
        )
        .expect("exact terminal retirement should succeed")
    );
}

#[test]
fn newer_never_realized_claim_supersedes_older_terminal_generation() {
    let (_dir, layout, authority, mut config, sandbox) = fixture();
    let first_claim = test_reservation_claim("completed-first-generation");
    let second_claim = test_reservation_claim("never-realized-second-generation");
    config.reservation_claim = first_claim.clone();
    allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&config),
        &sandbox,
        &first_claim,
    )
    .expect("first generation should reserve IPAM");
    deallocate_container_ips_after_confirmed_detach(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &first_claim,
        config.provider_kind(),
    )
    .expect("first generation should publish terminal evidence");

    deallocate_container_ips_for_claim(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &second_claim,
        config.provider_kind(),
    )
    .expect("authenticated newer no-effect cleanup should supersede old terminal evidence");
    let state = read_ipam_state(&authority, &layout).expect("IPAM authority should inspect");
    assert!(state.allocations.is_empty());
    assert!(
        state.released_allocations.is_empty(),
        "the newer generation never committed IPAM and must not inherit old retry history"
    );
    deallocate_container_ips_for_claim(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &second_claim,
        config.provider_kind(),
    )
    .expect("no-effect cleanup replay should be idempotent");
}

#[test]
fn completed_unique_attachment_churn_does_not_accumulate_terminal_ipam() {
    let dir = tempdir().expect("temp dir");
    let tenant = TenantId::new("tenant-ipam-churn").expect("tenant should parse");
    for index in 0..256 {
        let sandbox = SandboxId::new(format!("sandbox-ipam-churn-{index}"));
        let layout = OciNetworkLayout::under_root(dir.path(), &tenant, &sandbox);
        let authority = OciIpamAuthority::reconstruct_for_direct_test(&layout)
            .expect("direct test authority should open");
        let claim = test_reservation_claim(&format!("churn-{index}"));
        let config = OciNetworkConfig {
            reservation_claim: claim.clone(),
            ..OciNetworkConfig::default()
        };
        allocate_container_ips_on_first_available(
            &authority,
            &layout,
            std::slice::from_ref(&config),
            &sandbox,
            &claim,
        )
        .expect("churn generation should reserve IPAM");
        deallocate_container_ips_after_confirmed_detach(
            &authority,
            &layout,
            &sandbox,
            &config.attachment_id,
            &claim,
            config.provider_kind(),
        )
        .expect("provider-confirmed detach should publish retry evidence");
        assert!(
            retire_terminal_container_ipam_release(
                &authority,
                &layout,
                &sandbox,
                &config.attachment_id,
                &claim,
                config.provider_kind(),
            )
            .expect("durably final lifecycle should retire exact evidence")
        );
        let state = read_ipam_state(&authority, &layout).expect("IPAM authority should inspect");
        assert!(state.allocations.is_empty());
        assert!(
            state.released_allocations.is_empty(),
            "completed attachment churn must leave no historical retry ledger"
        );
    }
}

#[test]
fn startup_reconciliation_retires_only_terminal_manifest_ipam_evidence() {
    let dir = tempdir().expect("temp dir");
    let tenant = TenantId::new("tenant-split-reconciliation").expect("tenant should parse");
    let sandbox = SandboxId::new("sandbox-split-reconciliation");
    let layout = OciNetworkLayout::with_roots(
        dir.path().join("project-state"),
        dir.path().join("node-network-state"),
        &tenant,
        &sandbox,
    );
    layout
        .ensure_directories()
        .expect("split workload artifact root should exist before locator authentication");
    let authority = OciIpamAuthority::reconstruct_for_direct_test(&layout)
        .expect("direct test authority should open");
    let mut config = OciNetworkConfig::default();
    let claim = test_reservation_claim("startup-terminal-reconciliation");
    config.reservation_claim = claim.clone();
    allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&config),
        &sandbox,
        &claim,
    )
    .expect("generation should reserve IPAM");
    deallocate_container_ips_after_confirmed_detach(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &claim,
        config.provider_kind(),
    )
    .expect("provider detach should publish terminal retry evidence");
    let manifest_path = crate::artifact_paths::manifest_path(
        &layout.workload_state_root,
        &layout.tenant_id,
        &sandbox,
    );
    fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("manifest parent should create");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "handle": {"id": &sandbox, "tenant_id": &layout.tenant_id},
            "spec": {"tenant_id": &layout.tenant_id},
            "network_layout": &layout,
            "network_config": &config,
            "network_cleanup_complete": true,
            "launch_artifact": null,
            "launch_reservation_claim": null,
            "status": "failed"
        }))
        .expect("manifest projection should render"),
    )
    .expect("terminal manifest should write");
    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    assert!(authority_path.is_file());
    assert!(
        !LocalNetworkStateStore::authority_path_for(&layout.workload_state_root).exists(),
        "split startup reconciliation must not create network authority under workload state"
    );

    assert_eq!(
        reconcile_terminal_container_ipam_releases(&authority, &layout.workload_state_root)
            .expect("startup reconciliation should succeed"),
        1
    );
    let state = read_ipam_state(&authority, &layout).expect("IPAM authority should inspect");
    assert!(state.released_allocations.is_empty());
    assert_eq!(
        reconcile_terminal_container_ipam_releases(&authority, &layout.workload_state_root)
            .expect("startup reconciliation replay should succeed"),
        0
    );
}

#[test]
fn startup_reconciliation_retains_ipam_until_explicit_network_cleanup_finality() {
    let (_dir, layout, authority, mut config, sandbox) = fixture();
    let claim = test_reservation_claim("startup-incomplete-network-cleanup");
    config.reservation_claim = claim.clone();
    allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&config),
        &sandbox,
        &claim,
    )
    .expect("generation should reserve IPAM");
    deallocate_container_ips_after_confirmed_detach(
        &authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &claim,
        config.provider_kind(),
    )
    .expect("provider detach should publish terminal retry evidence");
    let manifest_path = crate::artifact_paths::manifest_path(
        &layout.workload_state_root,
        &layout.tenant_id,
        &sandbox,
    );
    fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("manifest parent should create");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "handle": {"id": &sandbox, "tenant_id": &layout.tenant_id},
            "spec": {"tenant_id": &layout.tenant_id},
            "network_layout": &layout,
            "network_config": &config,
            "network_cleanup_complete": false,
            "launch_artifact": null,
            "launch_reservation_claim": null,
            "status": "failed"
        }))
        .expect("manifest projection should render"),
    )
    .expect("terminal projection should write");

    assert_eq!(
        reconcile_terminal_container_ipam_releases(&authority, &layout.workload_state_root)
            .expect("incomplete cleanup should be a successful no-op"),
        0
    );
    let state = read_ipam_state(&authority, &layout).expect("IPAM authority should inspect");
    assert_eq!(
        state.released_allocations.len(),
        1,
        "terminal observed status must not retire retry evidence without durable cleanup finality"
    );
}

#[test]
fn startup_reconciliation_rejects_cross_root_manifest_without_mutation() {
    let trusted = tempdir().expect("trusted state root");
    let foreign = tempdir().expect("foreign state root");
    let tenant = TenantId::new("tenant-cross-root").expect("tenant should parse");
    let sandbox = SandboxId::new("sandbox-cross-root");
    let foreign_layout = OciNetworkLayout::under_root(foreign.path(), &tenant, &sandbox);
    let foreign_authority = OciIpamAuthority::reconstruct_for_direct_test(&foreign_layout)
        .expect("foreign direct test authority should open");
    let trusted_layout = OciNetworkLayout::under_root(trusted.path(), &tenant, &sandbox);
    let trusted_authority = OciIpamAuthority::reconstruct_for_direct_test(&trusted_layout)
        .expect("trusted direct test authority should open");
    let claim = test_reservation_claim("cross-root");
    let config = OciNetworkConfig {
        reservation_claim: claim.clone(),
        ..OciNetworkConfig::default()
    };
    allocate_container_ips_on_first_available(
        &foreign_authority,
        &foreign_layout,
        std::slice::from_ref(&config),
        &sandbox,
        &claim,
    )
    .expect("foreign generation should reserve IPAM");
    deallocate_container_ips_after_confirmed_detach(
        &foreign_authority,
        &foreign_layout,
        &sandbox,
        &config.attachment_id,
        &claim,
        config.provider_kind(),
    )
    .expect("foreign detach should publish terminal evidence");
    let authority_path = LocalNetworkStateStore::authority_path_for(foreign.path());
    let before = fs::read(&authority_path).expect("foreign authority should read");

    let copied_manifest_path =
        crate::artifact_paths::manifest_path(trusted.path(), &tenant, &sandbox);
    fs::create_dir_all(
        copied_manifest_path
            .parent()
            .expect("copied manifest parent"),
    )
    .expect("copied manifest parent should create");
    fs::write(
        &copied_manifest_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "handle": {"id": &sandbox, "tenant_id": &tenant},
            "spec": {"tenant_id": &tenant},
            "network_layout": &foreign_layout,
            "network_config": &config,
            "network_cleanup_complete": true,
            "launch_artifact": null,
            "launch_reservation_claim": null,
            "status": "failed"
        }))
        .expect("copied manifest should render"),
    )
    .expect("copied manifest should write");

    let error = reconcile_terminal_container_ipam_releases(&trusted_authority, trusted.path())
        .expect_err("embedded foreign state root must fail closed");
    assert!(
        error.to_string().contains("untrusted network layout"),
        "the rejected authority redirection must be explicit: {error}"
    );
    assert_eq!(
        fs::read(&authority_path).expect("foreign authority should reread"),
        before,
        "a copied manifest must not mutate another state root's authority"
    );
    assert_eq!(
        read_ipam_state(&foreign_authority, &foreign_layout)
            .expect("foreign IPAM should inspect")
            .released_allocations
            .len(),
        1,
        "foreign terminal evidence must remain intact"
    );
}

#[test]
fn existing_ipam_requires_the_exact_reservation_claim() {
    let (_dir, layout, authority, config, sandbox) = fixture();
    let owner = test_reservation_claim("owner");
    let stale = test_reservation_claim("stale");
    allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&config),
        &sandbox,
        &owner,
    )
    .expect("owner should reserve IPAM");
    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    let before = fs::read(&authority_path).expect("authority bytes should read");

    let error = allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&config),
        &sandbox,
        &stale,
    )
    .expect_err("a different coordinator must not adopt the existing allocation");
    assert!(
        error.to_string().contains("cross-generation adoption"),
        "claim mismatch must fail at the generation fence: {error}"
    );
    assert_eq!(
        fs::read(&authority_path).expect("authority bytes should reread"),
        before,
        "rejected cross-generation adoption must not rewrite authority state"
    );
}

#[test]
fn ipam_load_is_byte_stable() {
    let (_dir, layout, authority, mut config, sandbox) = fixture();
    config.attachment_id = default_network_attachment_id(&sandbox);
    let claim = test_reservation_claim("read-only-load");
    config.reservation_claim = claim.clone();
    let allocation = allocate_container_ips_on_first_available(
        &authority,
        &layout,
        std::slice::from_ref(&config),
        &sandbox,
        &claim,
    )
    .expect("fixture should reserve IPAM");
    let authority_path = LocalNetworkStateStore::authority_path_for(&layout.network_state_root);
    let before = fs::read(&authority_path).expect("authority bytes should read");

    assert_eq!(
        load_container_ips(&authority, &layout, &sandbox).expect("generic load should succeed"),
        allocation.ips
    );
    assert_eq!(
        load_container_ips_for_segment(&authority, &layout, &config, &sandbox)
            .expect("segment-fenced load should succeed"),
        allocation.ips
    );
    assert_eq!(
        fs::read(&authority_path).expect("authority bytes should reread"),
        before,
        "IPAM observation must not advance revision or rewrite durable state"
    );
}

#[test]
fn ipam_allocation_requires_a_reservation_claim() {
    let error = serde_json::from_value::<IpamState>(serde_json::json!({
        "allocations": {
            "netattach_01ARZ3NDEKTSV4RRFFQ69G5FAV": {
                "segment_id": "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "ips": ["10.89.0.2"]
            }
        },
        "last_assigned_ip": "10.89.0.2"
    }))
    .expect_err("claim-less durable IPAM must fail closed");
    assert!(
        error.to_string().contains("reservation_claim"),
        "schema rejection must name the missing generation fence: {error}"
    );
}

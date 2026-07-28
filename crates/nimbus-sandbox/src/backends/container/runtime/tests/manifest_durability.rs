//! Container manifest schema and crash-publication proofs.

use std::fs::OpenOptions;
use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::super::manifest::{
    ContainerSandboxManifest, MANIFEST_PUBLICATION_LOCK_FILE, MANIFEST_PUBLICATION_STAGE_FILE,
    establish_durable_manifest_directory_chain_with, publish_with_directory_sync,
};
use super::support::*;
use super::*;
use fs2::FileExt;
use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};
use tempfile::TempDir;

#[test]
fn manifest_schema_requires_launch_execution_context() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, manifest) =
        plan_only_manifest(temp_dir.path(), "required-runner-execution-context");
    let mut serialized =
        serde_json::to_value(&manifest).expect("container manifest should serialize");
    serialized
        .as_object_mut()
        .expect("container manifest should be an object")
        .remove("runner_config");

    let error = serde_json::from_value::<ContainerSandboxManifest>(serialized)
        .expect_err("missing launch-time execution context must fail closed");
    assert!(
        error.to_string().contains("runner_config"),
        "the missing authority-bearing field must be explicit: {error}"
    );
    assert!(
        backend
            .read_manifest(&manifest.handle.id)
            .expect("read-only inspection should remain available")
            .is_none(),
        "schema validation must not publish a manifest"
    );
}

#[test]
fn manifest_schema_requires_egress_reload_generations() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, manifest) = plan_only_manifest(temp_dir.path(), "required-egress-reload-state");
    let mut serialized =
        serde_json::to_value(&manifest).expect("container manifest should serialize");
    serialized
        .as_object_mut()
        .expect("container manifest should be an object")
        .remove("egress_policy_reload");

    let error = serde_json::from_value::<ContainerSandboxManifest>(serialized)
        .expect_err("missing egress reload generations must fail closed");
    assert!(
        error.to_string().contains("egress_policy_reload"),
        "the missing authority-bearing field must be explicit: {error}"
    );
    assert!(
        backend
            .read_manifest(&manifest.handle.id)
            .expect("read-only inspection should remain available")
            .is_none(),
        "schema validation must not publish a manifest"
    );
}

#[test]
fn first_manifest_publication_syncs_complete_ancestor_chain_before_stage_creation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let state_root = temp_dir.path().join("state");
    let tenant_id =
        nimbus_core::TenantId::new("durable-ancestor-tenant").expect("tenant id should validate");
    let sandbox_id = SandboxId::new("durable-ancestor-sandbox");
    let manifest_path = crate::artifact_paths::manifest_path(&state_root, &tenant_id, &sandbox_id);
    let state_dir = manifest_path
        .parent()
        .expect("manifest parent should exist");
    let stage = state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE);
    let mut synchronized = Vec::new();

    establish_durable_manifest_directory_chain_with(&state_root, state_dir, |path| {
        assert!(
            !stage.exists(),
            "every ancestor must be durable before the stage commit begins"
        );
        synchronized.push(path.to_path_buf());
        Ok(())
    })
    .expect("fresh manifest hierarchy should become durable");

    let mut expected = vec![
        state_root
            .parent()
            .expect("state root should have a parent")
            .to_path_buf(),
        state_root.clone(),
    ];
    let mut current = state_root.clone();
    for component in state_dir
        .strip_prefix(&state_root)
        .expect("state directory should belong to the state root")
        .components()
    {
        current.push(component.as_os_str());
        expected.push(current.clone());
    }
    assert_eq!(
        synchronized, expected,
        "publication must fsync the trusted parent and every manifest ancestor in top-down order"
    );
}

#[test]
fn ancestor_sync_failure_fences_manifest_before_stage_creation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let state_root = temp_dir.path().join("state");
    let tenant_id =
        nimbus_core::TenantId::new("failed-sync-tenant").expect("tenant id should validate");
    let sandbox_id = SandboxId::new("failed-sync-sandbox");
    let manifest_path = crate::artifact_paths::manifest_path(&state_root, &tenant_id, &sandbox_id);
    let state_dir = manifest_path
        .parent()
        .expect("manifest parent should exist");
    let failed_directory = state_root.join("tenants");
    let mut synchronized = Vec::new();

    let error = publish_with_directory_sync(
        &state_root,
        state_dir,
        &manifest_path,
        b"complete manifest bytes\n",
        |path| {
            synchronized.push(path.to_path_buf());
            if path == failed_directory {
                Err(std::io::Error::other("injected ancestor sync failure"))
            } else {
                Ok(())
            }
        },
    )
    .expect_err("ancestor durability failure must precede publication");

    assert!(
        error.to_string().contains("failed to durably establish")
            && error.to_string().contains("injected ancestor sync failure"),
        "the failed durability boundary must be explicit: {error}"
    );
    assert_eq!(
        synchronized,
        [
            temp_dir.path().to_path_buf(),
            state_root.clone(),
            failed_directory,
        ],
        "publication must stop at the exact failed ancestor"
    );
    assert!(
        !manifest_path.exists()
            && !state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE).exists()
            && !state_dir.join(MANIFEST_PUBLICATION_LOCK_FILE).exists(),
        "ancestor failure must precede lock, stage, and canonical manifest creation"
    );
}

#[test]
fn non_directory_manifest_ancestor_fails_closed_before_publication() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let state_root = temp_dir.path().join("state");
    std::fs::create_dir(&state_root).expect("state root should create");
    let tenants_path = state_root.join("tenants");
    std::fs::write(&tenants_path, b"not a directory").expect("invalid ancestor should create");
    let state_dir = tenants_path.join("tenant/sandboxes/sandbox/state/containers/sandbox");

    let error =
        establish_durable_manifest_directory_chain_with(&state_root, &state_dir, |_| Ok(()))
            .expect_err("a non-directory ancestor must fence manifest publication");

    assert!(
        error.to_string().contains("not a directory")
            && error
                .to_string()
                .contains(&tenants_path.display().to_string())
            && error.to_string().contains("publication remains fenced"),
        "the invalid ancestor diagnostic must preserve its exact path: {error}"
    );
}

#[test]
fn next_write_reconciles_exact_stages_without_promoting_them() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, manifest) = plan_only_manifest(temp_dir.path(), "next-write-stage-reconcile");
    let state_dir = &manifest.conmon_layout.container_state_dir;
    let fixed_stage = state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE);
    let retired_stage_name =
        state_dir.join(".nimbus-container-manifest.00000000000000000000000007.stage");
    let lookalike = state_dir.join(".nimbus-container-manifest.crash-orphan.stage");
    std::fs::write(&fixed_stage, b"fixed crash-cut bytes").expect("fixed stage should persist");
    std::fs::write(&retired_stage_name, b"unowned operator bytes")
        .expect("retired stage-shaped name should persist");
    std::fs::write(&lookalike, b"operator lookalike").expect("lookalike should persist");

    assert!(
        backend
            .read_manifest(&manifest.handle.id)
            .expect("read-only inspection should succeed")
            .is_none(),
        "stage bytes must never be promoted by read-side inspection"
    );
    assert!(fixed_stage.exists());

    backend
        .write_manifest(&manifest)
        .expect("the next writer should reconcile exact abandoned stages");
    assert!(!fixed_stage.exists());
    assert!(
        retired_stage_name.exists() && lookalike.exists(),
        "reconciliation must preserve every name outside the one canonical stage path"
    );
    assert_eq!(
        backend
            .read_manifest(&manifest.handle.id)
            .expect("canonical manifest should inspect")
            .expect("canonical manifest should exist"),
        manifest,
        "only the complete new writer may establish manifest.json"
    );
}

#[test]
fn startup_reconciles_a_stage_only_first_publication_crash() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = plan_only_config(temp_dir.path());
    let spec = sample_spec();
    let id = SandboxId::new("stage-only-startup-reconcile");
    let manifest_path =
        crate::artifact_paths::manifest_path(&config.state_root, &spec.tenant_id, &id);
    let state_dir = manifest_path
        .parent()
        .expect("manifest parent should exist");
    std::fs::create_dir_all(state_dir).expect("stage-only state directory should create");
    let stage = state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE);
    std::fs::write(&stage, b"complete but unpublished first-write bytes")
        .expect("first-write crash stage should persist");

    let backend = ContainerSandboxBackend::new(config);

    assert!(
        !stage.exists(),
        "startup must find and discard stage-only first-publication crash evidence"
    );
    backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("successful startup reconciliation should admit later planning");
    assert!(
        !manifest_path.exists(),
        "stage bytes must not become the canonical manifest during reconciliation"
    );
}

#[test]
fn startup_ignores_and_preserves_retired_unique_stage_names() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = plan_only_config(temp_dir.path());
    let spec = sample_spec();
    let id = SandboxId::new("retired-stage-name");
    let manifest_path =
        crate::artifact_paths::manifest_path(&config.state_root, &spec.tenant_id, &id);
    let state_dir = manifest_path
        .parent()
        .expect("manifest parent should exist");
    std::fs::create_dir_all(state_dir).expect("state directory should create");
    let retired_stage =
        state_dir.join(".nimbus-container-manifest.00000000000000000000000007.stage");
    std::fs::write(&retired_stage, b"unowned operator bytes")
        .expect("retired stage-shaped name should persist");

    let backend = ContainerSandboxBackend::new(config);

    assert_eq!(
        std::fs::read(&retired_stage).expect("retired name should remain untouched"),
        b"unowned operator bytes",
        "startup must not interpret the retired compatibility grammar as Nimbus state"
    );
    backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("retired unowned bytes must not fence new planning");
    assert!(
        retired_stage.exists() && !manifest_path.exists(),
        "planning must preserve the retired name without promoting it to manifest.json"
    );
}

#[test]
fn non_regular_exact_stage_fails_closed_but_read_remains_side_effect_free() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = plan_only_config(temp_dir.path());
    let spec = sample_spec();
    let id = SandboxId::new("non-regular-publication-stage");
    let manifest_path =
        crate::artifact_paths::manifest_path(&config.state_root, &spec.tenant_id, &id);
    let state_dir = manifest_path
        .parent()
        .expect("manifest parent should exist");
    std::fs::create_dir_all(state_dir).expect("container state directory should create");
    let stage = state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE);
    std::fs::create_dir(&stage).expect("non-regular exact stage should create");
    let backend = ContainerSandboxBackend::new(config);

    assert!(
        backend
            .read_manifest(&id)
            .expect("read-side inspection should remain available")
            .is_none(),
        "read-side inspection must not mutate or promote the invalid stage"
    );
    assert!(stage.is_dir());
    let error = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("publication-must-remain-fenced"),
            None,
            None,
        )
        .expect_err("new durable work must remain fenced");
    assert!(
        error.to_string().contains("not a regular file")
            && error.to_string().contains("publication remains fenced"),
        "startup must preserve the exact fail-closed diagnostic: {error}"
    );
    assert!(stage.is_dir(), "failure must preserve operator evidence");
}

#[test]
fn startup_reconciles_every_independent_state_directory_before_failing_closed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = plan_only_config(temp_dir.path());
    let spec = sample_spec();
    let invalid_ids = [
        SandboxId::new("aaa-invalid-publication-stage"),
        SandboxId::new("bbb-invalid-publication-stage"),
    ];
    for id in &invalid_ids {
        let manifest_path =
            crate::artifact_paths::manifest_path(&config.state_root, &spec.tenant_id, id);
        let state_dir = manifest_path
            .parent()
            .expect("manifest parent should exist");
        std::fs::create_dir_all(state_dir).expect("invalid state directory should create");
        std::fs::create_dir(state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE))
            .expect("non-regular stage should create");
    }
    let recoverable_id = SandboxId::new("zzz-recoverable-publication-stage");
    let recoverable_manifest_path =
        crate::artifact_paths::manifest_path(&config.state_root, &spec.tenant_id, &recoverable_id);
    let recoverable_state_dir = recoverable_manifest_path
        .parent()
        .expect("recoverable manifest parent should exist");
    std::fs::create_dir_all(recoverable_state_dir)
        .expect("recoverable state directory should create");
    let recoverable_stage = recoverable_state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE);
    std::fs::write(&recoverable_stage, b"abandoned first-publication bytes")
        .expect("recoverable stage should persist");

    let backend = ContainerSandboxBackend::new(config);
    let startup_error = backend
        .startup_reconciliation_error
        .as_ref()
        .expect("the two invalid directories must retain a startup fence");

    assert!(
        startup_error.contains("2 independent state directories"),
        "the aggregate must report every independently fenced state directory: {startup_error}"
    );
    for id in &invalid_ids {
        assert!(
            startup_error.contains(id.as_str()),
            "the aggregate must retain the exact sandbox path for {id}: {startup_error}"
        );
    }
    assert!(
        !recoverable_stage.exists(),
        "one fenced sandbox must not prevent safe reconciliation of an independent sandbox"
    );
}

#[test]
fn public_oci_plan_is_fenced_before_materialization_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = plan_only_config(temp_dir.path());
    let startup_owner = SandboxId::new("oci-plan-startup-fence");
    let mut spec = sample_spec();
    let startup_manifest_path =
        crate::artifact_paths::manifest_path(&config.state_root, &spec.tenant_id, &startup_owner);
    let startup_state_dir = startup_manifest_path
        .parent()
        .expect("startup manifest parent should exist");
    std::fs::create_dir_all(startup_state_dir).expect("startup state directory should create");
    std::fs::create_dir(startup_state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE))
        .expect("non-regular startup stage should create");
    spec.root = SandboxRootSpec::oci_image_build(
        "must-not-build",
        temp_dir.path().join("missing.Dockerfile"),
        temp_dir.path().join("missing-context"),
    );
    let backend = ContainerSandboxBackend::new(config.clone());

    let error = backend
        .plan_start(&spec)
        .expect_err("retained startup failure must win before OCI preparation");

    assert!(
        error
            .to_string()
            .contains("refuses new durable work because startup reconciliation did not complete")
            && !error.to_string().contains("missing.Dockerfile"),
        "the startup fence must be the exact primary diagnostic: {error}"
    );
    assert!(
        !config.state_root.join("image-cache").exists(),
        "readiness rejection must precede OCI cache creation"
    );
    assert_eq!(
        crate::artifact_paths::all_container_state_dirs(&config.state_root)
            .expect("container state directories should inspect"),
        [startup_state_dir.to_path_buf()],
        "readiness rejection must not create a generated sandbox rootfs or build session"
    );
}

#[test]
fn retained_startup_failure_fences_egress_reload_before_live_policy_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let proxy_port = unused_loopback_port();
    config.published_port_range = proxy_port..=proxy_port;
    let mut backend = ContainerSandboxBackend::new(config.clone());
    let manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("execute manifest should lower")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("baseline manifest should publish");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("execute plan should retain launch claim"),
            ),
        )
        .expect("baseline PEP should start");
    let readiness_before = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("baseline readiness should inspect")
        .expect("baseline PEP should be registered");

    let fenced_owner = SandboxId::new("reload-startup-fence");
    let fenced_manifest_path = crate::artifact_paths::manifest_path(
        &config.state_root,
        &manifest.spec.tenant_id,
        &fenced_owner,
    );
    let fenced_state_dir = fenced_manifest_path
        .parent()
        .expect("fenced manifest parent should exist");
    std::fs::create_dir_all(fenced_state_dir).expect("fenced state directory should create");
    std::fs::create_dir(fenced_state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE))
        .expect("non-regular stage should create");
    let failed_startup = ContainerSandboxBackend::new(config);
    backend.startup_reconciliation_error = Some(Arc::clone(
        failed_startup
            .startup_reconciliation_error
            .as_ref()
            .expect("invalid exact stage must retain startup failure"),
    ));

    let error = backend
        .reload_egress_policy(
            &manifest.handle.id,
            EgressPolicy::new([EgressRule::new(
                "must-not-activate",
                EgressProtocol::Https,
                "example.com",
                443,
            )]),
        )
        .expect_err("startup failure must fence live PEP mutation");
    let readiness_after = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("post-rejection readiness should inspect")
        .expect("baseline PEP should remain registered");
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("canonical manifest should inspect")
        .expect("canonical manifest should remain");

    assert!(
        error
            .to_string()
            .contains("refuses new durable work because startup reconciliation did not complete"),
        "the exact retained startup failure must remain primary: {error}"
    );
    assert_eq!(
        readiness_after, readiness_before,
        "rejected reload must preserve the active PEP policy generation"
    );
    assert!(
        persisted.spec.egress.is_deny_all(),
        "rejected reload must preserve canonical desired policy"
    );
}

#[test]
fn direct_predecision_egress_reload_cannot_create_a_provider_effect() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let proxy_port = unused_loopback_port();
    config.published_port_range = proxy_port..=proxy_port;
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("reload-before-direct-effect-fence"),
            None,
            None,
        )
        .expect("execute manifest should lower")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("predecision manifest should publish");
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("canonical predecision bytes should be readable");

    let error = backend
        .reload_egress_policy(
            &manifest.handle.id,
            EgressPolicy::new([EgressRule::new(
                "must-not-start-before-fence",
                EgressProtocol::Https,
                "example.com",
                443,
            )]),
        )
        .expect_err("predecision reload must not create a provider effect");

    assert!(
        error.to_string().contains("launch reservation")
            && error.to_string().contains("provider effects"),
        "rejection must name the exact missing effect fence: {error}"
    );
    assert!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("provider readiness should inspect")
            .is_none(),
        "rejected reload must not register a PEP"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("canonical predecision bytes should remain readable"),
        before,
        "rejected reload must not mutate desired or durable state"
    );
}

#[test]
fn reload_acknowledgement_before_completion_persistence_retains_durable_desired_intent() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let proxy_port = unused_loopback_port();
    config.published_port_range = proxy_port..=proxy_port;
    let baseline = ContainerSandboxBackend::new(config.clone());
    let mut manifest = baseline
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("reload-ack-before-completion-persistence"),
            None,
            None,
        )
        .expect("execute manifest should lower")
        .manifest;
    baseline
        .write_manifest(&manifest)
        .expect("baseline manifest should publish");
    baseline
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("execute plan should retain launch claim"),
            ),
        )
        .expect("baseline PEP should start");
    manifest.launch_reservation_claim = None;
    baseline
        .write_manifest(&manifest)
        .expect("running manifest should publish post-launch authority");

    let stage_path = manifest
        .conmon_layout
        .container_state_dir
        .join(MANIFEST_PUBLICATION_STAGE_FILE);
    let blocked_stage = stage_path.clone();
    let inject_once = Arc::new(AtomicBool::new(true));
    let observer_inject_once = Arc::clone(&inject_once);
    let backend = baseline.with_post_egress_reload_ack_observer(move || {
        if observer_inject_once.swap(false, Ordering::SeqCst) {
            std::fs::create_dir(&blocked_stage)
                .expect("post-acknowledgement stage blocker should create");
        }
    });
    let desired = EgressPolicy::new([EgressRule::new(
        "durable-reload-intent",
        EgressProtocol::Https,
        "example.com",
        443,
    )]);

    let error = backend
        .reload_egress_policy(&manifest.handle.id, desired.clone())
        .expect_err("completion publication should fail after provider acknowledgement");
    assert!(
        error.to_string().contains("not a regular file"),
        "the fault must occur at the exact completion-publication boundary: {error}"
    );
    std::fs::remove_dir(&stage_path).expect("stage blocker should remove");

    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("durable manifest should inspect")
        .expect("durable manifest should remain");
    assert_eq!(
        persisted.spec.egress, desired,
        "provider acknowledgement must never be the sole copy of the desired reload"
    );
    assert_eq!(
        persisted.egress_policy_reload.desired_generation().get(),
        2,
        "desired generation must advance in the pre-effect publication"
    );
    assert_eq!(
        persisted.egress_policy_reload.latest_attempt_generation(),
        1,
        "the exact provider attempt generation must survive lost completion acknowledgement"
    );
    assert!(
        persisted.egress_policy_reload.is_applying(),
        "failed completion publication must retain an applying reconciliation fence"
    );

    let acknowledged = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("acknowledged provider state should inspect")
        .expect("running PEP should remain registered");
    assert_eq!(
        acknowledged
            .policy_generation
            .map(|generation| generation.get()),
        Some(2),
        "the first exact reload attempt should advance the process-local PEP generation once"
    );

    backend
        .reload_egress_policy(&manifest.handle.id, desired.clone())
        .expect("retry should inspect and complete the exact acknowledged attempt");
    let completed = backend
        .read_manifest(&manifest.handle.id)
        .expect("completed manifest should inspect")
        .expect("completed manifest should remain");
    assert_eq!(completed.spec.egress, desired);
    assert!(
        !completed.egress_policy_reload.is_applying(),
        "exact provider inspection must durably complete the applying attempt"
    );
    assert_eq!(
        completed.egress_policy_reload.latest_attempt_generation(),
        1,
        "reconciling an acknowledged attempt must not mint another provider attempt"
    );
    let reconciled = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("reconciled provider state should inspect")
        .expect("running PEP should remain registered");
    assert_eq!(
        reconciled.policy_generation, acknowledged.policy_generation,
        "inspect-before-retry must not apply an already exact provider attempt twice"
    );
}

#[test]
fn retained_startup_failure_propagates_exit_receipt_inspection_errors() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("startup-fence-exit-inspection"),
            None,
            None,
        )
        .expect("execute manifest should lower")
        .manifest;
    manifest.launch_reservation_claim = None;
    backend
        .write_manifest(&manifest)
        .expect("canonical manifest should publish");
    std::fs::remove_dir(&manifest.conmon_layout.exit_dir)
        .expect("empty exit directory should be replaceable");
    std::fs::write(&manifest.conmon_layout.exit_dir, b"not a directory")
        .expect("non-directory obstacle should replace the exit directory");
    backend.startup_reconciliation_error = Some(Arc::from("injected startup fence"));

    let error = backend
        .inspect_sync(&manifest.handle.id)
        .expect_err("receipt metadata failure must not become absence");
    assert!(
        error
            .to_string()
            .contains("failed to inspect sandbox exit-status receipt")
            && error.to_string().contains(
                &manifest
                    .conmon_layout
                    .exit_status_file
                    .display()
                    .to_string()
            ),
        "inspection failure must preserve the exact artifact path: {error}"
    );
}

#[test]
fn publication_lock_contention_is_bounded_and_preserves_canonical_bytes() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest) = plan_only_manifest(temp_dir.path(), "bounded-publication-lock");
    backend
        .write_manifest(&manifest)
        .expect("baseline canonical manifest should publish");
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("baseline canonical bytes should read");
    manifest.shutdown_requested = true;
    manifest.status = SandboxStatus::Stopping;
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(MANIFEST_PUBLICATION_LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("publication lock should open");
    FileExt::lock_exclusive(&lock).expect("test should hold the publication lock");
    let started = Instant::now();

    let error = backend
        .write_manifest(&manifest)
        .expect_err("a contended publication must fail at its bounded deadline");
    let elapsed = started.elapsed();
    FileExt::unlock(&lock).expect("test publication lock should release");

    assert!(
        error
            .to_string()
            .contains("timed out acquiring container manifest publication lock"),
        "contention must expose a precise retryable diagnostic: {error}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "test-time lock acquisition must be bounded; elapsed {elapsed:?}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("canonical bytes should remain readable"),
        before,
        "timed-out publication must not mutate the prior commit point"
    );
    assert!(
        !manifest
            .conmon_layout
            .container_state_dir
            .join(MANIFEST_PUBLICATION_STAGE_FILE)
            .exists(),
        "lock timeout must not create a stage file"
    );
}

fn plan_only_manifest(
    root: &std::path::Path,
    id: &str,
) -> (ContainerSandboxBackend, ContainerSandboxManifest) {
    let backend = ContainerSandboxBackend::new(plan_only_config(root));
    let manifest = backend
        .plan_start_with_id(&sample_spec(), &SandboxId::new(id), None, None)
        .expect("plan-only manifest should lower without provider effects")
        .manifest;
    (backend, manifest)
}

fn plan_only_config(root: &std::path::Path) -> ContainerSandboxBackendConfig {
    ContainerSandboxBackendConfig {
        start_mode: ContainerStartMode::PlanOnly,
        ..ContainerSandboxBackendConfig::under_root(root)
    }
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("ephemeral listener should bind")
        .local_addr()
        .expect("ephemeral listener should expose address")
        .port()
}

use super::support::*;
use super::*;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

use crate::backends::oci::network::{OciSegmentAllocator, RecordingSegmentAllocator};
use nimbus_network::{ListenerId, LocalPortLeaseAuthority, NetworkResourceId};
use tempfile::TempDir;

fn split_plan_only_config(root: &Path) -> ContainerSandboxBackendConfig {
    ContainerSandboxBackendConfig::plan_only(root.join("bundles"), root.join("workload-state"))
        .with_network_state_root(root.join("network-state"))
}

fn read_manifest_for(
    backend: &ContainerSandboxBackend,
    id: &SandboxId,
) -> ContainerSandboxManifest {
    backend
        .read_manifest(id)
        .expect("preselected manifest lookup should succeed")
        .expect("preselected manifest should remain durable")
}

fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(base: &Path, current: &Path, snapshot: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        let mut entries = match std::fs::read_dir(current) {
            Ok(entries) => entries
                .map(|entry| entry.expect("test artifact entry should inspect"))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("test artifact directory should inspect: {error}"),
        };
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(base)
                .expect("test artifact should remain under its fixture root")
                .to_path_buf();
            let metadata = entry
                .metadata()
                .expect("test artifact metadata should inspect");
            if metadata.is_dir() {
                snapshot.insert(relative, None);
                visit(base, &path, snapshot);
            } else {
                snapshot.insert(
                    relative,
                    Some(std::fs::read(&path).expect("test artifact should read")),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn assert_preselected_rejection_has_no_effects(
    config: ContainerSandboxBackendConfig,
    spec: SandboxSpec,
    id: SandboxId,
    expected_error: &str,
) {
    let fixture_root = config
        .bundle_root
        .parent()
        .expect("fixture bundle root should have a parent")
        .to_path_buf();
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.82.0.0/24",
        82,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let network_root = config.network_state_root.clone();
    let backend = ContainerSandboxBackend::with_segment_allocator(config, injected);
    let port_authority =
        LocalPortLeaseAuthority::open(&network_root).expect("test port authority should open");
    let operations_before = recorder.operations();
    let leases_before = port_authority
        .list()
        .expect("test port authority should inspect");
    let artifacts_before = snapshot_tree(&fixture_root);

    let error = backend
        .prepare_plan_only_service_workload_with_id(spec, id)
        .expect_err("invalid preselected workload must fail before planning");

    assert!(
        error.to_string().contains(expected_error),
        "rejection should preserve the named admission reason: {error}"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "rejected preselected admission must not call the segment authority"
    );
    assert_eq!(
        port_authority
            .list()
            .expect("test port authority should remain inspectable"),
        leases_before,
        "rejected preselected admission must not mutate port authority"
    );
    assert_eq!(
        snapshot_tree(&fixture_root),
        artifacts_before,
        "rejected preselected admission must not create or rewrite workload artifacts"
    );
}

#[test]
fn preselected_service_workload_preserves_identity_across_every_handoff_artifact() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let config = split_plan_only_config(temp_dir.path());
    let workload_root = config.workload_state_root.clone();
    let tenant_id = sample_spec().tenant_id;
    let id = SandboxId::new("parent-selected-incarnation-01");
    let backend = ContainerSandboxBackend::new(config);

    let prepared = backend
        .prepare_plan_only_service_workload_with_id(
            sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 15_555, 8080)),
            id.clone(),
        )
        .expect("preselected service workload should prepare");

    assert_eq!(prepared.handle.id, id);
    let expected_manifest_path =
        crate::artifact_paths::manifest_path(&workload_root, &tenant_id, &id);
    assert!(
        expected_manifest_path.is_file(),
        "the canonical manifest path must be derived from the supplied identity"
    );
    let pointer_path = prepared.bundle_dir.join(RUNNER_MANIFEST_POINTER_FILE);
    assert_eq!(
        std::fs::read_to_string(&pointer_path)
            .expect("runner pointer should read")
            .trim(),
        expected_manifest_path.to_string_lossy(),
        "the runner pointer must hand off the exact preselected manifest"
    );

    let manifest = read_manifest_for(&backend, &id);
    assert_eq!(manifest.handle.id, id);
    assert_eq!(
        manifest.lifecycle_coordinator,
        ContainerLifecycleCoordinator::PreparedServiceRunner
    );
    assert_eq!(manifest.conmon_layout.manifest_path, expected_manifest_path);
    assert!(
        manifest
            .bundle_layout
            .bundle_dir
            .to_string_lossy()
            .contains(id.as_str())
            && manifest
                .network_layout
                .netns_path
                .to_string_lossy()
                .contains(id.as_str()),
        "bundle and network layout paths must use the supplied identity"
    );

    let expected_http_owner = NetworkResourceId::from(ListenerId::for_tenant_workload_listener(
        &tenant_id,
        id.as_str(),
        "published:http:8080",
    ));
    assert!(
        manifest
            .port_leases
            .iter()
            .any(|lease| lease.owner_id() == &expected_http_owner),
        "the published port lease owner must be derived from the supplied identity"
    );
    let expected_pep_owner = NetworkResourceId::from(ListenerId::for_tenant_workload_listener(
        &tenant_id,
        id.as_str(),
        "egress-pep",
    ));
    assert_eq!(
        manifest
            .egress_proxy
            .as_ref()
            .expect("prepared runner should own an egress PEP")
            .port_lease
            .owner_id(),
        &expected_pep_owner,
        "the internal listener owner must share the exact supplied workload identity"
    );
}

#[test]
fn preselected_service_workloads_keep_distinct_parent_identities() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = ContainerSandboxBackend::new(split_plan_only_config(temp_dir.path()));
    let first_id = SandboxId::new("parent-selected-distinct-a");
    let second_id = SandboxId::new("parent-selected-distinct-b");

    let first = backend
        .prepare_plan_only_service_workload_with_id(sample_spec(), first_id.clone())
        .expect("first preselected workload should prepare");
    let second = backend
        .prepare_plan_only_service_workload_with_id(sample_spec(), second_id.clone())
        .expect("second preselected workload should prepare");

    assert_eq!(first.handle.id, first_id);
    assert_eq!(second.handle.id, second_id);
    assert_ne!(first.handle.id, second.handle.id);
    assert_ne!(first.bundle_dir, second.bundle_dir);
    let first_manifest = read_manifest_for(&backend, &first_id);
    let second_manifest = read_manifest_for(&backend, &second_id);
    assert_ne!(
        first_manifest
            .egress_proxy
            .expect("first runner should own an egress PEP")
            .port_lease
            .owner_id(),
        second_manifest
            .egress_proxy
            .expect("second runner should own an egress PEP")
            .port_lease
            .owner_id(),
        "distinct parent-issued sandbox incarnations must not alias listener authority"
    );
}

#[test]
fn zero_binding_preselected_workload_has_exact_empty_publication_evidence() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = split_plan_only_config(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(65_000));
    let backend = ContainerSandboxBackend::new(config);
    let id = SandboxId::new("parent-selected-zero-binding");
    let prepared = backend
        .prepare_plan_only_service_workload_with_id(sample_spec(), id.clone())
        .expect("zero-binding preselected workload should prepare");
    let manifest = read_manifest_for(&backend, &id);

    assert!(prepared.handle.published_endpoints.is_empty());
    backend
        .persist_exposed_machine_port_receipts(&manifest, Vec::new())
        .expect("exact empty exposure must have a durable observed commit");
    assert!(
        backend
            .exposed_machine_port_receipts(&id)
            .expect("durable empty exposure should reload as exact observed evidence")
            .is_empty()
    );
    let forwarder = manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("machine-forwarded fixture should retain provider authority");
    backend
        .persist_absent_machine_port_receipts(
            &manifest.spec.tenant_id,
            &id,
            &manifest.spec.port_bindings,
            forwarder,
            Vec::new(),
        )
        .expect("exact empty absence must have a durable observed commit");
    assert!(
        backend
            .absent_machine_port_receipts(&id)
            .expect("durable empty absence should reload as exact observed evidence")
            .is_empty()
    );
    let absence = backend
        .absent_machine_port_evidence(&id)
        .expect("durable empty absence should authenticate")
        .expect("durable empty absence should remain present");
    assert_eq!(absence.tenant_id, manifest.spec.tenant_id);
    assert_eq!(absence.sandbox_id, id);
    assert!(absence.receipts.is_empty());
    assert!(
        manifest
            .conmon_layout
            .container_state_dir
            .join(".nimbus-machine-port-evidence.json")
            .is_file(),
        "an empty observed set still requires a durable authenticated header"
    );
}

#[test]
fn duplicate_preselected_service_workload_cannot_replace_durable_owner() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let config = split_plan_only_config(temp_dir.path());
    let network_root = config.network_state_root.clone();
    let backend = ContainerSandboxBackend::new(config);
    let id = SandboxId::new("parent-selected-durable-owner");
    let first = backend
        .prepare_plan_only_service_workload_with_id(sample_spec(), id.clone())
        .expect("first preselected workload should prepare");
    let manifest = read_manifest_for(&backend, &id);
    let manifest_path = manifest.conmon_layout.manifest_path.clone();
    let pointer_path = first.bundle_dir.join(RUNNER_MANIFEST_POINTER_FILE);
    let manifest_before = std::fs::read(&manifest_path).expect("first manifest should read");
    let pointer_before = std::fs::read(&pointer_path).expect("first pointer should read");
    let authority =
        LocalPortLeaseAuthority::open(&network_root).expect("test port authority should open");
    let leases_before = authority.list().expect("first leases should inspect");

    let error = backend
        .prepare_plan_only_service_workload_with_id(
            sample_spec_for_tenant("svc-demo", "replacement"),
            id.clone(),
        )
        .expect_err("a duplicate preselected identity must be rejected");

    assert!(
        error.to_string().contains("already has a durable manifest"),
        "duplicate diagnostics should identify the existing durable owner: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest_path).expect("original manifest should remain"),
        manifest_before,
        "a duplicate identity must not overwrite the original manifest"
    );
    assert_eq!(
        std::fs::read(&pointer_path).expect("original pointer should remain"),
        pointer_before,
        "a duplicate identity must not rewrite the runner handoff"
    );
    assert_eq!(
        authority
            .list()
            .expect("original leases should remain inspectable"),
        leases_before,
        "a duplicate identity must not mutate the original network claims"
    );
    assert_eq!(
        read_manifest_for(&backend, &id),
        manifest,
        "the original durable workload owner must remain authoritative"
    );
}

#[test]
fn concurrent_duplicate_preselected_identity_has_exactly_one_durable_owner() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let config = split_plan_only_config(temp_dir.path());
    let network_root = config.network_state_root.clone();
    let backend = Arc::new(ContainerSandboxBackend::new(config));
    let tenant_id = sample_spec().tenant_id;
    let id = SandboxId::new("parent-selected-concurrent-owner");
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for service_name in ["winner-a", "winner-b"] {
        let backend = Arc::clone(&backend);
        let barrier = Arc::clone(&barrier);
        let id = id.clone();
        workers.push(std::thread::spawn(move || {
            let spec = sample_spec_for_tenant("svc-demo", service_name);
            barrier.wait();
            backend.prepare_plan_only_service_workload_with_id(spec, id)
        }));
    }
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("duplicate worker should join"))
        .collect::<Vec<_>>();
    let success_count = results.iter().filter(|result| result.is_ok()).count();
    let failures = results
        .iter()
        .filter_map(|result| result.as_ref().err())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(
        success_count, 1,
        "one and only one concurrent caller may publish the durable owner: {failures:?}"
    );
    assert_eq!(failures.len(), 1);
    assert!(
        failures[0].contains("already")
            || failures[0].contains("owned")
            || failures[0].contains("conflict")
            || failures[0].contains("lifetime")
            || failures[0].contains("different launch reservation coordinator"),
        "the losing caller must report an authority conflict: {}",
        failures[0]
    );

    let manifest = read_manifest_for(&backend, &id);
    assert_eq!(manifest.handle.id, id);
    assert_eq!(manifest.spec.tenant_id, tenant_id);
    let authority =
        LocalPortLeaseAuthority::open(&network_root).expect("port authority should reopen");
    let records = authority.list().expect("winning authority should inspect");
    assert!(
        !records.is_empty(),
        "the winning owner must retain its durable listener authority"
    );
    assert!(
        records
            .iter()
            .all(|record| record.request().tenant_id() == Some(&tenant_id)),
        "the losing concurrent caller must not cross or replace tenant authority"
    );
}

#[test]
fn invalid_preselected_service_admission_has_zero_artifact_or_network_effects() {
    let execute_root = TempDir::new().expect("execute tempdir should build");
    assert_preselected_rejection_has_no_effects(
        ContainerSandboxBackendConfig::under_root(execute_root.path()),
        sample_spec(),
        SandboxId::new("wrong-mode-preselected"),
        "requires plan-only mode",
    );

    let standalone_root = TempDir::new().expect("standalone tempdir should build");
    let standalone_spec = SandboxSpec::new(
        sample_spec().tenant_id,
        SandboxOwnerSpec::standalone_named("not-service-owned"),
        SandboxBackendKind::Container,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new(PathBuf::from("/tmp/rootfs"))),
        SandboxProcessSpec::new(["/bin/true"]),
    );
    assert_preselected_rejection_has_no_effects(
        split_plan_only_config(standalone_root.path()),
        standalone_spec,
        SandboxId::new("standalone-preselected"),
        "requires service owner metadata",
    );

    let empty_id_root = TempDir::new().expect("empty-id tempdir should build");
    assert_preselected_rejection_has_no_effects(
        split_plan_only_config(empty_id_root.path()),
        sample_spec(),
        SandboxId::new(""),
        "identity cannot be empty",
    );
}

#[test]
fn automatic_service_workload_identity_remains_generated_and_concrete() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let config = split_plan_only_config(temp_dir.path());
    let workload_root = config.workload_state_root.clone();
    let tenant_id = sample_spec().tenant_id;
    let backend = ContainerSandboxBackend::new(config);

    let prepared = backend
        .prepare_plan_only_service_workload(sample_spec())
        .expect("automatic service workload should retain its existing behavior");

    assert!(
        !prepared.handle.id.as_str().is_empty() && prepared.handle.id.as_str().starts_with("db-"),
        "automatic service workload identity must remain a concrete generated db-* incarnation: {}",
        prepared.handle.id
    );
    let manifest_path =
        crate::artifact_paths::manifest_path(&workload_root, &tenant_id, &prepared.handle.id);
    assert!(manifest_path.is_file());
    assert_eq!(
        std::fs::read_to_string(prepared.bundle_dir.join(RUNNER_MANIFEST_POINTER_FILE))
            .expect("automatic runner pointer should read")
            .trim(),
        manifest_path.to_string_lossy(),
        "the existing automatic path must still publish its exact runner handoff"
    );
}

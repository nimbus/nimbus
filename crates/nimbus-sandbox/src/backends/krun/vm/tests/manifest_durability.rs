//! First-publication durability proofs for krun launch authority.

use super::support::*;

use crate::backends::oci::conmon::OciConmonLayout;

fn initial_manifest(
    backend: &KrunSandboxBackend,
    tenant: &str,
    sandbox: &str,
) -> KrunSandboxManifest {
    let spec = sample_spec_for_tenant(tenant, sandbox);
    let sandbox_id = SandboxId::new(sandbox);
    let mut manifest = sample_manifest(spec.clone(), KrunStartMode::Execute);
    manifest.handle.id = sandbox_id.clone();
    manifest.conmon_layout = OciConmonLayout::new_for_tenant(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    );
    manifest
}

#[test]
fn initial_manifest_syncs_full_trusted_ancestor_chain_before_attachment_reservation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let manifest = initial_manifest(&backend, "durable-krun-tenant", "durable-krun-sandbox");
    let stage_prefix = ".nimbus-krun-manifest.";
    let mut synchronized = Vec::new();

    backend
        .create_manifest_with_directory_sync(&manifest, |path| {
            if !manifest.conmon_layout.manifest_path.exists() {
                assert!(
                    std::fs::read_dir(&manifest.conmon_layout.container_state_dir)
                        .map(
                            |entries| entries.filter_map(std::result::Result::ok).all(|entry| {
                                !entry
                                    .file_name()
                                    .to_string_lossy()
                                    .starts_with(stage_prefix)
                            })
                        )
                        .unwrap_or(true),
                    "every ancestor must be durable before the first stage is created"
                );
            }
            synchronized.push(path.to_path_buf());
            Ok(())
        })
        .expect("first manifest should durably publish");

    let state_dir = &manifest.conmon_layout.container_state_dir;
    let mut expected = vec![
        backend
            .config
            .workload_state_root
            .parent()
            .expect("state root should have a parent")
            .to_path_buf(),
        backend.config.workload_state_root.clone(),
    ];
    let mut current = backend.config.workload_state_root.clone();
    for component in state_dir
        .strip_prefix(&backend.config.workload_state_root)
        .expect("manifest directory should belong to state root")
        .components()
    {
        current.push(component.as_os_str());
        expected.push(current.clone());
    }
    expected.push(state_dir.clone());
    assert_eq!(
        synchronized, expected,
        "the trusted parent and every manifest ancestor must sync top-down before the final \
         commit-point acknowledgement"
    );
    assert!(
        manifest.conmon_layout.manifest_path.is_file(),
        "the exact durable owner record must exist before attachment reservation can follow"
    );
}

#[test]
fn initial_manifest_ancestor_sync_failure_prevents_attachment_reservation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let manifest = initial_manifest(
        &backend,
        "failed-durable-krun-tenant",
        "failed-durable-krun-sandbox",
    );
    let failed_directory = backend.config.workload_state_root.join("tenants");
    let mut synchronized = Vec::new();

    let error = backend
        .create_manifest_with_directory_sync(&manifest, |path| {
            synchronized.push(path.to_path_buf());
            if path == failed_directory {
                Err(std::io::Error::other("injected krun ancestor sync failure"))
            } else {
                Ok(())
            }
        })
        .expect_err("ancestor durability failure must precede launch authority publication");

    assert!(
        error.to_string().contains("failed to durably establish")
            && error
                .to_string()
                .contains(&failed_directory.display().to_string())
            && error
                .to_string()
                .contains("injected krun ancestor sync failure"),
        "the exact failed durability boundary must remain explicit: {error}"
    );
    assert_eq!(
        synchronized,
        [
            temp_dir.path().to_path_buf(),
            backend.config.workload_state_root.clone(),
            failed_directory,
        ],
        "publication must stop at the failed ancestor before any attachment authority can exist"
    );
    assert!(
        !manifest.conmon_layout.manifest_path.exists(),
        "no durable launch owner may appear after ancestor failure"
    );
    assert!(
        std::fs::read_dir(&manifest.conmon_layout.container_state_dir)
            .map(|entries| entries
                .filter_map(std::result::Result::ok)
                .all(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".nimbus-krun-manifest.")))
            .unwrap_or(true),
        "ancestor failure must precede all stage-file creation"
    );
}

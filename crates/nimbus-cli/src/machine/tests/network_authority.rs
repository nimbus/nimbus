use std::fs;

use nimbus_machine::{
    CURRENT_MACHINE_CONFIG_VERSION, MachineConfigRecord, MachineForwarderAuthority,
    MachineGuestConfig, MachineGuestProvisioning, MachineImageSource,
    MachineNetworkAuthorityRecord, MachineProvider, MachineResources, MachineRootLayout,
    MachineStateRecord,
};
use nimbus_network::{LocalNetworkStateStore, NetworkResourceGeneration};
use nimbus_sandbox::backends::container::OciMachinePortForwarderConfig;
use tempfile::TempDir;

use super::super::command::{MachineCommand, MachineStartCommand, MachineSubcommand};
use super::super::files::{load_initialized_machine, write_json_file};
use super::super::handlers::delete_machine_with_layout;
use super::super::network_composition::HostMachineNetworkComposition;

const MACHINE_NAME: &str = "authority-fixture";

fn artifact_roots(base: &std::path::Path) -> MachineRootLayout {
    MachineRootLayout::new(
        base.join("config"),
        base.join("state"),
        base.join("data"),
        base.join("cache"),
        base.join("runtime"),
    )
}

fn config(
    roots: &MachineRootLayout,
    network_authority: MachineNetworkAuthorityRecord,
) -> MachineConfigRecord {
    MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: MACHINE_NAME.to_owned(),
        provider: MachineProvider::Krunkit,
        guest: MachineGuestConfig {
            image_source: MachineImageSource::LocalDisk {
                path: roots.data_root.join("fixture.raw"),
            },
            provisioning: MachineGuestProvisioning::Ignition,
            ssh_user: "core".to_owned(),
            ssh_identity_path: None,
            ignition_file_path: None,
            efi_variable_store_path: None,
        },
        resources: MachineResources {
            cpus: 2,
            memory_mib: 2_048,
            disk_gib: 20,
        },
        volumes: Vec::new(),
        roots: roots.clone(),
        network_authority,
    }
}

fn provider_instance(scope: &str) -> nimbus_network::NetworkProviderHandle {
    OciMachinePortForwarderConfig::gvproxy_provider_handle(format!("nnc4.6e-test:{scope}"))
        .expect("fixture provider identity should validate")
}

#[test]
#[serial_test::serial]
fn machine_config_rejects_foreign_network_authority_before_state_or_artifact_mutation() {
    let temp = TempDir::new().expect("fixture root should exist");
    let composition = HostMachineNetworkComposition::claim_at(&temp.path().join("active-network"))
        .expect("active parent authority should be claimed");
    let network = composition.authority();
    let roots = artifact_roots(&temp.path().join("artifacts"));
    let paths = roots.paths(MACHINE_NAME);
    let attempted_root = temp.path().join("foreign-network");
    let attempted_authority = LocalNetworkStateStore::authority_path_for(&attempted_root);
    let persisted = config(
        &roots,
        MachineNetworkAuthorityRecord::new(
            attempted_authority.clone(),
            provider_instance("foreign"),
        )
        .expect("foreign provenance fixture should validate lexically"),
    );
    let state = MachineStateRecord::initialized();
    write_json_file(&paths.config_path, &persisted).expect("config fixture should write");
    write_json_file(&paths.state_path, &state).expect("state fixture should write");
    fs::create_dir_all(&paths.data_dir).expect("data fixture should exist");
    let marker = paths.data_dir.join("must-survive");
    fs::write(&marker, b"unchanged").expect("marker should write");
    let state_before = fs::read(&paths.state_path).expect("state bytes should read");

    let error = load_initialized_machine(&roots, &network, MACHINE_NAME)
        .expect_err("foreign manager provenance must fail before refresh or state write");
    let rendered = error.to_string();
    assert!(
        rendered.contains(&network.authority_path().display().to_string()),
        "error must include the active authority path: {rendered}"
    );
    assert!(
        rendered.contains(&attempted_authority.display().to_string()),
        "error must include the attempted authority path: {rendered}"
    );
    assert_eq!(
        fs::read(&paths.state_path).expect("state should remain readable"),
        state_before,
        "provenance rejection must precede state refresh/write"
    );
    assert_eq!(
        fs::read(&marker).expect("artifact marker should survive"),
        b"unchanged"
    );
    assert!(
        !attempted_root.exists(),
        "authentication must not create the attempted authority root"
    );
}

#[test]
#[serial_test::serial]
fn machine_delete_uses_persisted_manager_provenance_not_substituted_caller_roots() {
    let temp = TempDir::new().expect("fixture root should exist");
    let composition = HostMachineNetworkComposition::claim_at(&temp.path().join("network"))
        .expect("active parent authority should be claimed");
    let network = composition.authority();
    let persisted_roots = artifact_roots(&temp.path().join("persisted-artifacts"));
    let persisted_paths = persisted_roots.paths(MACHINE_NAME);
    let persisted = config(
        &persisted_roots,
        network
            .new_machine_record(MACHINE_NAME)
            .expect("persisted manager provenance should build"),
    );
    write_json_file(&persisted_paths.config_path, &persisted)
        .expect("persisted config should write");
    write_json_file(
        &persisted_paths.state_path,
        &MachineStateRecord::initialized(),
    )
    .expect("persisted state should write");
    fs::create_dir_all(&persisted_paths.data_dir).expect("persisted data should exist");
    let marker = persisted_paths.data_dir.join("must-survive");
    fs::write(&marker, b"persisted").expect("marker should write");

    let substituted_base = temp.path().join("substituted-artifacts");
    let substituted_roots = MachineRootLayout::new(
        persisted_roots.config_root.clone(),
        substituted_base.join("state"),
        substituted_base.join("data"),
        substituted_base.join("cache"),
        substituted_base.join("runtime"),
    );

    let error = delete_machine_with_layout(MACHINE_NAME, &substituted_roots, &network)
        .expect_err("substituted caller roots must fail before lock, lease, or deletion");
    assert!(
        error.to_string().contains("artifact root"),
        "typed rejection should identify artifact-root provenance: {error}"
    );
    assert!(
        persisted_paths.config_path.exists() && persisted_paths.state_path.exists(),
        "persisted config and state must survive a substituted delete"
    );
    assert_eq!(
        fs::read(&marker).expect("persisted marker should survive"),
        b"persisted"
    );
    assert!(
        !substituted_base.exists(),
        "rejection must precede creation of substituted lock/state/runtime roots"
    );
}

#[test]
#[serial_test::serial]
fn machine_delete_rejects_fenced_publication_until_exact_terminal_reconciliation() {
    let temp = TempDir::new().expect("fixture root should exist");
    let composition = HostMachineNetworkComposition::claim_at(&temp.path().join("network"))
        .expect("active parent authority should be claimed");
    let network = composition.authority();
    let roots = artifact_roots(&temp.path().join("artifacts"));
    let paths = roots.paths(MACHINE_NAME);
    let persisted = config(
        &roots,
        network
            .new_machine_record(MACHINE_NAME)
            .expect("persisted manager provenance should build"),
    );
    write_json_file(&paths.config_path, &persisted).expect("persisted config should write");
    write_json_file(&paths.state_path, &MachineStateRecord::initialized())
        .expect("persisted state should write");
    fs::create_dir_all(&paths.data_dir).expect("persisted data should exist");
    let marker = paths.data_dir.join("must-survive-fenced-publication");
    fs::write(&marker, b"fenced").expect("marker should write");

    let store = network
        .machine_publications()
        .expect("publication authority should open");
    let forwarder_authority = MachineForwarderAuthority::new(
        persisted.network_authority.provider_instance().clone(),
        NetworkResourceGeneration::new(1),
    );
    let intent = store
        .stage_service_attempt(
            &nimbus::TenantId::new("tenant-delete-fence").expect("tenant fixture should validate"),
            "api",
            &forwarder_authority,
            &[],
        )
        .expect("publication intent should stage");
    store
        .commit_before_machine_api(&intent.plan_id)
        .expect("publication intent should become ambiguous");

    let error = delete_machine_with_layout(MACHINE_NAME, &roots, &network)
        .expect_err("nonterminal publication must fence artifact deletion");
    assert!(
        error
            .to_string()
            .contains("nonterminal parent publication plans"),
        "{error}"
    );
    assert_eq!(
        fs::read(&marker).expect("fenced marker should survive"),
        b"fenced"
    );
    assert!(
        paths.config_path.exists() && paths.state_path.exists(),
        "rejected deletion must preserve machine records"
    );

    store
        .mark_terminal(&intent.plan_id)
        .expect("exact reconciliation should terminally settle the publication");
    delete_machine_with_layout(MACHINE_NAME, &roots, &network)
        .expect("terminal publication should permit deterministic deletion");
    assert!(
        !paths.config_path.exists() && !paths.state_path.exists() && !paths.data_dir.exists(),
        "successful deletion must remove only machine artifact roots"
    );
    assert!(
        network.machine_publications().is_ok(),
        "machine deletion must not remove the separate parent network authority"
    );
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn embedded_machine_lifecycle_retains_parent_authority_across_alias_retarget() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().expect("fixture root should exist");
    let original_root = temp.path().join("network-a");
    let replacement_root = temp.path().join("network-b");
    fs::create_dir_all(&original_root).expect("original authority root should exist");
    fs::create_dir_all(&replacement_root).expect("replacement authority root should exist");
    let alias = temp.path().join("network-current");
    symlink(&original_root, &alias).expect("authority alias should exist");
    let composition = HostMachineNetworkComposition::claim_at(&alias)
        .expect("embedded parent authority should be claimed through the alias");
    let network = composition.authority();
    let original_authority_path = network.authority_path().to_path_buf();

    fs::remove_file(&alias).expect("old alias should be removed");
    symlink(&replacement_root, &alias).expect("alias should retarget");

    assert_eq!(
        network.authority_path(),
        original_authority_path,
        "retained embedded authority must not follow a later alias retarget"
    );
    assert!(
        HostMachineNetworkComposition::claim_at(&alias).is_err(),
        "a retargeted alias cannot replace the retained process composition"
    );
    assert!(
        !LocalNetworkStateStore::authority_path_for(&replacement_root).exists(),
        "rejected substitution must not initialize the replacement authority"
    );
}

#[test]
#[serial_test::serial]
fn wsl2_refuses_machine_network_composition_before_authority_mutation() {
    let temp = TempDir::new().expect("fixture root should exist");
    let roots = artifact_roots(&temp.path().join("artifacts"));
    let attempted_network_root = temp.path().join("network-authority");
    let command = MachineCommand {
        command: MachineSubcommand::Start(MachineStartCommand::default()),
    };

    let error =
        super::super::handlers::reject_provider_managed_networking_before_composition_for_test(
            &command.command,
            &roots,
            MachineProvider::Wsl2,
        )
        .expect_err("provider-managed WSL2 must fail before host network composition");

    assert!(
        error.to_string().contains("WSL2"),
        "the existing named provider-unavailable error must be preserved: {error}"
    );
    assert!(
        !attempted_network_root.exists(),
        "provider rejection must precede manager authority creation"
    );
    assert!(
        !roots.config_root.exists()
            && !roots.state_root.exists()
            && !roots.data_root.exists()
            && !roots.cache_root.exists()
            && !roots.runtime_root.exists(),
        "provider rejection must precede machine artifacts and provider effects"
    );
}

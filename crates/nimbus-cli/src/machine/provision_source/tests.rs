use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use nimbus_machine::{
    CURRENT_MACHINE_CONFIG_VERSION, MachineConnectivityCapabilities, MachineForwarderAuthority,
    MachineGuestConfig, MachineGuestProvisioning, MachineHelperBinaryPaths, MachineImageSource,
    MachineManagerState, MachineProvider, MachineResources, MachineRootLayout, MachineRuntimeState,
    MachineStateRecord,
};
use nimbus_network::{
    ListenerId, NetworkAttachmentCapabilitySet, NetworkAttachmentMode, NetworkControlPlaneLocality,
    NetworkExposure, NetworkIsolationMode, NetworkManagementMode, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, NetworkSovereigntyCapabilities,
};
use nimbus_operator::LocalNodeNetworkRoot;
use nimbus_workloads::NodeIdentity;
use tempfile::TempDir;

use super::*;
use crate::machine::files::write_json_file;
use crate::machine::manager::start_machine_with_expected_forwarder_authority;
use crate::machine::network_composition::HostMachineNetworkComposition;
use crate::network_composition::{
    StagedLocalNetworkComposition, prepare_forwarded_server_profile_for_test,
};

struct Fixture {
    temp: TempDir,
    _composition: HostMachineNetworkComposition,
    network: HostMachineNetworkAuthority,
    roots: MachineRootLayout,
    config: MachineConfigRecord,
}

impl Fixture {
    fn new(provider: MachineProvider, state: MachineStateRecord) -> Self {
        #[cfg(unix)]
        let temp = TempDir::new_in("/tmp").expect("short fixture root should exist");
        #[cfg(not(unix))]
        let temp = TempDir::new().expect("fixture root should exist");
        let composition =
            HostMachineNetworkComposition::claim_at(&temp.path().join("network-authority"))
                .expect("test network authority should be claimed");
        let network = composition.authority();
        let roots = MachineRootLayout::new(
            temp.path().join("machine/config"),
            temp.path().join("machine/state"),
            temp.path().join("machine/data"),
            temp.path().join("machine/cache"),
            temp.path().join("machine/runtime"),
        );
        let paths = roots.paths(DEFAULT_MACHINE_NAME);
        let config = machine_config(&roots, &network, provider);
        write_json_file(&paths.config_path, &config).expect("config fixture should persist");
        write_json_file(&paths.state_path, &state).expect("state fixture should persist");
        Self {
            temp,
            _composition: composition,
            network,
            roots,
            config,
        }
    }

    fn prepare(&self) -> PreparedDefaultMachineProvisionSource {
        prepare_default_machine_provision_source_at_with(
            &self.roots,
            &self.network,
            NodeIdentity::new("machine-source-node").expect("node fixture should validate"),
            |provider| {
                assert!(
                    matches!(provider, MachineProvider::Krunkit | MachineProvider::Vfkit),
                    "only host-managed providers may reach capability resolution"
                );
                Ok(source_connectivity())
            },
        )
        .expect("host-managed source should prepare")
    }
}

fn machine_config(
    roots: &MachineRootLayout,
    network: &HostMachineNetworkAuthority,
    provider: MachineProvider,
) -> MachineConfigRecord {
    MachineConfigRecord {
        version: CURRENT_MACHINE_CONFIG_VERSION,
        name: DEFAULT_MACHINE_NAME.to_owned(),
        provider,
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
        network_authority: network
            .new_machine_record(DEFAULT_MACHINE_NAME)
            .expect("machine network provenance should validate"),
    }
}

fn source_connectivity() -> MachineConnectivityCapabilities {
    MachineConnectivityCapabilities::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::VirtualMachineGuest],
            [NetworkIsolationMode::WorkloadNamespace],
        ),
        [NetworkExposure::Loopback],
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

fn authority(config: &MachineConfigRecord, generation: u64) -> MachineForwarderAuthority {
    MachineForwarderAuthority::new(
        config.network_authority.provider_instance().clone(),
        NetworkResourceGeneration::new(generation),
    )
}

fn crossed_authority(generation: u64) -> MachineForwarderAuthority {
    MachineForwarderAuthority::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("crossed-machine-source"),
            "crossed-machine-source-provider",
        )
        .expect("crossed provider fixture should validate"),
        NetworkResourceGeneration::new(generation),
    )
}

fn runtime(paths: &MachinePaths, authority: MachineForwarderAuthority) -> MachineRuntimeState {
    MachineRuntimeState {
        helper_binaries: MachineHelperBinaryPaths {
            vmm: PathBuf::from("/fixture/vmm"),
            gvproxy: PathBuf::from("/fixture/gvproxy"),
        },
        image_path: paths.materialized_image_path.clone(),
        efi_variable_store_path: paths.efi_variable_store_path.clone(),
        machine_image_source: "fixture".to_owned(),
        ssh_listener_id: ListenerId::for_workload_listener("machine-source-fixture", "ssh"),
        forwarder_authority: authority,
        ssh_port: 22_222,
        rest_uri: "none://fixture".to_owned(),
        ready_vsock_port: 1_025,
    }
}

fn stopped_state_with_runtime(
    paths: &MachinePaths,
    authority: MachineForwarderAuthority,
) -> MachineStateRecord {
    let mut state = MachineStateRecord::initialized();
    state.manager = MachineManagerState::Stale;
    state.runtime = Some(runtime(paths, authority));
    state
}

fn running_state(paths: &MachinePaths, authority: MachineForwarderAuthority) -> MachineStateRecord {
    let mut state = MachineStateRecord::initialized();
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.runtime = Some(runtime(paths, authority));
    state
}

fn persist_running_state(
    paths: &MachinePaths,
    state: &mut MachineStateRecord,
    authority: &MachineForwarderAuthority,
) -> Result<(), Error> {
    state.lifecycle = MachineLifecycle::Running;
    state.manager = MachineManagerState::Ready;
    state.runtime = Some(runtime(paths, authority.clone()));
    state.last_error = None;
    write_json_file(&paths.state_path, state)
}

fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    fn visit(base: &Path, path: &Path, snapshot: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
        let relative = path
            .strip_prefix(base)
            .expect("snapshot path should remain below base")
            .to_path_buf();
        if path.is_dir() {
            snapshot.push((relative, None));
            let mut children = fs::read_dir(path)
                .expect("snapshot directory should be readable")
                .map(|entry| entry.expect("snapshot entry should be readable").path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                visit(base, &child, snapshot);
            }
        } else {
            snapshot.push((
                relative,
                Some(fs::read(path).expect("snapshot file should be readable")),
            ));
        }
    }

    let mut snapshot = Vec::new();
    visit(root, root, &mut snapshot);
    snapshot
}

#[test]
#[serial_test::serial]
fn parent_forwarder_uses_the_exact_machine_services_socket_and_authority() {
    let fixture = Fixture::new(MachineProvider::Krunkit, MachineStateRecord::initialized());
    let paths = fixture.roots.paths(DEFAULT_MACHINE_NAME);
    let authority = authority(&fixture.config, 11);

    let config = parent_forwarder_config(&paths, &authority)
        .expect("parent forwarder config should validate");

    assert_eq!(
        config.unix_socket_path(),
        Some(paths.gvproxy_services_socket_path().as_path())
    );
    assert_eq!(config.provider_instance(), authority.provider_instance());
    assert_eq!(config.provider_generation(), authority.generation());
}

#[test]
#[serial_test::serial]
fn provider_managed_wsl_rejects_before_mutation_or_effect() {
    let fixture = Fixture::new(MachineProvider::Wsl2, MachineStateRecord::initialized());
    let before = snapshot_tree(fixture.temp.path());
    let connectivity_calls = AtomicUsize::new(0);

    let error = prepare_default_machine_provision_source_at_with(
        &fixture.roots,
        &fixture.network,
        NodeIdentity::new("wsl-source-node").expect("node fixture should validate"),
        |_| {
            connectivity_calls.fetch_add(1, Ordering::SeqCst);
            Ok(source_connectivity())
        },
    )
    .err()
    .expect("provider-managed networking must fail before source activation");

    assert!(error.to_string().contains("WSL2"), "{error}");
    assert_eq!(
        connectivity_calls.load(Ordering::SeqCst),
        0,
        "provider-managed mode must fail before capability or provider resolution"
    );
    assert_eq!(
        snapshot_tree(fixture.temp.path()),
        before,
        "rejection must not create a lock, directory, journal, lease, socket, or provider artifact"
    );
    assert!(!fixture.roots.lock_path(DEFAULT_MACHINE_NAME).exists());
}

#[test]
#[serial_test::serial]
fn running_source_adopts_exact_current_authority_without_start() {
    let fixture = Fixture::new(MachineProvider::Krunkit, MachineStateRecord::initialized());
    let paths = fixture.roots.paths(DEFAULT_MACHINE_NAME);
    let current = authority(&fixture.config, 7);
    write_json_file(&paths.state_path, &running_state(&paths, current.clone()))
        .expect("running state should persist");
    let prepared = fixture.prepare();
    let starts = AtomicUsize::new(0);

    let activation = prepared
        .activate_machine_with(|_, _, _, _, _| {
            starts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .expect("the exact running source should be adopted");

    assert_eq!(starts.load(Ordering::SeqCst), 0);
    assert_eq!(activation.authority, current);
    assert_eq!(activation.source_plan, prepared.source_plan);
}

#[test]
#[serial_test::serial]
fn forwarded_server_profile_defers_machine_activation_until_after_engine_construction() {
    #[cfg(unix)]
    let temp = TempDir::new_in("/tmp").expect("short fixture root should exist");
    #[cfg(not(unix))]
    let temp = TempDir::new().expect("fixture root should exist");
    let network_root = LocalNodeNetworkRoot::resolve_for_current_platform(Some(
        &temp.path().join("network-authority"),
    ))
    .expect("forwarded server network root should resolve");
    let staged = StagedLocalNetworkComposition::claim(&network_root)
        .expect("forwarded server network should stage");
    let network = HostMachineNetworkAuthority::injected(staged.authority());
    let roots = MachineRootLayout::new(
        temp.path().join("machine/config"),
        temp.path().join("machine/state"),
        temp.path().join("machine/data"),
        temp.path().join("machine/cache"),
        temp.path().join("machine/runtime"),
    );
    let paths = roots.paths(DEFAULT_MACHINE_NAME);
    let config = machine_config(&roots, &network, MachineProvider::Krunkit);
    write_json_file(&paths.config_path, &config).expect("config fixture should persist");
    let current = authority(&config, 7);
    let running = running_state(&paths, current);
    write_json_file(&paths.state_path, &running).expect("running state should persist");
    let source = prepare_default_machine_provision_source_at_with(
        &roots,
        &network,
        NodeIdentity::new("forwarded-server-node").expect("node fixture should validate"),
        |_| Ok(source_connectivity()),
    )
    .expect("forwarded machine source should prepare");
    let profile = prepare_forwarded_server_profile_for_test(staged, source)
        .expect("forwarded server profile should prepare without activation");
    assert_eq!(
        load_machine_provision_source_snapshot(&roots, &network, DEFAULT_MACHINE_NAME,)
            .expect("prepared machine source should remain readable")
            .2,
        running,
        "profile preparation must not activate or mutate the machine source"
    );

    let engine = Arc::new(
        nimbus::Engine::new(temp.path().join("engine"))
            .expect("the canonical Engine must exist before activation"),
    );
    let error = match profile.complete(engine) {
        Ok(_) => panic!("the running fixture intentionally has no guest API socket"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("guest machine API socket"),
        "completion must reach machine activation only after Engine construction: {error}"
    );
}

#[test]
#[serial_test::serial]
fn stopped_source_starts_with_exact_next_generation() {
    let fixture = Fixture::new(MachineProvider::Krunkit, MachineStateRecord::initialized());
    let paths = fixture.roots.paths(DEFAULT_MACHINE_NAME);
    write_json_file(
        &paths.state_path,
        &stopped_state_with_runtime(&paths, authority(&fixture.config, 6)),
    )
    .expect("stopped state should persist");
    let prepared = fixture.prepare();
    let starts = AtomicUsize::new(0);

    let activation = prepared
        .activate_machine_with(|_, paths, _, state, expected| {
            starts.fetch_add(1, Ordering::SeqCst);
            assert_eq!(expected.generation(), NetworkResourceGeneration::new(7));
            persist_running_state(paths, state, expected)
        })
        .expect("the stopped source should start with its exact next generation");

    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        activation.authority.generation(),
        NetworkResourceGeneration::new(7)
    );
    let persisted = load_machine_provision_source_snapshot(
        &fixture.roots,
        &fixture.network,
        DEFAULT_MACHINE_NAME,
    )
    .expect("started state should remain authenticated")
    .2;
    assert_eq!(persisted.lifecycle, MachineLifecycle::Running);
    assert_eq!(
        persisted
            .runtime
            .expect("started runtime should exist")
            .forwarder_authority,
        activation.authority
    );
}

#[test]
#[serial_test::serial]
fn prepared_source_rejects_persisted_generation_change_before_start() {
    for changed_generation in [7, 8] {
        let fixture = Fixture::new(MachineProvider::Krunkit, MachineStateRecord::initialized());
        let paths = fixture.roots.paths(DEFAULT_MACHINE_NAME);
        write_json_file(
            &paths.state_path,
            &stopped_state_with_runtime(&paths, authority(&fixture.config, 6)),
        )
        .expect("initial stopped state should persist");
        let prepared = fixture.prepare();
        let changed =
            stopped_state_with_runtime(&paths, authority(&fixture.config, changed_generation));
        write_json_file(&paths.state_path, &changed).expect("crossed state should persist");
        let changed_bytes = fs::read(&paths.state_path).expect("changed state should read");
        let starts = AtomicUsize::new(0);

        let error = prepared
            .activate_machine_with(|_, _, _, _, _| {
                starts.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .err()
            .expect("a changed generation must fence the prepared start");

        assert!(
            error.to_string().contains("changed after preparation"),
            "{error}"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert_eq!(
            fs::read(&paths.state_path).expect("state should remain readable"),
            changed_bytes,
            "fencing must precede any start mutation"
        );
    }
}

#[test]
#[serial_test::serial]
fn stale_or_crossed_generation_rejects_before_start() {
    let fixture = Fixture::new(MachineProvider::Krunkit, MachineStateRecord::initialized());
    let paths = fixture.roots.paths(DEFAULT_MACHINE_NAME);
    let stopped = stopped_state_with_runtime(&paths, authority(&fixture.config, 6));
    write_json_file(&paths.state_path, &stopped).expect("stopped state should persist");
    let before = snapshot_tree(fixture.temp.path());
    let attempts = [authority(&fixture.config, 6), crossed_authority(7)];

    for attempted in attempts {
        let mut config = fixture.config.clone();
        let mut state = stopped.clone();
        let error = start_machine_with_expected_forwarder_authority(
            &fixture.network,
            &paths,
            &mut config,
            &mut state,
            &attempted,
        )
        .expect_err("stale or crossed authority must fail before start preparation");

        assert!(
            matches!(error, Error::PreconditionFailed(_)),
            "exact authority mismatch must be a precondition failure: {error}"
        );
        assert_eq!(config, fixture.config);
        assert_eq!(state, stopped);
        assert_eq!(
            snapshot_tree(fixture.temp.path()),
            before,
            "authority fencing must precede directories, journals, leases, sockets, and provider effects"
        );
    }
}

#[test]
#[serial_test::serial]
fn concurrent_equal_stopped_activation_invokes_one_start() {
    let fixture = Fixture::new(MachineProvider::Krunkit, MachineStateRecord::initialized());
    let prepared = fixture.prepare();
    let barrier = Arc::new(Barrier::new(3));
    let starts = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::new();

    for prepared in [prepared.clone(), prepared] {
        let barrier = Arc::clone(&barrier);
        let starts = Arc::clone(&starts);
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            prepared
                .activate_machine_with(|_, paths, _, state, expected| {
                    starts.fetch_add(1, Ordering::SeqCst);
                    persist_running_state(paths, state, expected)
                })
                .map(|activation| activation.authority)
                .map_err(|error| error.to_string())
        }));
    }

    barrier.wait();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().expect("activation thread should not panic"))
        .collect::<Vec<_>>();
    let successes = results.iter().filter(|result| result.is_ok()).count();
    let rejections = results.iter().filter(|result| result.is_err()).count();

    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(successes, 1, "results: {results:?}");
    assert_eq!(rejections, 1, "results: {results:?}");
    assert!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .any(|error| error.contains("changed after preparation")),
        "the losing activation must report exact-source fencing: {results:?}"
    );
}

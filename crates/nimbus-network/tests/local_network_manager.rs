use std::net::Ipv4Addr;
use std::num::NonZeroU16;
use std::sync::{Arc, Barrier, Mutex};

use nimbus_core::TenantId;
use nimbus_network::{
    ListenerId, LocalNetworkAuthority, LocalNetworkAuthorityRootMismatch, LocalNetworkManager,
    LocalNetworkManagerBootstrap, LocalNetworkManagerError, LocalNetworkStateStore,
    NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration, NetworkCapabilityBundle,
    NetworkCapabilityRegistry, NetworkCapabilitySelection, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLeaseEpoch, NetworkLifecycleCapabilitySet,
    NetworkManagementMode, NetworkProviderId, NetworkResourceGeneration,
    NetworkSovereigntyCapabilities, NetworkStatePartition, PortBindRealm, PortBindTarget,
    PortBindingSpec, PortExposure, PortLeaseAccounting, PortLeaseFence, PortLeaseId,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

static MANAGER_TEST_SERIALIZER: Mutex<()> = Mutex::new(());

#[cfg(unix)]
struct CurrentDirGuard(Option<std::path::PathBuf>);

#[cfg(unix)]
impl CurrentDirGuard {
    fn enter(path: &std::path::Path) -> Self {
        let original = std::env::current_dir().expect("current directory should be readable");
        std::env::set_current_dir(path).expect("fixture current directory should be selected");
        Self(Some(original))
    }

    fn restore(mut self) {
        let original = self
            .0
            .take()
            .expect("current-directory guard should restore exactly once");
        std::env::set_current_dir(original).expect("original current directory should be restored");
    }
}

#[cfg(unix)]
impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        if let Some(original) = self.0.take() {
            let _ = std::env::set_current_dir(original);
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SiblingState {
    owner: Option<String>,
}

fn empty_registry() -> NetworkCapabilityRegistry {
    NetworkCapabilityRegistry::new([]).expect("an empty fail-closed registry should validate")
}

fn fixture_registry() -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let attachment = NetworkAttachmentProviderRegistration::new(
        NetworkProviderId::for_registration_key("manager-contract.attachment"),
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        [],
        NetworkLifecycleCapabilitySet::new([]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let ingress = NetworkIngressProviderRegistration::new(
        NetworkProviderId::for_registration_key("manager-contract.ingress"),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let selection = NetworkCapabilitySelection::new(
        attachment.provider_id().clone(),
        ingress.provider_id().clone(),
    );
    let registry =
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("the supplied complete registry should validate");
    (registry, selection)
}

fn fixture_request() -> PortLeaseRequest {
    let tenant = TenantId::new("manager-contract").expect("fixture tenant should validate");
    let listener = ListenerId::for_tenant_workload_listener(
        &tenant,
        "manager-incarnation",
        "contract-listener",
    );
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener),
        listener.into(),
        Some(tenant),
        PortLeaseFence::new(NetworkResourceGeneration::new(1), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::HostInternal,
        PortPublicationIntent::Unpublished,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::Exact(
                NonZeroU16::new(41_474).expect("fixture port should be non-zero"),
            ),
        ),
    )
}

#[test]
fn local_network_manager_enforces_one_process_composition() {
    let _serial = MANAGER_TEST_SERIALIZER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = tempdir().expect("manager root should exist");
    let manager = LocalNetworkManager::open(root.path(), empty_registry())
        .expect("first manager should own the process composition");
    assert_eq!(manager.capability_registry().selections().count(), 0);
    assert_eq!(
        manager.authority_path(),
        LocalNetworkStateStore::authority_path_for(manager.state_root())
    );

    let request = fixture_request();
    manager
        .port_leases()
        .reserve(request.clone())
        .expect("manager-derived port authority should reserve");
    manager
        .state_store()
        .transaction(
            &NetworkStatePartition::TenantIpam(
                TenantId::new("manager-sibling").expect("fixture tenant should validate"),
            ),
            |state: &mut SiblingState| {
                state.owner = Some("manager".to_owned());
                Ok::<_, std::convert::Infallible>(())
            },
        )
        .expect("manager-derived state handle should preserve sibling partitions");
    assert!(
        manager
            .port_leases()
            .inspect(request.lease_id())
            .expect("port authority should remain readable")
            .is_some(),
        "a sibling-partition commit must not lose the manager's port lease"
    );

    let duplicate = LocalNetworkManager::open(root.path(), empty_registry())
        .expect_err("a second independent manager must fail");
    assert_duplicate(
        &duplicate,
        manager.authority_path(),
        manager.authority_path(),
    );

    #[cfg(unix)]
    {
        let missing_cwd_parent = tempdir().expect("missing-cwd parent should exist");
        let missing_cwd = missing_cwd_parent.path().join("removed-current-directory");
        std::fs::create_dir(&missing_cwd).expect("fixture current directory should exist");
        let cwd = CurrentDirGuard::enter(&missing_cwd);
        std::fs::remove_dir(&missing_cwd).expect("fixture current directory should be removed");
        let duplicate = LocalNetworkManager::open("relative-root", empty_registry());
        cwd.restore();
        let duplicate = duplicate
            .expect_err("duplicate rejection must not depend on resolving the current directory");
        assert_duplicate(
            &duplicate,
            manager.authority_path(),
            &LocalNetworkStateStore::authority_path_for("relative-root"),
        );
    }

    let lexical_alias = root.path().join(".");
    let duplicate = LocalNetworkManager::open(&lexical_alias, empty_registry())
        .expect_err("a lexical alias must be the same process composition");
    assert_duplicate(
        &duplicate,
        manager.authority_path(),
        manager.authority_path(),
    );

    #[cfg(unix)]
    {
        let alias_parent = tempdir().expect("alias parent should exist");
        let alias = alias_parent.path().join("manager-root-alias");
        std::os::unix::fs::symlink(root.path(), &alias).expect("root alias should be created");
        let duplicate = LocalNetworkManager::open(&alias, empty_registry())
            .expect_err("a symlink alias must be the same process composition");
        assert_duplicate(
            &duplicate,
            manager.authority_path(),
            manager.authority_path(),
        );
    }

    let untouched_parent = tempdir().expect("alternate parent should exist");
    let untouched_root = untouched_parent.path().join("attempted-root");
    let duplicate = LocalNetworkManager::open(&untouched_root, empty_registry())
        .expect_err("a divergent second process composition must fail");
    match &duplicate {
        LocalNetworkManagerError::DuplicateProcessComposition {
            active_authority_path,
            attempted_authority_path,
        } => {
            assert_eq!(active_authority_path, manager.authority_path());
            assert_ne!(attempted_authority_path, active_authority_path);
        }
        other => panic!("expected duplicate composition error, got {other}"),
    }
    assert!(
        !untouched_root.exists(),
        "duplicate rejection must not create or mutate the attempted root"
    );
    assert_actionable(&duplicate);

    let shared = Arc::clone(&manager);
    assert!(Arc::ptr_eq(&manager, &shared));
    drop(manager);
    LocalNetworkManager::open(root.path(), empty_registry())
        .expect_err("a non-final clone must retain the process claim");
    drop(shared);

    let reopened = LocalNetworkManager::open(root.path(), empty_registry())
        .expect("the final drop should release only the process claim");
    assert!(
        reopened
            .port_leases()
            .inspect(request.lease_id())
            .expect("reopened authority should remain readable")
            .is_some(),
        "manager reopen must preserve durable lease state"
    );
    drop(reopened);

    let invalid_root_parent = tempdir().expect("invalid-root parent should exist");
    let invalid_root = invalid_root_parent.path().join("not-a-directory");
    std::fs::write(&invalid_root, b"file").expect("invalid root fixture should be written");
    LocalNetworkManager::open(&invalid_root, empty_registry())
        .expect_err("store initialization through a file must fail");
    let after_failure = LocalNetworkManager::open(root.path(), empty_registry())
        .expect("failed construction must release its provisional process claim");
    drop(after_failure);

    let barrier = Arc::new(Barrier::new(3));
    let contenders = ["alpha", "beta"].map(|_| {
        let barrier = Arc::clone(&barrier);
        let root = root.path().to_path_buf();
        std::thread::spawn(move || {
            barrier.wait();
            LocalNetworkManager::open(root, empty_registry())
        })
    });
    barrier.wait();
    let outcomes = contenders.map(|thread| thread.join().expect("contender should not panic"));
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "concurrent constructors must have exactly one owning manager"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    Err(LocalNetworkManagerError::DuplicateProcessComposition { .. })
                )
            })
            .count(),
        1,
        "the concurrent loser must receive the typed duplicate error"
    );
}

#[test]
fn manager_bootstrap_freezes_once_and_authority_retains_process_claim() {
    let _serial = MANAGER_TEST_SERIALIZER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = tempdir().expect("manager root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(root.path())
        .expect("the first bootstrap should claim the process composition");
    let bootstrap_authority = bootstrap.authority();
    assert_authority_root(&bootstrap_authority, root.path());

    let lexical_alias = root.path().join(".");
    bootstrap_authority
        .authenticate_state_root(&lexical_alias)
        .expect("a lexical alias should authenticate as the selected root");

    #[cfg(unix)]
    let symlink_alias = {
        let alias_parent = tempdir().expect("alias parent should exist");
        let alias = alias_parent.path().join("manager-root-alias");
        std::os::unix::fs::symlink(root.path(), &alias).expect("root alias should be created");
        bootstrap_authority
            .authenticate_state_root(&alias)
            .expect("an existing symlink alias should authenticate as the selected root");
        (alias_parent, alias)
    };

    let divergent_parent = tempdir().expect("divergent parent should exist");
    let divergent_root = divergent_parent.path().join("attempted-root");
    let mismatch = bootstrap_authority
        .authenticate_state_root(&divergent_root)
        .expect_err("a divergent root must fail authority authentication");
    assert_authority_mismatch(
        &mismatch,
        bootstrap_authority.authority_path(),
        &LocalNetworkStateStore::authority_path_for(&divergent_root),
    );
    assert!(
        !divergent_root.exists(),
        "root authentication must not create or mutate a divergent root"
    );

    let duplicate = expect_bootstrap_error(
        LocalNetworkManager::bootstrap(root.path()),
        "a duplicate bootstrap at the same root must fail",
    );
    assert_duplicate(
        &duplicate,
        bootstrap_authority.authority_path(),
        bootstrap_authority.authority_path(),
    );
    let duplicate = expect_bootstrap_error(
        LocalNetworkManager::bootstrap(&lexical_alias),
        "a lexical alias must remain the same process composition",
    );
    assert_duplicate(
        &duplicate,
        bootstrap_authority.authority_path(),
        bootstrap_authority.authority_path(),
    );
    #[cfg(unix)]
    {
        let duplicate = expect_bootstrap_error(
            LocalNetworkManager::bootstrap(&symlink_alias.1),
            "a symlink alias must remain the same process composition",
        );
        assert_duplicate(
            &duplicate,
            bootstrap_authority.authority_path(),
            bootstrap_authority.authority_path(),
        );
    }
    let duplicate = expect_bootstrap_error(
        LocalNetworkManager::bootstrap(&divergent_root),
        "a divergent second process composition must fail",
    );
    match &duplicate {
        LocalNetworkManagerError::DuplicateProcessComposition {
            active_authority_path,
            attempted_authority_path,
        } => {
            assert_eq!(active_authority_path, bootstrap_authority.authority_path());
            assert_ne!(attempted_authority_path, active_authority_path);
        }
        other => panic!("expected duplicate composition error, got {other}"),
    }
    assert!(
        !divergent_root.exists(),
        "duplicate rejection must precede attempted-root mutation"
    );
    assert_actionable(&duplicate);

    let request = fixture_request();
    bootstrap_authority
        .port_leases()
        .reserve(request.clone())
        .expect("the paired authority should reserve durable port state");
    let sibling_partition = NetworkStatePartition::TenantIpam(
        TenantId::new("bootstrap-sibling").expect("fixture tenant should validate"),
    );
    bootstrap_authority
        .state_store()
        .transaction(&sibling_partition, |state: &mut SiblingState| {
            state.owner = Some("bootstrap".to_owned());
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("the paired authority should expose the same durable store");
    assert!(
        bootstrap_authority
            .port_leases()
            .inspect(request.lease_id())
            .expect("the paired port authority should remain readable")
            .is_some(),
        "a sibling-partition write must not lose the paired port lease"
    );

    let (registry, expected_selection) = fixture_registry();
    let manager = bootstrap.freeze(registry);
    assert_eq!(
        manager
            .capability_registry()
            .selections()
            .cloned()
            .collect::<Vec<_>>(),
        vec![expected_selection],
        "freeze must preserve the exact immutable registry supplied by composition"
    );
    let manager_authority = manager.authority();
    assert_authority_root(&manager_authority, root.path());
    assert_eq!(
        bootstrap_authority.authority_path(),
        manager_authority.authority_path(),
        "bootstrap and frozen-manager handles must remain paired to one authority"
    );

    drop(bootstrap_authority);
    drop(manager);
    expect_bootstrap_error(
        LocalNetworkManager::bootstrap(root.path()),
        "a manager-derived authority clone must retain the process claim",
    );
    drop(manager_authority);

    let reopened = LocalNetworkManager::bootstrap(root.path())
        .expect("the final authority drop should permit a deterministic reopen");
    let reopened_authority = reopened.authority();
    assert!(
        reopened_authority
            .port_leases()
            .inspect(request.lease_id())
            .expect("reopened durable authority should remain readable")
            .is_some(),
        "releasing the process claim must not release durable lease state"
    );
    drop(reopened_authority);
    drop(reopened);

    let failed_without_escape = {
        let provisional = LocalNetworkManager::bootstrap(root.path())
            .expect("registry assembly should begin under a provisional claim");
        let result: Result<NetworkCapabilityRegistry, &'static str> =
            Err("simulated source-owned registry assembly failure");
        drop(provisional);
        result
    };
    assert!(
        failed_without_escape.is_err(),
        "the registry assembly fixture must take its failure path"
    );
    let after_unescaped_failure = LocalNetworkManager::bootstrap(root.path())
        .expect("failed assembly without an escaped authority must release the claim");
    drop(after_unescaped_failure);

    let (failed_with_escape, escaped_authority) = {
        let provisional = LocalNetworkManager::bootstrap(root.path())
            .expect("registry assembly should begin under a provisional claim");
        let escaped_authority = provisional.authority();
        let result: Result<NetworkCapabilityRegistry, &'static str> =
            Err("simulated source-owned registry assembly failure");
        drop(provisional);
        (result, escaped_authority)
    };
    assert!(
        failed_with_escape.is_err(),
        "the retained-authority fixture must take its failure path"
    );
    expect_bootstrap_error(
        LocalNetworkManager::bootstrap(root.path()),
        "an escaped authority from failed assembly must retain the process claim",
    );
    drop(escaped_authority);
    let after_escaped_failure = LocalNetworkManager::bootstrap(root.path())
        .expect("the final escaped-authority drop should release the process claim");
    drop(after_escaped_failure);

    let barrier = Arc::new(Barrier::new(3));
    let contenders = ["alpha", "beta"].map(|_| {
        let barrier = Arc::clone(&barrier);
        let root = root.path().to_path_buf();
        std::thread::spawn(move || {
            barrier.wait();
            LocalNetworkManager::bootstrap(root)
        })
    });
    barrier.wait();
    let outcomes = contenders.map(|thread| thread.join().expect("contender should not panic"));
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "concurrent bootstraps must have exactly one winner"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    Err(LocalNetworkManagerError::DuplicateProcessComposition { .. })
                )
            })
            .count(),
        1,
        "the concurrent loser must receive the typed duplicate error"
    );
}

fn assert_authority_root(authority: &LocalNetworkAuthority, expected_root: &std::path::Path) {
    authority
        .authenticate_state_root(expected_root)
        .expect("the selected root should authenticate");
    assert_eq!(
        authority.state_root(),
        std::fs::canonicalize(expected_root).expect("selected root should canonicalize")
    );
    assert_eq!(
        authority.authority_path(),
        LocalNetworkStateStore::authority_path_for(authority.state_root())
    );
    assert_eq!(
        authority.state_store().authority_path(),
        authority.authority_path(),
        "the derived store must share the paired authority path"
    );
}

fn expect_bootstrap_error(
    result: Result<LocalNetworkManagerBootstrap, LocalNetworkManagerError>,
    context: &str,
) -> LocalNetworkManagerError {
    match result {
        Ok(_) => panic!("{context}"),
        Err(error) => error,
    }
}

fn assert_authority_mismatch(
    mismatch: &LocalNetworkAuthorityRootMismatch,
    expected_active: &std::path::Path,
    expected_attempted: &std::path::Path,
) {
    assert_eq!(mismatch.active_authority_path(), expected_active);
    assert_eq!(mismatch.attempted_authority_path(), expected_attempted);
    let rendered = mismatch.to_string();
    assert!(
        rendered.contains("inject")
            && rendered.contains(&expected_active.display().to_string())
            && rendered.contains(&expected_attempted.display().to_string()),
        "root mismatch diagnostics must identify both authorities and the sharing action: {rendered}"
    );
}

fn assert_duplicate(
    error: &LocalNetworkManagerError,
    expected_active: &std::path::Path,
    expected_attempted: &std::path::Path,
) {
    match error {
        LocalNetworkManagerError::DuplicateProcessComposition {
            active_authority_path,
            attempted_authority_path,
        } => {
            assert_eq!(active_authority_path, expected_active);
            assert_eq!(attempted_authority_path, expected_attempted);
        }
        other => panic!("expected duplicate composition error, got {other}"),
    }
    assert_actionable(error);
}

fn assert_actionable(error: &LocalNetworkManagerError) {
    let rendered = error.to_string();
    assert!(
        rendered.contains("clone") && rendered.contains("inject"),
        "duplicate diagnostics must tell the caller how to share ownership: {rendered}"
    );
}

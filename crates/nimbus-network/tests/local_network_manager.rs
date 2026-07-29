use std::net::Ipv4Addr;
use std::num::NonZeroU16;
use std::sync::{Arc, Barrier};

use nimbus_core::TenantId;
use nimbus_network::{
    ListenerId, LocalNetworkManager, LocalNetworkManagerError, LocalNetworkStateStore,
    NetworkCapabilityRegistry, NetworkLeaseEpoch, NetworkResourceGeneration, NetworkStatePartition,
    PortBindRealm, PortBindTarget, PortBindingSpec, PortExposure, PortLeaseAccounting,
    PortLeaseFence, PortLeaseId, PortLeaseRequest, PortProtocol, PortPublicationIntent,
    PortRequestMode,
};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

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

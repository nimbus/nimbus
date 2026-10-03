use std::fs;
use std::sync::{Arc, Barrier};

use nimbus_core::{Cidr, TenantId};
use nimbus_network::{LocalNetworkManager, LocalNetworkStateStore};
use tempfile::TempDir;

use super::{OciNetworkProcess, OciNetworkProcessError};
use crate::backends::oci::network::OciNetworkLayout;
use crate::instance::SandboxId;

fn fixture_process() -> (TempDir, Arc<OciNetworkProcess>) {
    let root = TempDir::new().expect("network process root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(root.path())
        .expect("manager bootstrap should claim the process authority");
    let process = OciNetworkProcess::new(
        bootstrap.authority(),
        Cidr::parse("10.80.0.0/16").expect("fixture super-net should validate"),
        24,
    )
    .expect("the first OCI composition should own process lifetimes");
    drop(bootstrap);
    (root, process)
}

#[test]
fn oci_network_process_contract_has_exactly_one_concurrent_winner() {
    let _serial = OciNetworkProcess::lock_test_process_claim();
    let root = TempDir::new().expect("network process root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(root.path())
        .expect("manager bootstrap should claim the process authority");
    let authority = bootstrap.authority();
    let topology = Cidr::parse("10.80.0.0/16").expect("fixture super-net should validate");

    let authority_before_invalid = fs::read(authority.authority_path()).ok();
    assert!(matches!(
        OciNetworkProcess::new(authority.clone(), topology, 15),
        Err(OciNetworkProcessError::InvalidTenantPrefix {
            node_supernet,
            attempted: 15,
        }) if node_supernet == topology
    ));
    assert_eq!(
        fs::read(authority.authority_path()).ok(),
        authority_before_invalid,
        "invalid topology must not mutate durable network authority"
    );

    let process =
        OciNetworkProcess::new(authority.clone(), topology, 24).expect("first process should open");
    assert_eq!(
        process.attachment_authority().authority_path(),
        authority.attachments().authority_path(),
        "OCI backends must receive the manager-derived attachment authority from the one process \
         composition"
    );

    #[cfg(unix)]
    {
        let alias_parent = TempDir::new().expect("alias parent should exist");
        let alias_root = alias_parent.path().join("network-root-alias");
        std::os::unix::fs::symlink(root.path(), &alias_root)
            .expect("canonical network-root alias should create");
        let active_before_alias = fs::read(authority.authority_path()).ok();
        process
            .authenticate_backend_config(&alias_root, "10.80.0.0/16", 24)
            .expect("the process must accept a configured canonical authority alias");
        assert_eq!(
            fs::read(authority.authority_path()).ok(),
            active_before_alias,
            "alias authentication must not mutate durable authority"
        );
    }

    let divergent_parent = TempDir::new().expect("divergent parent should exist");
    let divergent_root = divergent_parent.path().join("uncreated-network-root");
    let active_before_divergence = fs::read(authority.authority_path()).ok();
    let divergence = process
        .authenticate_backend_config(&divergent_root, "10.80.0.0/16", 24)
        .expect_err("a divergent configured authority must fail before creation");
    match divergence {
        OciNetworkProcessError::AuthorityRootMismatch(error) => {
            assert_eq!(error.active_authority_path(), authority.authority_path());
            assert_eq!(
                error.attempted_authority_path(),
                LocalNetworkStateStore::authority_path_for(&divergent_root)
            );
        }
        other => panic!("expected typed divergent-root evidence, got {other}"),
    }
    assert_eq!(
        fs::read(authority.authority_path()).ok(),
        active_before_divergence,
        "divergent-root rejection must not mutate active durable authority"
    );
    assert!(
        !divergent_root.exists(),
        "divergent-root rejection must not create attempted authority"
    );

    let duplicate = OciNetworkProcess::new(authority.clone(), topology, 24)
        .expect_err("a second process composition must fail");
    assert!(matches!(
        duplicate,
        OciNetworkProcessError::DuplicateProcessComposition { .. }
    ));

    let mismatch = OciNetworkProcess::new(
        authority.clone(),
        Cidr::parse("10.81.0.0/16").expect("alternate super-net should validate"),
        25,
    )
    .expect_err("a topology-substituted process composition must fail");
    match mismatch {
        OciNetworkProcessError::DuplicateProcessComposition {
            active_supernet,
            attempted_supernet,
            active_tenant_prefix,
            attempted_tenant_prefix,
            ..
        } => {
            assert_eq!(active_supernet, topology);
            assert_ne!(attempted_supernet, active_supernet);
            assert_eq!(active_tenant_prefix, 24);
            assert_eq!(attempted_tenant_prefix, 25);
        }
        other => panic!("expected typed duplicate composition evidence, got {other}"),
    }

    drop(process);

    let barrier = Arc::new(Barrier::new(3));
    let contenders = ["container", "krun"].map(|_| {
        let authority = authority.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            OciNetworkProcess::new(authority, topology, 24)
        })
    });
    barrier.wait();
    let outcomes = contenders.map(|thread| thread.join().expect("contender should not panic"));
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "concurrent process composition must have one winner"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    Err(OciNetworkProcessError::DuplicateProcessComposition { .. })
                )
            })
            .count(),
        1,
        "the concurrent loser must receive typed duplicate evidence"
    );
    drop(outcomes);

    let reopened = OciNetworkProcess::new(authority, topology, 24)
        .expect("final process drop should permit deterministic reopen");
    drop(reopened);
}

#[test]
fn oci_network_process_retains_ipam_authority_and_rejects_substituted_layout_without_mutation() {
    let _serial = OciNetworkProcess::lock_test_process_claim();
    let (root, process) = fixture_process();
    let foreign = TempDir::new().expect("foreign network root should exist");
    let tenant = TenantId::new("process-ipam").expect("fixture tenant should validate");
    let sandbox = SandboxId::new("process-ipam");
    let active_layout = OciNetworkLayout::with_roots(
        root.path().join("workloads"),
        root.path(),
        &tenant,
        &sandbox,
    );
    let substituted_layout = OciNetworkLayout::with_roots(
        root.path().join("workloads"),
        foreign.path(),
        &tenant,
        &sandbox,
    );
    let active_authority_path = process.authority().authority_path().to_path_buf();
    let foreign_authority_path = LocalNetworkStateStore::authority_path_for(foreign.path());
    let active_before = fs::read(&active_authority_path).ok();
    assert!(
        !foreign_authority_path.exists(),
        "the substituted root must begin without durable network authority"
    );

    let ipam = process.ipam_authority();
    assert_eq!(
        ipam.state_root(),
        process.authority().state_root(),
        "the process must retain an IPAM adapter derived from its active authority"
    );
    ipam.authenticate_layout(&active_layout)
        .expect("the retained IPAM authority should authenticate its active root");
    let error = ipam
        .authenticate_layout(&substituted_layout)
        .expect_err("a substituted layout root must fail before state access");
    assert!(
        error
            .to_string()
            .contains("rejected network layout authority")
            && error
                .to_string()
                .contains(&foreign_authority_path.display().to_string()),
        "the refusal must preserve substituted-root evidence: {error}"
    );
    assert_eq!(
        fs::read(&active_authority_path).ok(),
        active_before,
        "layout authentication must not mutate the active durable authority"
    );
    assert!(
        !foreign_authority_path.exists(),
        "rejected layout authentication must not create foreign authority"
    );
}

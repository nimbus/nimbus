use std::io::Write as _;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use super::*;
use nimbus_network::{
    NetworkLeaseEpoch, NetworkPlanContentDigest, PortBindingSpec, PortLeaseFence,
};

const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PROCESS_ROOT_ENV: &str = "NIMBUS_NNC64_PROVIDER_JOURNAL_ROOT";
const PROCESS_ROLE_ENV: &str = "NIMBUS_NNC64_PROVIDER_JOURNAL_ROLE";
const PROCESS_CHILD_TEST: &str = "provision::tests::provider_claim_child";

fn provision_plan_with_binding(
    binding_spec: PortBindingSpec,
) -> Result<SandboxProvisionNetworkPlan, SandboxProvisionNetworkPlanError> {
    let tenant = TenantId::new("validation-tenant").expect("tenant should parse");
    let generation = NetworkResourceGeneration::new(3);
    let plan = NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(&tenant, "validation-workload"),
        generation,
        NetworkPlanContentDigest::sha256("validation-plan"),
        crate::backends::sandbox_network_plan_requirements(crate::SandboxBackendKind::Container)
            .capability_requirements()
            .clone(),
    );
    let listener_id =
        ListenerId::for_tenant_workload_listener(&tenant, "validation-workload", "api");
    let request = PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener_id),
        listener_id.clone().into(),
        Some(tenant.clone()),
        PortLeaseFence::new(generation, NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host("127.0.0.1".parse().expect("address should parse")),
        binding_spec,
    )
    .with_plan_id(plan.plan_id().clone());
    SandboxProvisionNetworkPlan::new(
        plan,
        tenant,
        generation,
        NetworkAttachmentId::for_workload_attachment("validation-workload", "primary"),
        [SandboxProvisionListener::new(
            listener_id,
            crate::SandboxPortBinding::tcp("api", 18_080, 8_080),
            request,
        )],
        [],
    )
}

#[test]
fn crossed_bind_realm_target_and_exposure_fail_at_plan_construction() {
    let port = NonZeroU16::new(18_080).expect("port should be non-zero");
    let cases = [
        (
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Unknown,
                PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
                PortExposure::Loopback,
                PortRequestMode::Exact(port),
            ),
            SandboxProvisionNetworkPlanError::BindRealmMismatch,
        ),
        (
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_wildcard(),
                PortExposure::Loopback,
                PortRequestMode::Exact(port),
            ),
            SandboxProvisionNetworkPlanError::BindTargetMismatch,
        ),
        (
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
                PortExposure::Public,
                PortRequestMode::Exact(port),
            ),
            SandboxProvisionNetworkPlanError::ExposureMismatch,
        ),
    ];
    for (binding, expected) in cases {
        assert_eq!(
            provision_plan_with_binding(binding).expect_err("crossed binding must fail"),
            expected
        );
    }
}

#[test]
fn every_current_application_protocol_maps_to_an_exact_tcp_host_lease() {
    let port = NonZeroU16::new(18_080).expect("port should be non-zero");
    for protocol in [
        nimbus_network::EndpointProtocol::Tcp,
        nimbus_network::EndpointProtocol::Http,
        nimbus_network::EndpointProtocol::Https,
    ] {
        let mut plan = provision_plan_with_binding(PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(std::net::Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::Exact(port),
        ))
        .expect("TCP transport should validate");
        plan.listeners[0].binding.protocol = protocol;
        SandboxProvisionNetworkPlan::new(
            plan.network_plan.clone(),
            plan.tenant_id.clone(),
            plan.generation,
            plan.attachment_id.clone(),
            plan.listeners,
            plan.dependency_listeners,
        )
        .expect("TCP, HTTP, and HTTPS application protocols all use TCP lease authority");
    }
}

fn claim(epoch: u64) -> ProviderProvisionClaim {
    ProviderProvisionClaim::new(ProviderProvisionClaimInput {
        authority_id: "wsg_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        effect_subject: r#"{"kind":"execution","id":"wex_alpha"}"#.to_owned(),
        attempt_id: "wpa_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        dispatch_epoch: epoch,
        generation: 7,
        desired_digest: DIGEST_A.to_owned(),
        source_digest: DIGEST_B.to_owned(),
        network_plan_digest: DIGEST_A.to_owned(),
        provider_target_digest: DIGEST_B.to_owned(),
        operation: ProviderProvisionOperation::ActivateWorkload,
    })
    .expect("fixture claim should be valid")
}

fn publish_claim(epoch: u64) -> ProviderProvisionClaim {
    let mut claim = claim(epoch);
    claim.operation = ProviderProvisionOperation::PublishIngress;
    claim
}

fn journal(root: &Path) -> ProviderProvisionAttemptJournal {
    ProviderProvisionAttemptJournal::open(root, "container-runtime")
        .expect("fixture journal should open")
}

#[test]
fn exact_replay_adopts_without_second_execute_authority() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = claim(0);

    assert!(matches!(
        journal
            .claim_dispatch_epoch(&claim)
            .expect("first claim should persist"),
        ProviderProvisionClaimDecision::ExecuteClaimed(_)
    ));
    let replay = journal
        .claim_dispatch_epoch(&claim)
        .expect("exact replay should inspect");
    assert!(matches!(
        replay,
        ProviderProvisionClaimDecision::AdoptExactAttempt(ProviderProvisionObservation {
            kind: ProviderProvisionObservationKind::Claimed,
            ..
        })
    ));
}

#[test]
fn exact_absence_is_the_only_authority_for_next_epoch() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let epoch_zero = claim(0);
    journal
        .claim_dispatch_epoch(&epoch_zero)
        .expect("first claim should persist");

    let without_absence = journal
        .claim_dispatch_epoch(&claim(1))
        .expect_err("retry without absence must fail");
    assert_eq!(
        without_absence,
        ProviderProvisionJournalError::RetryWithoutAbsence
    );

    journal
        .record_observation(
            &epoch_zero,
            ProviderProvisionObservationKind::Absent,
            b"runtime and manifest absent",
        )
        .expect("exact absence should persist");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&claim(1))
            .expect("exact next epoch should claim"),
        ProviderProvisionClaimDecision::ExecuteClaimed(_)
    ));
}

#[test]
fn process_bound_publish_success_reconciles_to_exact_absence_before_retry() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = publish_claim(0);
    journal
        .claim_dispatch_epoch(&first)
        .expect("publish claim should persist");
    journal
        .record_observation(
            &first,
            ProviderProvisionObservationKind::Succeeded,
            b"listener was active before process death",
        )
        .expect("publish success should persist");

    let reconciled = journal
        .record_reconciled_absence(
            &first,
            b"dead process lifetime proves the listener is absent",
        )
        .expect("provider-proven process absence should supersede success");
    assert_eq!(reconciled.kind(), ProviderProvisionObservationKind::Absent);
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&publish_claim(1))
            .expect("exact next publish epoch should receive authority"),
        ProviderProvisionClaimDecision::ExecuteClaimed(_)
    ));
}

#[test]
fn reconciled_absence_rejects_non_publish_and_definite_failure() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let activation = claim(0);
    journal
        .claim_dispatch_epoch(&activation)
        .expect("activation claim should persist");
    assert!(matches!(
        journal.record_reconciled_absence(&activation, b"invalid"),
        Err(ProviderProvisionJournalError::InvalidClaim { .. })
    ));

    let publish = publish_claim(0);
    let publish_journal =
        ProviderProvisionAttemptJournal::open(root.path().join("publish"), "container-runtime")
            .expect("publish journal should open");
    publish_journal
        .claim_dispatch_epoch(&publish)
        .expect("publish claim should persist");
    publish_journal
        .record_observation(
            &publish,
            ProviderProvisionObservationKind::DefiniteFailure,
            b"provider rejected the exact request",
        )
        .expect("definite failure should persist");
    assert_eq!(
        publish_journal
            .record_reconciled_absence(&publish, b"invalid overwrite")
            .expect_err("definite failure must remain terminal"),
        ProviderProvisionJournalError::CrossedClaim
    );
}

#[test]
fn stale_skipped_and_crossed_claims_fail_before_mutation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = claim(2);
    journal
        .claim_dispatch_epoch(&first)
        .expect("first claim should persist");
    journal
        .record_observation(&first, ProviderProvisionObservationKind::Absent, b"absent")
        .expect("absence should persist");

    assert_eq!(
        journal
            .claim_dispatch_epoch(&claim(1))
            .expect_err("stale epoch must fail"),
        ProviderProvisionJournalError::StaleDispatchEpoch {
            current: 2,
            candidate: 1,
        }
    );
    assert_eq!(
        journal
            .claim_dispatch_epoch(&claim(4))
            .expect_err("skipped epoch must fail"),
        ProviderProvisionJournalError::SkippedDispatchEpoch {
            current: 2,
            candidate: 4,
        }
    );
    let mut crossed = claim(2);
    crossed.effect_subject = "crossed-subject".to_owned();
    assert_eq!(
        journal
            .claim_dispatch_epoch(&crossed)
            .expect_err("crossed claim must fail"),
        ProviderProvisionJournalError::CrossedClaim
    );

    assert_eq!(
        journal
            .adopt_exact_attempt(&first)
            .expect("original observation should remain")
            .expect("original observation should exist")
            .kind(),
        ProviderProvisionObservationKind::Absent
    );
}

#[test]
fn concurrent_equal_claims_grant_one_execute_authority() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = Arc::new(journal(root.path()));
    let barrier = Arc::new(Barrier::new(16));
    let handles: Vec<_> = (0..16)
        .map(|_| {
            let journal = Arc::clone(&journal);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                journal
                    .claim_dispatch_epoch(&claim(0))
                    .expect("contending claim should resolve")
            })
        })
        .collect();

    let decisions: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("claim thread should finish"))
        .collect();
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| matches!(
                decision,
                ProviderProvisionClaimDecision::ExecuteClaimed(_)
            ))
            .count(),
        1,
        "only one contender may receive effect authority"
    );
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| matches!(
                decision,
                ProviderProvisionClaimDecision::AdoptExactAttempt(_)
            ))
            .count(),
        15,
        "every losing contender must adopt the durable claim"
    );
}

#[test]
#[ignore = "spawned only by the NNC6.4 provider-journal process parent"]
fn provider_claim_child() {
    let root = PathBuf::from(
        std::env::var_os(PROCESS_ROOT_ENV).expect("child process root must be supplied"),
    );
    let role = std::env::var(PROCESS_ROLE_ENV).expect("child process role must be supplied");
    let ready = root.join(format!("ready-{role}"));
    File::create(&ready)
        .and_then(|file| file.sync_all())
        .expect("child readiness marker should become durable");
    let gate = root.join("go");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.exists() {
        assert!(
            Instant::now() < deadline,
            "child timed out waiting for process contention gate"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    match journal(&root)
        .claim_dispatch_epoch(&claim(0))
        .expect("child claim should resolve")
    {
        ProviderProvisionClaimDecision::ExecuteClaimed(_) => {
            let effect_path = root.join("external-effect");
            let mut effect = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&effect_path)
                .expect("only the execute winner may create the external-effect marker");
            effect
                .write_all(role.as_bytes())
                .and_then(|()| effect.sync_all())
                .expect("external-effect marker should become durable");
            println!("NNC64_PROVIDER_DECISION:execute");
        }
        ProviderProvisionClaimDecision::AdoptExactAttempt(_) => {
            println!("NNC64_PROVIDER_DECISION:adopt");
        }
    }
}

#[test]
fn concurrent_processes_produce_one_external_effect() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let mut children: Vec<_> = (0..8)
        .map(|role| spawn_provider_child(root.path(), role))
        .collect();
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let ready = (0..8)
            .filter(|role| root.path().join(format!("ready-{role}")).is_file())
            .count();
        if ready == 8 {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "only {ready}/8 provider children reached the contention gate"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    File::create(root.path().join("go"))
        .and_then(|file| file.sync_all())
        .expect("parent gate should become durable");

    let outputs: Vec<_> = children.iter_mut().map(wait_for_provider_child).collect();
    let execute_count = outputs
        .iter()
        .filter(|output| output.contains("NNC64_PROVIDER_DECISION:execute"))
        .count();
    let adopt_count = outputs
        .iter()
        .filter(|output| output.contains("NNC64_PROVIDER_DECISION:adopt"))
        .count();
    assert_eq!(execute_count, 1, "one process must own the effect");
    assert_eq!(adopt_count, 7, "every losing process must adopt");
    assert!(
        root.path().join("external-effect").is_file(),
        "the sole execute owner must leave the external-effect witness"
    );
}

#[test]
fn authenticated_record_rejects_tampering() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let claim = claim(0);
    journal
        .claim_dispatch_epoch(&claim)
        .expect("claim should persist");
    let paths = journal.paths(&claim);
    let bytes = fs::read(&paths.record).expect("record should be readable");
    let tampered = String::from_utf8(bytes)
        .expect("record should be UTF-8")
        .replace("\"generation\": 7", "\"generation\": 8");
    fs::write(&paths.record, tampered).expect("test should tamper record");

    let error = journal
        .adopt_exact_attempt(&claim)
        .expect_err("tampering must fail closed");
    assert!(matches!(
        error,
        ProviderProvisionJournalError::Corrupt { .. }
    ));
}

#[test]
fn higher_generation_requires_resolved_prior_effect_and_fences_stale_generation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let journal = journal(root.path());
    let first = claim(0);
    journal
        .claim_dispatch_epoch(&first)
        .expect("first claim should persist");

    let mut next = claim(0);
    next.generation = 8;
    next.attempt_id =
        "wpa_cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned();
    next.effect_subject = r#"{"kind":"execution","id":"wex_beta"}"#.to_owned();
    assert_eq!(
        journal
            .claim_dispatch_epoch(&next)
            .expect_err("unresolved prior effect must fence replacement"),
        ProviderProvisionJournalError::PriorEffectUnresolved
    );

    journal
        .record_observation(
            &first,
            ProviderProvisionObservationKind::Absent,
            b"provider and manifest absent",
        )
        .expect("absence should persist");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&next)
            .expect("resolved prior generation permits successor"),
        ProviderProvisionClaimDecision::ExecuteClaimed(_)
    ));
    assert_eq!(
        journal
            .claim_dispatch_epoch(&first)
            .expect_err("old generation must stay fenced"),
        ProviderProvisionJournalError::StaleGeneration {
            current: 8,
            candidate: 7,
        }
    );
}

fn spawn_provider_child(root: &Path, role: usize) -> Child {
    Command::new(std::env::current_exe().expect("sandbox test executable should resolve"))
        .args(["--exact", PROCESS_CHILD_TEST, "--ignored", "--nocapture"])
        .env(PROCESS_ROOT_ENV, root)
        .env(PROCESS_ROLE_ENV, role.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("provider journal child should start")
}

fn wait_for_provider_child(child: &mut Child) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("provider journal child exceeded 15 seconds");
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("failed to inspect provider journal child: {error}");
            }
        }
    };
    let mut stdout = String::new();
    std::io::Read::read_to_string(
        child.stdout.as_mut().expect("child stdout should be piped"),
        &mut stdout,
    )
    .expect("child stdout should be readable");
    let mut stderr = String::new();
    std::io::Read::read_to_string(
        child.stderr.as_mut().expect("child stderr should be piped"),
        &mut stderr,
    )
    .expect("child stderr should be readable");
    assert!(status.success(), "provider child failed: {stderr}");
    stdout
}

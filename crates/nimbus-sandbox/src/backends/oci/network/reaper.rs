//! Tenant-bridge reaper + legacy shared-bridge migration.
//!
//! netavark creates the per-tenant bridge on first-sandbox setup but does NOT
//! remove it on last-sandbox teardown, so the crash-safe reaper removes the
//! bridge when the allocator reports the tenant drained (the last sandbox hold
//! released). The one-shot legacy purge removes the pre-MTN shared `nimbus0`
//! bridge before the first per-tenant setup, since the routed per-tenant model
//! deletes the shared bridge (pre-launch, breaking — no compat path).

use std::collections::BTreeSet;
use std::path::Path;

use nimbus_core::TenantId;

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::segment::ReleaseOutcome;
use super::{NetworkSegmentAllocator, OciSegmentRealization, SingleNodeSegmentAllocator};

/// Remove a tenant block-bridge interface by name once its last sandbox has
/// drained (netavark won't auto-GC it). Idempotent / best-effort: a bridge that
/// is already gone is success.
pub(crate) fn reap_bridge_interface(interface: &str) -> Result<()> {
    delete_bridge(interface)
}

/// Drop one sandbox's allocator hold and reap every bridge returned when the
/// tenant drains. Both OCI-family backends use this exact ordering.
pub(crate) fn release_network_segment_hold(
    allocator: &dyn NetworkSegmentAllocator,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
) -> Vec<SandboxError> {
    release_network_segment_hold_with(allocator, tenant_id, sandbox_id, |segment| {
        reap_bridge_interface(segment.network_interface())
    })
}

fn release_network_segment_hold_with(
    allocator: &dyn NetworkSegmentAllocator,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    mut reap: impl FnMut(&OciSegmentRealization) -> Result<()>,
) -> Vec<SandboxError> {
    let segments = match allocator.release(tenant_id, sandbox_id) {
        Ok(ReleaseOutcome::TenantDrained { segments }) => segments,
        Ok(ReleaseOutcome::StillLive) => return Vec::new(),
        Err(error) => return vec![error],
    };
    segments
        .iter()
        .filter_map(|segment| reap(segment).err())
        .collect()
}

/// Startup orphan GC: reclaim segment holds whose sandbox netns no longer exists,
/// and reap the tenant bridges that drain as a result. The live-hold set is read
/// directly from the persistent-netns tree
/// (`<state_root>/tenants/<tenant>/networks/netns/<sandbox>`) — a live sandbox has
/// a netns; a cleanly-torn-down one does not — so no manifest parsing is needed
/// and a crash that leaked a hold (netns gone, allocator entry stranded) is
/// reclaimed while a still-live sandbox is conservatively kept. Best-effort +
/// idempotent. Returns the number of tenant bridges reclaimed (the reclaimed
/// metric).
pub(crate) fn reconcile_network_segment_orphans(
    state_root: &Path,
    allocator: &SingleNodeSegmentAllocator,
) -> Result<usize> {
    let live = live_netns_holds(state_root);
    let drained = allocator.reconcile_orphans(&live)?;
    for segment in &drained {
        reap_bridge_interface(segment.network_interface())?;
    }
    Ok(drained.len())
}

/// Enumerate the `(tenant_id, sandbox_id)` pairs that currently hold a persistent
/// netns. A missing tree (fresh node) yields the empty set.
fn live_netns_holds(state_root: &Path) -> BTreeSet<(String, String)> {
    let mut holds = BTreeSet::new();
    let tenants_root = state_root.join("tenants");
    let Ok(tenants) = std::fs::read_dir(&tenants_root) else {
        return holds;
    };
    for tenant in tenants.flatten() {
        let tenant_id = tenant.file_name().to_string_lossy().into_owned();
        let netns_dir = tenant.path().join("networks").join("netns");
        let Ok(sandboxes) = std::fs::read_dir(&netns_dir) else {
            continue;
        };
        for sandbox in sandboxes.flatten() {
            let sandbox_id = sandbox.file_name().to_string_lossy().into_owned();
            holds.insert((tenant_id.clone(), sandbox_id));
        }
    }
    holds
}

/// One-shot migration: remove the legacy shared `nimbus0` bridge from the pre-MTN
/// single-bridge scheme, guarded by a marker under `<networks_root>` so it runs
/// at most once per node. Best-effort / idempotent.
pub(crate) fn purge_legacy_nimbus0_once(networks_root: &Path) -> Result<()> {
    let marker = networks_root.join(".legacy-nimbus0-purged");
    if marker.exists() {
        return Ok(());
    }
    delete_bridge(super::DEFAULT_NETWORK_INTERFACE)?;
    std::fs::create_dir_all(networks_root).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create networks root {} for the legacy-purge marker: {error}",
            networks_root.display()
        ),
    })?;
    std::fs::write(&marker, b"legacy nimbus0 bridge purged by MTN migration\n").map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to write legacy-purge marker {}: {error}",
                marker.display()
            ),
        }
    })
}

#[cfg(target_os = "linux")]
fn delete_bridge(interface: &str) -> Result<()> {
    use std::process::Command;

    let output = Command::new("ip")
        .args(["link", "del", interface])
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to run `ip link del {interface}`: {error}"),
        })?;
    // A missing interface ("Cannot find device") is success — teardown is
    // idempotent and a crash may have already removed the bridge.
    if output.status.success()
        || String::from_utf8_lossy(&output.stderr).contains("Cannot find device")
    {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: format!(
                "`ip link del {interface}` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        })
    }
}

#[cfg(not(target_os = "linux"))]
fn delete_bridge(_interface: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use nimbus_core::TenantId;
    use tempfile::tempdir;

    use crate::backends::oci::network::NetworkSegmentAllocator;
    use crate::instance::SandboxId;

    fn touch_netns(root: &Path, tenant: &str, sandbox: &str) {
        let dir = root
            .join("tenants")
            .join(tenant)
            .join("networks")
            .join("netns");
        std::fs::create_dir_all(&dir).expect("netns dir");
        std::fs::write(dir.join(sandbox), b"").expect("netns file");
    }

    fn touch_evidence(root: &Path, directory: &str, tenant: &str, sandbox: &str, value: &str) {
        let dir = root
            .join("tenants")
            .join(tenant)
            .join("networks")
            .join(directory);
        std::fs::create_dir_all(&dir).expect("evidence directory");
        std::fs::write(dir.join(format!("{sandbox}.json")), value).expect("evidence file");
    }

    fn touch_manifest(root: &Path, tenant: &str, sandbox: &str) {
        let dir = root.join("tenants").join(tenant).join("sandboxes");
        std::fs::create_dir_all(&dir).expect("manifest directory");
        std::fs::write(dir.join(format!("{sandbox}.json")), "{}").expect("manifest file");
    }

    fn evidence_exists(root: &Path, directory: &str, tenant: &str, sandbox: &str) -> bool {
        root.join("tenants")
            .join(tenant)
            .join("networks")
            .join(directory)
            .join(format!("{sandbox}.json"))
            .exists()
    }

    fn manifest_exists(root: &Path, tenant: &str, sandbox: &str) -> bool {
        root.join("tenants")
            .join(tenant)
            .join("sandboxes")
            .join(format!("{sandbox}.json"))
            .exists()
    }

    fn allocator_has_hold(root: &Path, tenant: &str, sandbox: &str) -> bool {
        SingleNodeSegmentAllocator::single_node_default(root).has_hold(tenant, sandbox)
    }

    #[test]
    fn reconcile_reclaims_holds_whose_netns_is_gone_and_keeps_live_ones() {
        let dir = tempdir().expect("temp dir");
        let root = dir.path();
        let allocator = SingleNodeSegmentAllocator::single_node_default(root);

        // tenant-live (index 0) holds a sandbox that still has a netns.
        allocator
            .acquire(
                &TenantId::new("tenant-live").unwrap(),
                &SandboxId::new("sb-live"),
            )
            .expect("acquire live");
        touch_netns(root, "tenant-live", "sb-live");
        // tenant-dead (index 1) holds a sandbox whose netns is gone (crash-leaked).
        allocator
            .acquire(
                &TenantId::new("tenant-dead").unwrap(),
                &SandboxId::new("sb-dead"),
            )
            .expect("acquire dead");

        let reclaimed = reconcile_network_segment_orphans(root, &allocator).expect("reconcile");
        assert_eq!(reclaimed, 1, "only the netns-less tenant is reclaimed");

        // tenant-dead's index 1 was freed -> reused by the next new tenant.
        let reused = allocator
            .acquire(
                &TenantId::new("tenant-new").unwrap(),
                &SandboxId::new("sb-new"),
            )
            .expect("acquire new");
        assert_eq!(reused.cidr().to_string(), "10.0.1.0/24");
        // tenant-live still holds its original index 0.
        let live = allocator
            .acquire(
                &TenantId::new("tenant-live").unwrap(),
                &SandboxId::new("sb-live"),
            )
            .expect("re-acquire live");
        assert_eq!(live.cidr().to_string(), "10.0.0.0/24");
    }

    #[derive(Clone, Copy)]
    struct OrphanEvidenceCase {
        name: &'static str,
        hold: bool,
        desired: bool,
        netns: bool,
        manifest: bool,
        effect: bool,
        desired_generation: u64,
        effect_generation: u64,
        inspection_unknown: bool,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct OrphanObservation {
        reclaimed_segments: usize,
        hold: bool,
        desired: bool,
        netns: bool,
        manifest: bool,
        effect: bool,
        classifier_result: &'static str,
    }

    fn observe_orphan_case(case: OrphanEvidenceCase) -> OrphanObservation {
        let dir = tempdir().expect("temp dir");
        let root = dir.path();
        let allocator = SingleNodeSegmentAllocator::single_node_default(root);
        let tenant = format!("tenant-{}", case.name);
        let sandbox = format!("sandbox-{}", case.name);
        let tenant_id = TenantId::new(&tenant).expect("tenant id should parse");
        let sandbox_id = SandboxId::new(&sandbox);

        if case.hold {
            allocator
                .acquire(&tenant_id, &sandbox_id)
                .expect("hold should persist");
        }
        if case.desired {
            touch_evidence(
                root,
                "attachments",
                &tenant,
                &sandbox,
                &format!(r#"{{"generation":{}}}"#, case.desired_generation),
            );
        }
        if case.netns {
            touch_netns(root, &tenant, &sandbox);
        }
        if case.manifest {
            touch_manifest(root, &tenant, &sandbox);
        }
        if case.effect {
            touch_evidence(
                root,
                "provider-effects",
                &tenant,
                &sandbox,
                &format!(
                    r#"{{"generation":{},"inspection":"{}"}}"#,
                    case.effect_generation,
                    if case.inspection_unknown {
                        "unknown"
                    } else {
                        "present"
                    }
                ),
            );
        }

        let reclaimed_segments =
            reconcile_network_segment_orphans(root, &allocator).expect("reconcile should run");
        let hold = allocator_has_hold(root, &tenant, &sandbox);
        let desired = evidence_exists(root, "attachments", &tenant, &sandbox);
        let netns = root
            .join("tenants")
            .join(&tenant)
            .join("networks")
            .join("netns")
            .join(&sandbox)
            .exists();
        let manifest = manifest_exists(root, &tenant, &sandbox);
        let effect = evidence_exists(root, "provider-effects", &tenant, &sandbox);
        let classifier_result = if hold {
            "retained-by-netns-filename"
        } else if desired || netns || manifest || effect {
            "unowned-evidence-left-behind"
        } else {
            "fully-removed"
        };

        OrphanObservation {
            reclaimed_segments,
            hold,
            desired,
            netns,
            manifest,
            effect,
            classifier_result,
        }
    }

    #[test]
    // This is the NNC0.7 fail-before executable baseline for the exact
    // `provider effect -> allocator hold` crash window in both OCI-family
    // backends. NNC0.1b already proves exact-boundary process killing and
    // same-root recovery; this test materializes the durable recovery image
    // left by that cut without duplicating the upper-layer subprocess harness
    // (which cannot be a dependency of this low-level crate).
    #[ignore = "NNC0.7 expected red until provider attempts precede effects and reconcile removes or quarantines unowned effects"]
    fn nnc0_7_effect_before_hold_crash_must_not_leave_an_unowned_provider_effect() {
        let observed = observe_orphan_case(OrphanEvidenceCase {
            name: "crash-after-effect-before-hold",
            hold: false,
            desired: false,
            netns: true,
            manifest: true,
            effect: true,
            desired_generation: 0,
            effect_generation: 7,
            inspection_unknown: false,
        });

        assert!(!observed.hold, "the crash cut precedes allocator acquire");
        assert!(
            observed.netns && observed.effect,
            "the exact provider-effect boundary must be present before the safety assertion"
        );
        assert_eq!(
            observed.classifier_result, "fully-removed",
            "NNCF8: recovery must remove or durably quarantine the provider effect and netns \
             when no desired attachment/provider attempt owns them"
        );
    }

    #[test]
    // This is the complete NNC0.7 fail-before evidence matrix for NNCF8. It
    // intentionally uses durable desired/effect/generation/inspection markers
    // that the current filename-only reaper cannot read. NNC5.2a owns the
    // classifier and must turn this green by adopting, removing, or
    // quarantining every row; NNC8.3 owns restart convergence.
    #[ignore = "NNC0.7 expected red until orphan recovery classifies durable intent, provider attempts, generations, and unknown inspection"]
    fn nnc0_7_orphan_recovery_must_classify_the_complete_evidence_matrix() {
        let cases = [
            (
                OrphanEvidenceCase {
                    name: "hold-desired-effect",
                    hold: true,
                    desired: true,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 7,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "adopted",
            ),
            (
                OrphanEvidenceCase {
                    name: "hold-no-desired-effect",
                    hold: true,
                    desired: false,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 0,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "hold-no-netns",
                    hold: true,
                    desired: true,
                    netns: false,
                    manifest: true,
                    effect: false,
                    desired_generation: 7,
                    effect_generation: 0,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "effect-no-hold",
                    hold: false,
                    desired: false,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 0,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "manifest-no-hold",
                    hold: false,
                    desired: false,
                    netns: false,
                    manifest: true,
                    effect: false,
                    desired_generation: 0,
                    effect_generation: 0,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "hold-netns-no-manifest",
                    hold: true,
                    desired: true,
                    netns: true,
                    manifest: false,
                    effect: true,
                    desired_generation: 7,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "adopted",
            ),
            (
                OrphanEvidenceCase {
                    name: "stale-generation",
                    hold: true,
                    desired: true,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 8,
                    effect_generation: 7,
                    inspection_unknown: false,
                },
                "removed-or-quarantined",
            ),
            (
                OrphanEvidenceCase {
                    name: "unknown-inspection",
                    hold: true,
                    desired: true,
                    netns: true,
                    manifest: true,
                    effect: true,
                    desired_generation: 7,
                    effect_generation: 7,
                    inspection_unknown: true,
                },
                "cleanup-pending",
            ),
        ];

        let observed: BTreeMap<&str, (&str, OrphanObservation)> = cases
            .into_iter()
            .map(|(case, expected)| (case.name, (expected, observe_orphan_case(case))))
            .collect();
        assert_eq!(observed.len(), 8, "every required evidence arm must run");
        assert!(
            observed.values().all(|(_, observation)| {
                observation.classifier_result == "retained-by-netns-filename"
                    || observation.classifier_result == "unowned-evidence-left-behind"
            }),
            "precondition: the current reaper must expose its filename/hold-only behavior"
        );

        let mismatches: BTreeMap<&str, (&str, &str)> = observed
            .iter()
            .filter_map(|(name, (expected, observation))| {
                (*expected != observation.classifier_result)
                    .then_some((*name, (*expected, observation.classifier_result)))
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "NNCF8: every restart state must be adopted, removed, quarantined, or held \
             cleanup-pending from durable ownership evidence; current mismatches: {mismatches:#?}; \
             full observations: {observed:#?}"
        );
    }

    #[test]
    // This is the NNC0.3 fail-before executable baseline, not the pass-after
    // fix. It must fail at the final safety assertion while the current
    // allocator frees before provider cleanup. NNC2.5 owns quarantine, will
    // turn this test green, and must remove the ignore marker.
    #[ignore = "NNC0.3 expected red until failed bridge cleanup fences segment reuse"]
    fn failed_bridge_cleanup_must_fence_segment_from_reuse() {
        let dir = tempdir().expect("temp dir");
        let allocator = SingleNodeSegmentAllocator::single_node_default(dir.path());
        let original_tenant = TenantId::new("tenant-original").expect("tenant should parse");
        let original_sandbox = SandboxId::new("sandbox-original");
        let original = allocator
            .acquire(&original_tenant, &original_sandbox)
            .expect("original segment should allocate");

        let mut surviving_bridges = Vec::new();
        let cleanup_errors = release_network_segment_hold_with(
            &allocator,
            &original_tenant,
            &original_sandbox,
            |segment| {
                surviving_bridges.push(segment.network_interface().to_owned());
                Err(SandboxError::OperationFailed {
                    message: "forced bridge provider cleanup failure".to_owned(),
                })
            },
        );
        assert_eq!(cleanup_errors.len(), 1);
        assert!(
            cleanup_errors[0]
                .to_string()
                .contains("forced bridge provider cleanup failure")
        );
        assert_eq!(
            surviving_bridges,
            [original.network_interface().to_owned()],
            "the failed provider cleanup leaves the original bridge effect present"
        );

        let replacement = allocator
            .acquire(
                &TenantId::new("tenant-replacement").expect("tenant should parse"),
                &SandboxId::new("sandbox-replacement"),
            )
            .expect("replacement segment should allocate");
        assert_ne!(
            replacement.cidr(),
            original.cidr(),
            "a segment with a surviving provider effect must remain fenced from reuse"
        );
    }
}

//! Object-placement migration classification.

use nimbus_storage::{ObjectStorePlacementTarget, ObjectStoreProviderKind, PlacementPolicy};
use url::Url;

/// The byte-transfer strategy needed when moving a tenant between nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationLeg {
    /// Blob bytes are durable in a shared external object store, so the
    /// destination node can attach to the same configured target instead of
    /// replicating bytes node-to-node.
    CloudObjectStore,
    /// Tenant blobs live only in a host-local leg; node moves must use the
    /// iroh replicate-then-handoff path.
    LocalReplicateHandoff,
}

/// Classifies the migration leg implied by a placement policy.
///
/// [`MigrationLeg::CloudObjectStore`] is chosen only when the policy
/// GUARANTEES every accepted write is durably present on a shared remote
/// target, so attaching the destination node to that target cannot miss
/// blobs:
///
/// - `Tier` and `CloudPrimary`: the remote leg is the byte authority — a
///   `put` persists to it before (or instead of) the local leg.
/// - `Mirror { require_ack: true }`: a put fails unless the mirror write
///   succeeded, so every accepted blob exists remotely.
///
/// Everything else uses [`MigrationLeg::LocalReplicateHandoff`]:
///
/// - `LocalOnly`: blobs live only in the local pack.
/// - `Mirror { require_ack: false }`: mirroring is best-effort — a put
///   succeeds locally even when the remote write fails, so the remote copy
///   may be incomplete and attaching to it could lose data.
/// - Any mode whose target provider is [`ObjectStoreProviderKind::Local`] or
///   [`ObjectStoreProviderKind::Memory`]: the "remote" leg is host-local,
///   nothing is shared.
/// - Any target whose endpoint override points at a loopback host (or cannot
///   be parsed): an S3-compatible service on `127.0.0.1`/`::1`/`localhost` is
///   node-local — the destination node's loopback is a different machine, so
///   attaching to "the same" endpoint would silently miss every blob.
///   Unparseable endpoints classify conservatively as local (fail-safe).
///
/// The classification describes writes accepted UNDER the current policy. A
/// tenant that changed policy (e.g. `LocalOnly` -> `Mirror`) may hold older
/// local-only blobs; the migration executor must reconcile such transitions
/// before relying on [`MigrationLeg::CloudObjectStore`].
pub fn migration_leg(policy: &PlacementPolicy) -> MigrationLeg {
    let (target, remote_is_authoritative) = match policy {
        PlacementPolicy::LocalOnly => return MigrationLeg::LocalReplicateHandoff,
        PlacementPolicy::Mirror {
            target,
            require_ack,
        } => (target, *require_ack),
        PlacementPolicy::Tier { target } | PlacementPolicy::CloudPrimary { target } => {
            (target, true)
        }
    };
    if !remote_is_authoritative || !endpoint_is_shared(target) {
        return MigrationLeg::LocalReplicateHandoff;
    }
    match target.provider {
        ObjectStoreProviderKind::S3
        | ObjectStoreProviderKind::Gcs
        | ObjectStoreProviderKind::Azure => MigrationLeg::CloudObjectStore,
        ObjectStoreProviderKind::Local | ObjectStoreProviderKind::Memory => {
            MigrationLeg::LocalReplicateHandoff
        }
    }
}

/// True when the target's endpoint is reachable from OTHER nodes. No endpoint
/// override means the provider's public service. A loopback host is
/// node-local by definition; an unparseable endpoint or one without a host is
/// treated as not shared (fail-safe: prefer replicate-then-handoff over a
/// migration that can silently miss blobs).
fn endpoint_is_shared(target: &ObjectStorePlacementTarget) -> bool {
    let Some(endpoint) = target.endpoint.as_deref() else {
        return true;
    };
    let Ok(parsed) = Url::parse(endpoint) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Domain(domain)) => !domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => !ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => !ip.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use nimbus_storage::{
        ObjectStorePlacementTarget, ObjectStoreProviderCredentials, ObjectStoreProviderKind,
        PlacementPolicy,
    };

    use super::{MigrationLeg, migration_leg};

    fn target(provider: ObjectStoreProviderKind) -> ObjectStorePlacementTarget {
        ObjectStorePlacementTarget::new(
            provider,
            "bucket",
            ObjectStoreProviderCredentials::Anonymous,
        )
        .expect("test placement target should be valid")
    }

    #[test]
    fn placement_migration_classification() {
        assert_eq!(
            migration_leg(&PlacementPolicy::LocalOnly),
            MigrationLeg::LocalReplicateHandoff
        );

        for provider in [
            ObjectStoreProviderKind::S3,
            ObjectStoreProviderKind::Gcs,
            ObjectStoreProviderKind::Azure,
        ] {
            assert_eq!(
                migration_leg(&PlacementPolicy::Mirror {
                    target: target(provider.clone()),
                    require_ack: true,
                }),
                MigrationLeg::CloudObjectStore
            );
            // A best-effort mirror may hold local-only blobs (a put succeeds
            // even when the remote write fails), so a remote target is NOT
            // enough: migration must replicate-then-handoff.
            assert_eq!(
                migration_leg(&PlacementPolicy::Mirror {
                    target: target(provider),
                    require_ack: false,
                }),
                MigrationLeg::LocalReplicateHandoff
            );
        }

        for provider in [
            ObjectStoreProviderKind::Local,
            ObjectStoreProviderKind::Memory,
        ] {
            assert_eq!(
                migration_leg(&PlacementPolicy::Mirror {
                    target: target(provider),
                    require_ack: true,
                }),
                MigrationLeg::LocalReplicateHandoff
            );
        }
    }

    #[test]
    fn local_only_uses_replicate_handoff() {
        assert_eq!(
            migration_leg(&PlacementPolicy::LocalOnly),
            MigrationLeg::LocalReplicateHandoff
        );
    }

    #[test]
    fn loopback_endpoint_never_classifies_as_cloud() {
        // An S3-compatible service on loopback is node-local: the destination
        // node's 127.0.0.1 is a different machine.
        for endpoint in [
            "http://127.0.0.1:9000",
            "http://localhost:8333",
            "http://[::1]:9000",
            "not a url",
        ] {
            assert_eq!(
                migration_leg(&PlacementPolicy::Tier {
                    target: target(ObjectStoreProviderKind::S3).with_endpoint(endpoint),
                }),
                MigrationLeg::LocalReplicateHandoff,
                "endpoint {endpoint} must classify as node-local"
            );
        }
        // A shared (non-loopback) endpoint override keeps the cloud leg.
        assert_eq!(
            migration_leg(&PlacementPolicy::Tier {
                target: target(ObjectStoreProviderKind::S3)
                    .with_endpoint("https://seaweed.internal.example:8333"),
            }),
            MigrationLeg::CloudObjectStore
        );
        // No endpoint override = the provider's public service.
        assert_eq!(
            migration_leg(&PlacementPolicy::Tier {
                target: target(ObjectStoreProviderKind::S3),
            }),
            MigrationLeg::CloudObjectStore
        );
    }

    #[test]
    fn best_effort_mirror_never_classifies_as_cloud() {
        // require_ack: false means the remote copy may be incomplete.
        for provider in [
            ObjectStoreProviderKind::S3,
            ObjectStoreProviderKind::Gcs,
            ObjectStoreProviderKind::Azure,
            ObjectStoreProviderKind::Local,
            ObjectStoreProviderKind::Memory,
        ] {
            assert_eq!(
                migration_leg(&PlacementPolicy::Mirror {
                    target: target(provider),
                    require_ack: false,
                }),
                MigrationLeg::LocalReplicateHandoff
            );
        }
    }

    #[test]
    fn mirror_to_remote_provider_uses_cloud_object_store() {
        assert_eq!(
            migration_leg(&PlacementPolicy::Mirror {
                target: target(ObjectStoreProviderKind::S3),
                require_ack: true,
            }),
            MigrationLeg::CloudObjectStore
        );
    }

    #[test]
    fn tier_to_remote_provider_uses_cloud_object_store() {
        assert_eq!(
            migration_leg(&PlacementPolicy::Tier {
                target: target(ObjectStoreProviderKind::Gcs),
            }),
            MigrationLeg::CloudObjectStore
        );
    }

    #[test]
    fn cloud_primary_to_remote_provider_uses_cloud_object_store() {
        assert_eq!(
            migration_leg(&PlacementPolicy::CloudPrimary {
                target: target(ObjectStoreProviderKind::Azure),
            }),
            MigrationLeg::CloudObjectStore
        );
    }

    #[test]
    fn placement_to_local_or_memory_provider_uses_replicate_handoff() {
        for policy in [
            PlacementPolicy::Mirror {
                target: target(ObjectStoreProviderKind::Local),
                require_ack: true,
            },
            PlacementPolicy::Tier {
                target: target(ObjectStoreProviderKind::Memory),
            },
            PlacementPolicy::CloudPrimary {
                target: target(ObjectStoreProviderKind::Local),
            },
        ] {
            assert_eq!(migration_leg(&policy), MigrationLeg::LocalReplicateHandoff);
        }
    }
}

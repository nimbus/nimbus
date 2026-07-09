//! Object-placement migration classification.

use nimbus_storage::{ObjectStoreProviderKind, PlacementPolicy};

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
/// `LocalOnly` policies always use [`MigrationLeg::LocalReplicateHandoff`]
/// because tenant blobs live only in the local pack. `Mirror`, `Tier`, and
/// `CloudPrimary` policies use [`MigrationLeg::CloudObjectStore`] only when
/// their target provider is a shared remote object-store kind
/// ([`ObjectStoreProviderKind::S3`], [`ObjectStoreProviderKind::Gcs`], or
/// [`ObjectStoreProviderKind::Azure`]). The same placement modes whose target
/// provider is [`ObjectStoreProviderKind::Local`] or
/// [`ObjectStoreProviderKind::Memory`] remain host-local and therefore still
/// use [`MigrationLeg::LocalReplicateHandoff`].
pub fn migration_leg(policy: &PlacementPolicy) -> MigrationLeg {
    match policy {
        PlacementPolicy::LocalOnly => MigrationLeg::LocalReplicateHandoff,
        PlacementPolicy::Mirror { target, .. }
        | PlacementPolicy::Tier { target }
        | PlacementPolicy::CloudPrimary { target } => match target.provider {
            ObjectStoreProviderKind::S3
            | ObjectStoreProviderKind::Gcs
            | ObjectStoreProviderKind::Azure => MigrationLeg::CloudObjectStore,
            ObjectStoreProviderKind::Local | ObjectStoreProviderKind::Memory => {
                MigrationLeg::LocalReplicateHandoff
            }
        },
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
                    target: target(provider),
                    require_ack: true,
                }),
                MigrationLeg::CloudObjectStore
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

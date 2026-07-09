//! Object-placement migration classification.

use nimbus_storage::{ObjectStoreProviderKind, PlacementPolicy};
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
/// GUARANTEES the remote leg is the read-and-write byte authority, so a
/// destination node that attaches to the shared target serves every blob:
///
/// - `Tier` and `CloudPrimary` with a shared remote target: a `put` persists
///   to the remote leg before (or instead of) the local one, and a local
///   miss on `get` falls through to it.
///
/// Everything else uses [`MigrationLeg::LocalReplicateHandoff`]:
///
/// - `LocalOnly`: blobs live only in the local pack.
/// - `Mirror` (with OR without `require_ack`): the local leg is the primary.
///   With `require_ack: false` the remote copy may be incomplete (a put
///   succeeds locally even when the mirror write fails). Even with
///   `require_ack: true`, the Mirror read path never falls through to the
///   mirror leg on a local miss — a migrated node with an empty local
///   primary could not serve pre-existing blobs. The mirror is redundancy,
///   not authority; migration must move the local leg.
/// - Any mode whose target provider is [`ObjectStoreProviderKind::Local`] or
///   [`ObjectStoreProviderKind::Memory`]: the "remote" leg is host-local,
///   nothing is shared.
/// - Any target whose endpoint override points at a loopback host (including
///   IPv4-mapped forms like `::ffff:127.0.0.1`) or cannot be parsed: an
///   S3-compatible service on loopback is node-local — the destination
///   node's loopback is a different machine. Unparseable endpoints classify
///   conservatively as local (fail-safe).
///
/// The classification describes writes accepted UNDER the current policy. A
/// tenant that changed policy (e.g. `LocalOnly` -> `Tier`) may hold older
/// local-only blobs; the migration executor must reconcile such transitions
/// before relying on [`MigrationLeg::CloudObjectStore`].
pub fn migration_leg(policy: &PlacementPolicy) -> MigrationLeg {
    // Mirror the resolver's endpoint materialization: an S3 target with no
    // stored endpoint can still resolve to AWS_ENDPOINT_URL_S3/AWS_ENDPOINT
    // at store-build time (`resolver.rs` blob_cloud_config), so the
    // classification must judge the endpoint bytes actually go to.
    migration_leg_with_env(policy, crate::resolver::env_s3_endpoint().as_deref())
}

/// [`migration_leg`] with the process-env S3 endpoint fallback made explicit
/// (pure and unit-testable). `env_s3_endpoint` is what
/// `AWS_ENDPOINT_URL_S3`/`AWS_ENDPOINT` would resolve to; it applies only to
/// [`ObjectStoreProviderKind::S3`] targets without a stored endpoint, exactly
/// like the resolver.
fn migration_leg_with_env(policy: &PlacementPolicy, env_s3_endpoint: Option<&str>) -> MigrationLeg {
    let target = match policy {
        PlacementPolicy::LocalOnly | PlacementPolicy::Mirror { .. } => {
            return MigrationLeg::LocalReplicateHandoff;
        }
        PlacementPolicy::Tier { target } | PlacementPolicy::CloudPrimary { target } => target,
    };
    let resolved_endpoint = target.endpoint.as_deref().or_else(|| {
        if target.provider == ObjectStoreProviderKind::S3 {
            env_s3_endpoint
        } else {
            None
        }
    });
    if !endpoint_is_shared(resolved_endpoint) {
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

/// True when the resolved endpoint is reachable from OTHER nodes. No endpoint
/// means the provider's public service. A loopback host is node-local by
/// definition; an unparseable endpoint or one without a host is treated as
/// not shared (fail-safe: prefer replicate-then-handoff over a migration that
/// can silently miss blobs).
fn endpoint_is_shared(endpoint: Option<&str>) -> bool {
    let Some(endpoint) = endpoint else {
        return true;
    };
    let Ok(parsed) = Url::parse(endpoint) else {
        return false;
    };
    match parsed.host() {
        Some(url::Host::Domain(domain)) => {
            // The whole RFC 6761 localhost namespace resolves to loopback:
            // `localhost`, the FQDN form `localhost.`, and any
            // `*.localhost` subdomain.
            let normalized = domain.strip_suffix('.').unwrap_or(domain);
            !(normalized.eq_ignore_ascii_case("localhost")
                || normalized.len() > ".localhost".len()
                    && normalized[normalized.len() - ".localhost".len()..]
                        .eq_ignore_ascii_case(".localhost"))
        }
        Some(url::Host::Ipv4(ip)) => !ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => {
            // Cover IPv4-mapped loopback (::ffff:127.0.0.1) as well.
            !ip.is_loopback()
                && !ip
                    .to_ipv4_mapped()
                    .map(|mapped| mapped.is_loopback())
                    .unwrap_or(false)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use nimbus_storage::{
        ObjectStorePlacementTarget, ObjectStoreProviderCredentials, ObjectStoreProviderKind,
        PlacementPolicy,
    };

    use super::{MigrationLeg, migration_leg, migration_leg_with_env};

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
            // Tier/CloudPrimary on a shared remote target: the remote leg is
            // the read-and-write authority, so migration attaches to it.
            assert_eq!(
                migration_leg(&PlacementPolicy::Tier {
                    target: target(provider.clone()),
                }),
                MigrationLeg::CloudObjectStore
            );
            assert_eq!(
                migration_leg(&PlacementPolicy::CloudPrimary {
                    target: target(provider),
                }),
                MigrationLeg::CloudObjectStore
            );
        }

        for provider in [
            ObjectStoreProviderKind::Local,
            ObjectStoreProviderKind::Memory,
        ] {
            assert_eq!(
                migration_leg(&PlacementPolicy::Tier {
                    target: target(provider),
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
            "http://localhost.:9000",
            "http://LOCALHOST.:9000",
            "http://s3.localhost:9000",
            "http://s3.localhost.:9000",
            "http://[::1]:9000",
            "http://[::ffff:127.0.0.1]:9000",
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
        // Shared (non-loopback) endpoint overrides keep the cloud leg —
        // including lookalike names that merely CONTAIN "localhost".
        for endpoint in [
            "https://seaweed.internal.example:8333",
            "https://notlocalhost.example:9000",
            "https://localhost.example.com:9000",
        ] {
            assert_eq!(
                migration_leg(&PlacementPolicy::Tier {
                    target: target(ObjectStoreProviderKind::S3).with_endpoint(endpoint),
                }),
                MigrationLeg::CloudObjectStore,
                "endpoint {endpoint} is shared"
            );
        }
        // No endpoint override = the provider's public service.
        assert_eq!(
            migration_leg(&PlacementPolicy::Tier {
                target: target(ObjectStoreProviderKind::S3),
            }),
            MigrationLeg::CloudObjectStore
        );
    }

    #[test]
    fn env_injected_loopback_endpoint_never_classifies_as_cloud() {
        // An S3 target with NO stored endpoint can still resolve to
        // AWS_ENDPOINT_URL_S3/AWS_ENDPOINT at store-build time. If that env
        // endpoint is loopback, bytes are node-local and migration must
        // replicate-then-handoff.
        let policy = PlacementPolicy::Tier {
            target: target(ObjectStoreProviderKind::S3),
        };
        assert_eq!(
            migration_leg_with_env(&policy, Some("http://127.0.0.1:9000")),
            MigrationLeg::LocalReplicateHandoff
        );
        // A shared env endpoint keeps the cloud leg.
        assert_eq!(
            migration_leg_with_env(&policy, Some("https://s3.internal.example")),
            MigrationLeg::CloudObjectStore
        );
        // A stored endpoint takes precedence over the env fallback.
        let stored = PlacementPolicy::Tier {
            target: target(ObjectStoreProviderKind::S3)
                .with_endpoint("https://seaweed.internal.example:8333"),
        };
        assert_eq!(
            migration_leg_with_env(&stored, Some("http://127.0.0.1:9000")),
            MigrationLeg::CloudObjectStore
        );
        // The env fallback is S3-specific (mirrors the resolver): a Gcs
        // target ignores it.
        assert_eq!(
            migration_leg_with_env(
                &PlacementPolicy::Tier {
                    target: target(ObjectStoreProviderKind::Gcs),
                },
                Some("http://127.0.0.1:9000")
            ),
            MigrationLeg::CloudObjectStore
        );
    }

    #[test]
    fn mirror_never_classifies_as_cloud() {
        // With require_ack: false the remote copy may be incomplete; even
        // with require_ack: true the Mirror read path never falls through to
        // the mirror leg, so a migrated empty local primary could not serve
        // pre-existing blobs. Either way: replicate-then-handoff.
        for require_ack in [false, true] {
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
                        require_ack,
                    }),
                    MigrationLeg::LocalReplicateHandoff
                );
            }
        }
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

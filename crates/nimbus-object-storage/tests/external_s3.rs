use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use nimbus_blob::{
    BlobCloudConfig, BlobS3Credentials, BlobStore, EncryptedBlobStore, MemoryBlobStore,
    ObjectStoreBlobStore, PlacementBlobStore, PlacementMode,
};
use nimbus_core::{Error, Result};
use nimbus_crypto::{DataEncryptionKey, FramedBlobKey};
use url::Url;

static NEXT_PREFIX: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct ExternalTarget {
    name: &'static str,
    env_name: &'static str,
    endpoint: String,
    config: BlobCloudConfig,
}

fn configured_targets() -> Vec<ExternalTarget> {
    [("rustfs", "RUSTFS"), ("seaweedfs", "SEAWEEDFS")]
        .into_iter()
        .filter_map(|(name, env_name)| configured_target(name, env_name))
        .collect()
}

fn configured_target(name: &'static str, env_name: &'static str) -> Option<ExternalTarget> {
    let endpoint_var = format!("NIMBUS_TEST_{env_name}_S3_URL");
    let endpoint = match env::var(&endpoint_var) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => {
            // Only an ALL-vars-absent target skips. Sibling vars without the
            // URL are a misconfigured harness and must fail loudly instead of
            // silently skipping the live suite.
            let siblings: Vec<String> = ["BUCKET", "REGION", "ACCESS_KEY", "SECRET_KEY"]
                .iter()
                .map(|suffix| format!("NIMBUS_TEST_{env_name}_S3_{suffix}"))
                .filter(|var| env::var_os(var).is_some())
                .collect();
            if !siblings.is_empty() {
                panic!(
                    "external-s3 target {name} is misconfigured: {} set but {endpoint_var} is missing",
                    siblings.join(", ")
                );
            }
            return None;
        }
        Err(env::VarError::NotUnicode(_)) => {
            panic!("{endpoint_var} must be valid Unicode");
        }
    };
    assert_loopback_endpoint(name, env_name, &endpoint);

    let bucket = env::var(format!("NIMBUS_TEST_{env_name}_S3_BUCKET"))
        .unwrap_or_else(|_| "nimbus-rfs8".to_string());
    let region = env::var(format!("NIMBUS_TEST_{env_name}_S3_REGION"))
        .unwrap_or_else(|_| "us-east-1".to_string());
    let access_key_var = format!("NIMBUS_TEST_{env_name}_S3_ACCESS_KEY");
    let secret_key_var = format!("NIMBUS_TEST_{env_name}_S3_SECRET_KEY");
    let credentials = match (env::var(&access_key_var), env::var(&secret_key_var)) {
        (Ok(access_key_id), Ok(secret_access_key)) => BlobS3Credentials::Keys {
            access_key_id,
            secret_access_key,
        },
        (Err(env::VarError::NotPresent), Err(env::VarError::NotPresent)) => {
            BlobS3Credentials::Anonymous
        }
        (Err(env::VarError::NotUnicode(_)), _) => {
            panic!("{access_key_var} must be valid Unicode");
        }
        (_, Err(env::VarError::NotUnicode(_))) => {
            panic!("{secret_key_var} must be valid Unicode");
        }
        _ => {
            panic!(
                "external-s3 target {env_name} is misconfigured: set both {access_key_var} and {secret_key_var}, or neither"
            );
        }
    };

    Some(ExternalTarget {
        name,
        env_name,
        endpoint: endpoint.clone(),
        config: BlobCloudConfig::S3 {
            bucket,
            region: Some(region),
            endpoint: Some(endpoint),
            credentials,
            session_token: None,
        },
    })
}

fn assert_loopback_endpoint(name: &str, env_name: &str, endpoint: &str) {
    let parsed = Url::parse(endpoint).unwrap_or_else(|error| {
        panic!("NIMBUS_TEST_{env_name}_S3_URL for target {name} must be an absolute URL: {error}");
    });
    let host = parsed.host_str().unwrap_or_else(|| {
        panic!("NIMBUS_TEST_{env_name}_S3_URL for target {name} must include a host");
    });
    if !matches!(host, "127.0.0.1" | "::1" | "localhost") {
        panic!(
            "external-s3 target {name} endpoint {endpoint} is not loopback; expected host 127.0.0.1, ::1, or localhost"
        );
    }
}

fn live_targets_or_skip() -> Vec<ExternalTarget> {
    let targets = configured_targets();
    if targets.is_empty() {
        eprintln!("external-s3 mode: skipped (no NIMBUS_TEST_*_S3_URL set)");
    } else {
        for target in &targets {
            eprintln!(
                "external-s3 mode: live target={} endpoint={}",
                target.name, target.endpoint
            );
        }
    }
    targets
}

fn unique_prefix() -> String {
    format!(
        "rfs8-{}-{}",
        std::process::id(),
        NEXT_PREFIX.fetch_add(1, Ordering::SeqCst)
    )
}

fn test_key() -> FramedBlobKey {
    FramedBlobKey::new(DataEncryptionKey::new([0x52; 32]))
}

fn local_leg() -> Arc<dyn BlobStore> {
    Arc::new(EncryptedBlobStore::new(MemoryBlobStore::new(), test_key()))
}

fn remote_leg(target: &ExternalTarget, prefix: &str) -> Result<Arc<dyn BlobStore>> {
    let store = ObjectStoreBlobStore::from_cloud_config(target.config.clone(), prefix)?;
    Ok(Arc::new(EncryptedBlobStore::new(store, test_key())))
}

fn placement_store(mode: PlacementMode) -> PlacementBlobStore {
    PlacementBlobStore::new(local_leg(), mode)
}

fn panic_target_error(target: &ExternalTarget, test_name: &str, error: Error) -> ! {
    panic!(
        "{test_name} failed for external-s3 target={} endpoint={}: {error}",
        target.name, target.endpoint
    );
}

#[tokio::test]
async fn external_s3_mirror_roundtrip() {
    for target in live_targets_or_skip() {
        if let Err(error) = mirror_roundtrip_for_target(&target).await {
            panic_target_error(&target, "external_s3_mirror_roundtrip", error);
        }
    }
}

async fn mirror_roundtrip_for_target(target: &ExternalTarget) -> Result<()> {
    let prefix = unique_prefix();
    let remote = remote_leg(target, &prefix)?;
    let remote_probe = remote_leg(target, &prefix)?;
    let local = local_leg();
    let placement = PlacementBlobStore::new(
        local.clone(),
        PlacementMode::Mirror {
            mirror: remote,
            require_ack: true,
        },
    );
    let bytes = Bytes::from_static(b"rfs8 mirror bytes");

    let hash = placement.put(bytes.clone()).await?;

    assert_eq!(
        remote_probe.get(&hash).await?,
        bytes,
        "independent remote leg should serve mirrored plaintext"
    );
    assert!(
        local.has(&hash).await?,
        "local leg should hold mirrored blob"
    );
    assert_eq!(
        placement.get(&hash).await?,
        bytes,
        "placement store should round-trip mirrored bytes"
    );

    placement.release(&hash).await?;
    assert!(
        !remote_probe.has(&hash).await?,
        "remote leg should not retain blob after placement release"
    );
    Ok(())
}

#[tokio::test]
async fn external_s3_tier_roundtrip() {
    for target in live_targets_or_skip() {
        if let Err(error) = tier_roundtrip_for_target(&target).await {
            panic_target_error(&target, "external_s3_tier_roundtrip", error);
        }
    }
}

async fn tier_roundtrip_for_target(target: &ExternalTarget) -> Result<()> {
    let prefix = unique_prefix();
    let bytes = Bytes::from_static(b"rfs8 tier bytes");
    let placement = placement_store(PlacementMode::Tier {
        cold: remote_leg(target, &prefix)?,
    });

    let hash = placement.put(bytes.clone()).await?;

    let fresh = placement_store(PlacementMode::Tier {
        cold: remote_leg(target, &prefix)?,
    });
    let got = fresh.get(&hash).await?;
    assert_eq!(got.len(), bytes.len(), "tier read length should match");
    assert_eq!(got, bytes, "tier read should fall through to cold leg");

    fresh.release(&hash).await?;
    Ok(())
}

#[tokio::test]
async fn external_s3_cloud_primary_roundtrip() {
    for target in live_targets_or_skip() {
        if let Err(error) = cloud_primary_roundtrip_for_target(&target).await {
            panic_target_error(&target, "external_s3_cloud_primary_roundtrip", error);
        }
    }
}

async fn cloud_primary_roundtrip_for_target(target: &ExternalTarget) -> Result<()> {
    let prefix = unique_prefix();
    let bytes = Bytes::from_static(b"rfs8 cloud-primary bytes");
    let placement = placement_store(PlacementMode::CloudPrimary {
        cloud: remote_leg(target, &prefix)?,
    });

    let hash = placement.put(bytes.clone()).await?;

    let fresh = placement_store(PlacementMode::CloudPrimary {
        cloud: remote_leg(target, &prefix)?,
    });
    assert!(
        fresh.has(&hash).await?,
        "fresh cloud-primary placement should see remote blob"
    );
    let got = fresh.get(&hash).await?;
    assert_eq!(
        got.len(),
        bytes.len(),
        "cloud-primary read length should match"
    );
    assert_eq!(
        got, bytes,
        "cloud-primary read should fall through to cloud leg"
    );

    fresh.release(&hash).await?;
    Ok(())
}

#[tokio::test]
async fn external_s3_range_length_checked() {
    for target in live_targets_or_skip() {
        if let Err(error) = range_length_checked_for_target(&target).await {
            panic_target_error(&target, "external_s3_range_length_checked", error);
        }
    }
}

async fn range_length_checked_for_target(target: &ExternalTarget) -> Result<()> {
    let prefix = unique_prefix();
    let remote = remote_leg(target, &prefix)?;
    let bytes = (0..1_048_576usize)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let hash = remote.put(Bytes::from(bytes.clone())).await?;

    let range = remote.get_range(&hash, 4096..8192).await?;
    assert_eq!(range.len(), 4096, "range read should not be clamped");
    assert_eq!(
        range,
        Bytes::copy_from_slice(&bytes[4096..8192]),
        "range read should return the requested plaintext slice"
    );
    assert!(
        remote.get_range(&hash, 1_048_577..1_048_578).await.is_err(),
        "start beyond blob length should error instead of returning clamped bytes"
    );

    remote.release(&hash).await?;
    Ok(())
}

#[tokio::test]
async fn external_s3_mirror_without_ack_survives_unreachable_remote() {
    let target = ExternalTarget {
        name: "closed-loopback",
        env_name: "CLOSED_LOOPBACK",
        endpoint: "http://127.0.0.1:9".to_string(),
        config: BlobCloudConfig::S3 {
            bucket: "nimbus-rfs8".to_string(),
            region: Some("us-east-1".to_string()),
            endpoint: Some("http://127.0.0.1:9".to_string()),
            credentials: BlobS3Credentials::Anonymous,
            session_token: None,
        },
    };
    assert_loopback_endpoint(target.name, target.env_name, &target.endpoint);

    let best_effort_local = local_leg();
    let best_effort = PlacementBlobStore::new(
        best_effort_local.clone(),
        PlacementMode::Mirror {
            mirror: remote_leg(&target, &unique_prefix()).unwrap_or_else(|error| {
                panic!("build unreachable best-effort mirror leg: {error}");
            }),
            require_ack: false,
        },
    );
    let bytes = Bytes::from_static(b"local survives unreachable mirror");
    let hash = best_effort
        .put(bytes.clone())
        .await
        .unwrap_or_else(|error| {
            panic!("best-effort mirror put should succeed locally: {error}");
        });

    assert!(
        best_effort_local.has(&hash).await.unwrap_or_else(|error| {
            panic!("best-effort local leg has check should succeed: {error}");
        }),
        "best-effort mirror should store bytes locally"
    );
    assert_eq!(
        best_effort.get(&hash).await.unwrap_or_else(|error| {
            panic!("best-effort mirror get should serve local bytes: {error}");
        }),
        bytes
    );

    let require_ack = PlacementBlobStore::new(
        local_leg(),
        PlacementMode::Mirror {
            mirror: remote_leg(&target, &unique_prefix()).unwrap_or_else(|error| {
                panic!("build unreachable acked mirror leg: {error}");
            }),
            require_ack: true,
        },
    );
    assert!(
        require_ack
            .put(Bytes::from_static(b"acked mirror must fail"))
            .await
            .is_err(),
        "require_ack mirror put must fail when the remote leg is unreachable"
    );
}

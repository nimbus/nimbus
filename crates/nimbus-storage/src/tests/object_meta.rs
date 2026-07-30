use super::*;
use crate::traits::{
    delete_multipart_upload_direct, delete_object_manifest_direct, put_multipart_upload_direct,
    put_object_manifest_direct,
};
use crate::{
    OBJECT_MANIFEST_TABLE, OBJECT_MULTIPART_TABLE, ObjectChecksums, ObjectChunkRef, ObjectManifest,
    ObjectManifestAttributes, ObjectMetaRead, ObjectMultipartPart, ObjectMultipartUpload,
};

const BUCKET: &str = "launch-bucket";

fn manifest(key: &str, blob_hash: &str) -> ObjectManifest {
    let mut metadata = serde_json::Map::new();
    metadata.insert("owner".to_string(), json!("storage-tests"));
    let mut attributes = ObjectManifestAttributes::new("\"etag\"", 1_776_960_000_000);
    attributes.content_type = Some("text/plain".to_string());
    attributes.user_metadata = metadata;
    attributes.checksums = ObjectChecksums {
        content_md5: Some("CY9rzUYh03PK3k6DJie09g==".to_string()),
        crc64nvme: Some("AAAAAAAAAAA=".to_string()),
        sha256: Some(
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
        ),
    };
    ObjectManifest::whole(BUCKET, key, 12, blob_hash, attributes).expect("manifest should be valid")
}

// Which stores implement `ObjectMetaRead` is pinned at build time next to the
// impls in `traits::provider_impls`; the tests below cover what those reads
// return.
#[test]
fn object_meta_store_round_trips_manifest_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = manifest("photos/2026/launch.txt", "hash-a");

    let commit = put_object_manifest_direct(&store, &first).expect("manifest put should commit");
    let fetched = store
        .get_object_manifest(&first.bucket, &first.key)
        .expect("manifest get should succeed")
        .expect("manifest should exist");

    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(commit.writes[0].table.as_str(), OBJECT_MANIFEST_TABLE);
    assert_eq!(fetched, first);
}

#[test]
fn object_meta_store_updates_existing_manifest_atomically_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = manifest("objects/report.pdf", "hash-a");
    let mut second = manifest("objects/report.pdf", "hash-b");
    second.size = 99;
    second.etag = "\"etag-2\"".to_string();

    put_object_manifest_direct(&store, &first).expect("initial manifest put should commit");
    let commit =
        put_object_manifest_direct(&store, &second).expect("manifest update should commit");
    let fetched = store
        .get_object_manifest(&second.bucket, &second.key)
        .expect("manifest get should succeed")
        .expect("manifest should exist");

    assert_eq!(commit.sequence, SequenceNumber(2));
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(fetched, second);
}

#[test]
fn object_meta_store_lists_by_prefix_and_deletes_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let keep = manifest("alpha/keep.txt", "hash-a");
    let drop = manifest("alpha/drop.txt", "hash-b");
    let other = manifest("beta/other.txt", "hash-c");

    put_object_manifest_direct(&store, &keep).unwrap();
    put_object_manifest_direct(&store, &drop).unwrap();
    put_object_manifest_direct(&store, &other).unwrap();

    let listed = store
        .list_object_manifests(BUCKET, "alpha/", 10)
        .expect("manifest list should succeed");
    assert_eq!(
        listed
            .iter()
            .map(|manifest| manifest.key.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha/drop.txt", "alpha/keep.txt"]
    );

    let (commit, deleted) = delete_object_manifest_direct(&store, &drop.bucket, &drop.key)
        .expect("manifest delete should succeed")
        .expect("manifest should exist");
    assert_eq!(commit.sequence, SequenceNumber(4));
    assert_eq!(deleted, drop);
    assert!(
        store
            .get_object_manifest(BUCKET, "alpha/drop.txt")
            .expect("manifest get should succeed")
            .is_none()
    );
}

#[test]
fn object_meta_store_isolates_buckets_for_the_same_key() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = manifest("shared/key.txt", "hash-a");
    let mut second = manifest("shared/key.txt", "hash-b");
    second.bucket = "archive-bucket".to_string();

    put_object_manifest_direct(&store, &first).unwrap();
    put_object_manifest_direct(&store, &second).unwrap();

    assert_eq!(
        store
            .get_object_manifest(BUCKET, "shared/key.txt")
            .expect("first bucket lookup")
            .expect("first bucket object")
            .blob_layout,
        first.blob_layout
    );
    assert_eq!(
        store
            .get_object_manifest("archive-bucket", "shared/key.txt")
            .expect("second bucket lookup")
            .expect("second bucket object")
            .blob_layout,
        second.blob_layout
    );
    assert_eq!(
        store
            .list_object_manifests(BUCKET, "shared/", 10)
            .expect("first bucket list")
            .len(),
        1
    );
}

#[test]
fn object_meta_store_persists_through_sqlite() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.sqlite3");
    let manifest = manifest("durable/object.txt", "hash-sqlite");

    {
        let store = SqliteTenantStore::open(&path).expect("sqlite store should open");
        put_object_manifest_direct(&store, &manifest).expect("manifest put should commit");
    }

    let reopened = SqliteTenantStore::open(&path).expect("sqlite store should reopen");
    let fetched = reopened
        .get_object_manifest(&manifest.bucket, &manifest.key)
        .expect("manifest get should succeed")
        .expect("manifest should exist");
    assert_eq!(fetched, manifest);
}

#[test]
fn object_meta_store_rejects_invalid_keys_before_document_write() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let invalid = ObjectManifest::whole(
        BUCKET,
        "",
        1,
        "hash",
        ObjectManifestAttributes::new("\"etag\"", 1),
    );

    assert!(matches!(invalid, Err(Error::InvalidInput(_))));
    assert_eq!(
        store
            .list_object_manifests(BUCKET, "", 1)
            .expect("empty prefix list should succeed")
            .len(),
        0
    );
}

#[test]
fn object_manifest_rejects_malformed_chunk_layout() {
    let offset_gap = ObjectManifest::chunked(
        BUCKET,
        "chunked/gap.bin",
        4,
        vec![ObjectChunkRef {
            blob_hash: "hash-a".to_string(),
            offset: 1,
            len: 4,
        }],
        ObjectManifestAttributes::new("\"etag\"", 1),
    )
    .expect_err("first chunk must start at offset zero");
    assert!(offset_gap.to_string().contains("expected offset 0"));

    let size_mismatch = ObjectManifest::chunked(
        BUCKET,
        "chunked/size.bin",
        5,
        vec![ObjectChunkRef {
            blob_hash: "hash-a".to_string(),
            offset: 0,
            len: 4,
        }],
        ObjectManifestAttributes::new("\"etag\"", 1),
    )
    .expect_err("chunk lengths must sum to object size");
    assert!(size_mismatch.to_string().contains("object size 5"));
}

#[test]
fn object_meta_store_round_trips_multipart_upload_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let mut upload = ObjectMultipartUpload::new(
        "upload-1",
        BUCKET,
        "large/video.mp4",
        Some("video/mp4".to_string()),
        serde_json::Map::new(),
        1_776_960_000_000,
    )
    .expect("upload should validate");
    upload
        .replace_part(ObjectMultipartPart {
            part_number: 2,
            blob_hash: "hash-b".to_string(),
            size: 4,
            etag: "\"part-b\"".to_string(),
            checksums: ObjectChecksums::default(),
            last_modified_millis: 1_776_960_000_002,
        })
        .expect("second part should insert");
    upload
        .replace_part(ObjectMultipartPart {
            part_number: 1,
            blob_hash: "hash-a".to_string(),
            size: 3,
            etag: "\"part-a\"".to_string(),
            checksums: ObjectChecksums {
                content_md5: Some("AAAAAAAAAAAAAAAAAAAAAA==".to_string()),
                crc64nvme: Some("AAAAAAAAAAA=".to_string()),
                sha256: Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                ),
            },
            last_modified_millis: 1_776_960_000_001,
        })
        .expect("first part should insert in order");

    let commit = put_multipart_upload_direct(&store, &upload).expect("multipart put should commit");
    let fetched = store
        .get_multipart_upload("upload-1")
        .expect("multipart get should succeed")
        .expect("multipart upload should exist");

    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(commit.writes[0].table.as_str(), OBJECT_MULTIPART_TABLE);
    assert_eq!(fetched.parts[0].part_number, 1);
    assert_eq!(fetched.parts[1].part_number, 2);
    assert_eq!(fetched, upload);

    let listed = store
        .list_multipart_uploads(BUCKET, "large/", 10)
        .expect("multipart list should succeed");
    assert_eq!(listed, vec![upload.clone()]);

    let (commit, deleted) = delete_multipart_upload_direct(&store, "upload-1")
        .expect("multipart delete should succeed")
        .expect("multipart upload should exist");
    assert_eq!(commit.sequence, SequenceNumber(2));
    assert_eq!(deleted, upload);
    assert!(
        store
            .get_multipart_upload("upload-1")
            .expect("multipart get after delete should succeed")
            .is_none()
    );
}

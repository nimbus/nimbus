use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use nimbus_core::{StorageErrorKind, Timestamp};
use nimbus_crypto::{DataEncryptionKey, FramedBlobKey};

use super::*;
use crate::local::{self, QuarantineCheck, RECORD_MAGIC};
use crate::{
    BlobGc, BlobStore, EncryptedBlobStore, LocalPackStore, LocalPackStoreOptions, StaticBlobRoots,
};

fn craft_checkpoint(last: u64, max_seen: u64, complete: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NBLSCP1\n");
    bytes.extend_from_slice(&last.to_le_bytes());
    bytes.extend_from_slice(&max_seen.to_le_bytes());
    bytes.push(u8::from(complete));
    let digest = blake3::hash(&bytes);
    bytes.extend_from_slice(digest.as_bytes());
    bytes
}

fn open_temp(target: u64) -> (tempfile::TempDir, LocalPackStore) {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let store =
        LocalPackStore::open_with_pack_target(dir.path(), target).expect("store should open");
    (dir, store)
}

async fn entry_for(store: &LocalPackStore, hash: BlobHash) -> PackEntry {
    store
        .blocking(move |state| Ok(state.index.get(&hash).copied().expect("hash is indexed")))
        .await
        .expect("entry lookup succeeds")
}

async fn flip_first_body_byte(
    dir: &tempfile::TempDir,
    store: &LocalPackStore,
    hash: BlobHash,
) -> PackEntry {
    let entry = entry_for(store, hash).await;
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    let body_offset = entry.offset + RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("pack opens for corruption");
    file.seek(SeekFrom::Start(body_offset))
        .expect("seek to body byte");
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).expect("read body byte");
    byte[0] ^= 0xff;
    file.seek(SeekFrom::Start(body_offset))
        .expect("seek back to body byte");
    file.write_all(&byte).expect("write flipped body byte");
    file.sync_data().expect("corruption lands on disk");
    entry
}

fn key(seed: &str) -> FramedBlobKey {
    FramedBlobKey::new(DataEncryptionKey::new(
        *blake3::hash(seed.as_bytes()).as_bytes(),
    ))
}

async fn smash_record_magic(dir: &tempfile::TempDir, store: &LocalPackStore, hash: BlobHash) {
    let entry = entry_for(store, hash).await;
    let path = local::pack_path(&dir.path().join("packs"), entry.pack_id);
    let mut file = OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("pack opens for corruption");
    file.seek(SeekFrom::Start(entry.offset))
        .expect("seek to record magic");
    file.write_all(b"XXXX").expect("smash record magic");
    file.sync_data().expect("corruption lands on disk");
}

mod detection;
mod quarantine;
mod rebuild;

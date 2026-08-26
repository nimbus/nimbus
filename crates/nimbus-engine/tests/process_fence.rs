use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nimbus_core::StorageErrorKind;
use nimbus_engine::{
    Engine, EnginePersistenceConfig, LocalEncryptionConfig, LocalKeyProviderConfig,
    MasterKeyFileConfig,
};
use tempfile::TempDir;

const CHILD_DATA_ROOT: &str = "NIMBUS_ENGINE_FENCE_CHILD_DATA_ROOT";
const CHILD_KEY_FILE: &str = "NIMBUS_ENGINE_FENCE_CHILD_KEY_FILE";
const CHILD_PROOF_FILE: &str = "engine-fence-child.proof";

fn encrypted_redb_config(data_root: &Path, key_file: &Path) -> EnginePersistenceConfig {
    EnginePersistenceConfig::embedded_default(data_root).with_local_encryption(
        LocalEncryptionConfig::Enabled(LocalKeyProviderConfig::MasterKeyFile(
            MasterKeyFileConfig {
                path: key_file.to_path_buf(),
            },
        )),
    )
}

#[tokio::test]
async fn encrypted_embedded_engine_refuses_a_second_process_on_the_same_root() {
    let fixture = TempDir::new().expect("engine fence fixture should build");
    let data_root = fixture.path().join("data");
    let key_file = fixture.path().join("master.key");
    fs::write(&key_file, [0x42_u8; 32]).expect("master key fixture should write");

    let engine = Engine::new_with_persistence_config(encrypted_redb_config(&data_root, &key_file))
        .await
        .expect("first encrypted engine should open");

    let output = Command::new(env::current_exe().expect("test executable should resolve"))
        .arg("--ignored")
        .arg("--exact")
        .arg("encrypted_embedded_engine_fence_child")
        .arg("--nocapture")
        .env(CHILD_DATA_ROOT, &data_root)
        .env(CHILD_KEY_FILE, &key_file)
        .output()
        .expect("fresh child process should start");

    assert!(
        output.status.success(),
        "fresh process did not observe the live Engine fence\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(data_root.join(CHILD_PROOF_FILE))
            .expect("child must leave an out-of-band execution proof"),
        b"busy",
        "a zero-test child invocation must not count as process-boundary proof"
    );

    drop(engine);
    Engine::new_with_persistence_config(encrypted_redb_config(&data_root, &key_file))
        .await
        .expect("the root must reopen after the first Engine drops");
}

#[tokio::test]
#[ignore = "subprocess entry point; exercised by the parent test"]
async fn encrypted_embedded_engine_fence_child() {
    let data_root = PathBuf::from(
        env::var_os(CHILD_DATA_ROOT).expect("parent must provide the Engine data root"),
    );
    let key_file =
        PathBuf::from(env::var_os(CHILD_KEY_FILE).expect("parent must provide the key file"));
    let error = Engine::new_with_persistence_config(encrypted_redb_config(&data_root, &key_file))
        .await
        .err()
        .expect("a live Engine in another process must own the root");
    assert_eq!(
        error.storage_kind(),
        Some(StorageErrorKind::Busy),
        "root contention must use the machine-readable busy class: {error}"
    );
    fs::write(data_root.join(CHILD_PROOF_FILE), b"busy")
        .expect("child should publish its out-of-band execution proof");
}

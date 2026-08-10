use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use tempfile::tempdir;

use super::*;

#[test]
fn teardown_store_rejects_corruption_and_unknown_versions() {
    let root = tempdir().expect("temporary state root should open");
    let store = SystemdTeardownStore::open(root.path()).expect("store should open");
    store
        .transact(|_| Ok(()))
        .expect("initial state should persist");
    let state_path = root.path().join(STATE_FILE);

    fs::write(root.path().join(TEMP_FILE), b"{torn-temporary-state")
        .expect("torn temporary state should write");
    SystemdTeardownStore::open(root.path())
        .expect("an uncommitted temporary write must not replace durable state");

    let mut checksum_crossed: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("durable state should read"))
            .expect("durable state should decode");
    checksum_crossed["checksum"] = serde_json::Value::String("00".repeat(32));
    fs::write(
        &state_path,
        serde_json::to_vec(&checksum_crossed).expect("crossed checksum should encode"),
    )
    .expect("crossed checksum should write");
    assert!(SystemdTeardownStore::open(root.path()).is_err());

    fs::write(&state_path, b"{not-json").expect("corrupt state should write");
    assert!(SystemdTeardownStore::open(root.path()).is_err());

    fs::write(
        &state_path,
        br#"{"version":3,"checksum":"0000000000000000000000000000000000000000000000000000000000000000","payload":{"activation":{},"drain":{},"stop":{}}}"#,
    )
    .expect("versioned state should write");
    assert!(SystemdTeardownStore::open(root.path()).is_err());
}

#[test]
fn teardown_store_serializes_independent_instances() {
    let root = tempdir().expect("temporary state root should open");
    let first = SystemdTeardownStore::open(root.path()).expect("first store should open");
    let second = SystemdTeardownStore::open(root.path()).expect("second store should open");
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first_thread = thread::spawn(move || {
        first
            .transact(|_| {
                locked_tx.send(()).expect("lock signal should send");
                release_rx.recv().expect("release signal should receive");
                Ok(())
            })
            .expect("first transaction should persist");
    });
    locked_rx.recv().expect("first transaction should lock");

    let (complete_tx, complete_rx) = mpsc::channel();
    let second_thread = thread::spawn(move || {
        second
            .transact(|_| Ok(()))
            .expect("second transaction should persist");
        complete_tx.send(()).expect("completion should send");
    });
    assert!(complete_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).expect("release should send");
    complete_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("second transaction should complete after release");
    first_thread.join().expect("first thread should join");
    second_thread.join().expect("second thread should join");
}

#[test]
fn teardown_store_serializes_independent_processes() {
    let root = tempdir().expect("temporary state root should open");
    SystemdTeardownStore::open(root.path()).expect("parent store should open");
    let ready = root.path().join("child-ready");
    let release = root.path().join("release-child");
    let mut child = Command::new(std::env::current_exe().expect("test executable should resolve"))
        .arg("--exact")
        .arg("systemd_transient::teardown_store::tests::teardown_store_cross_process_lock_child")
        .arg("--nocapture")
        .env("NIMBUS_SYSTEMD_TEARDOWN_STORE_CHILD_ROOT", root.path())
        .spawn()
        .expect("lock child should spawn");

    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if !ready.exists() {
        let _ = child.kill();
        let child_status = child.wait().expect("failed lock child should reap");
        panic!("lock child did not acquire the store before {child_status}");
    }

    let parent_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.path().join(LOCK_FILE))
        .expect("parent lock file should open");
    let contention = parent_lock
        .try_lock_exclusive()
        .expect_err("a second process must not acquire the store lock while the child holds it");
    assert_eq!(
        contention.kind(),
        ErrorKind::WouldBlock,
        "cross-process contention must be reported as a held lock"
    );
    fs::write(&release, b"release").expect("child release signal should write");
    let child_status = child.wait().expect("lock child should reap");
    assert!(child_status.success(), "lock child failed: {child_status}");
    parent_lock
        .try_lock_exclusive()
        .expect("parent should acquire the store lock after child exit");
    FileExt::unlock(&parent_lock).expect("parent store lock should release");
}

#[test]
fn teardown_store_cross_process_lock_child() {
    let Ok(root) = std::env::var("NIMBUS_SYSTEMD_TEARDOWN_STORE_CHILD_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let store = SystemdTeardownStore::open(&root).expect("child store should open");
    store
        .transact(|_| {
            fs::write(root.join("child-ready"), b"ready")
                .map_err(|error| store_io("publish child lock readiness", error))?;
            let release = root.join("release-child");
            let deadline = Instant::now() + Duration::from_secs(5);
            while !release.exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if !release.exists() {
                return Err(Error::Internal(
                    "timed out waiting to release systemd teardown child lock".to_owned(),
                ));
            }
            Ok(())
        })
        .expect("child transaction should persist");
}

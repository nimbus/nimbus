use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::IsolatedRuntimeTestCase;

pub(crate) fn acquire_runtime_suite_lock() -> MutexGuard<'static, ()> {
    static IN_PROCESS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    IN_PROCESS_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) struct RuntimeSuiteSubprocessLockGuard {
    #[cfg(unix)]
    file: std::fs::File,
}

fn acquire_runtime_suite_subprocess_lock() -> RuntimeSuiteSubprocessLockGuard {
    #[cfg(unix)]
    {
        const LOCK_EX: i32 = 2;

        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }

        // The isolated runtime tests spawn nested test binaries. Keep those
        // subprocess runs serialized across the host so coverage and other
        // multi-binary lanes do not overlap locker-sensitive V8 state.
        let path = std::env::temp_dir().join("nimbus-runtime-subprocess-suite.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .expect("runtime subprocess suite lockfile should open");
        let status = unsafe { flock(file.as_raw_fd(), LOCK_EX) };
        assert_eq!(
            status, 0,
            "runtime subprocess suite lock should acquire successfully"
        );
        RuntimeSuiteSubprocessLockGuard { file }
    }

    #[cfg(not(unix))]
    {
        RuntimeSuiteSubprocessLockGuard {}
    }
}

pub(crate) fn run_v8_sensitive_runtime_test_in_subprocess(case: IsolatedRuntimeTestCase) {
    let _guard = acquire_runtime_suite_lock();
    let _subprocess_guard = acquire_runtime_suite_subprocess_lock();
    let output = std::process::Command::new(
        std::env::current_exe().expect("current test binary path should resolve"),
    )
    .arg("--ignored")
    .arg("--exact")
    .arg(case.subprocess_test_name())
    .arg("--nocapture")
    .output()
    .expect("isolated runtime test subprocess should launch");
    assert!(
        output.status.success(),
        "{} (exit status: {})\nstdout:\n{}\nstderr:\n{}",
        case.failure_context("isolated runtime test subprocess should succeed"),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

pub(crate) struct SnapshotResetTestLockGuard {
    _in_process_guard: MutexGuard<'static, ()>,
    #[cfg(unix)]
    file: std::fs::File,
}

pub(crate) fn acquire_snapshot_reset_test_lock() -> SnapshotResetTestLockGuard {
    static IN_PROCESS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let in_process_guard = IN_PROCESS_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    #[cfg(unix)]
    {
        const LOCK_EX: i32 = 2;

        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }

        let path = std::env::temp_dir().join("nimbus-runtime-snapshot-reset-test.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .expect("snapshot reset test lockfile should open");
        let status = unsafe { flock(file.as_raw_fd(), LOCK_EX) };
        assert_eq!(
            status, 0,
            "snapshot reset test lock should acquire successfully"
        );
        SnapshotResetTestLockGuard {
            _in_process_guard: in_process_guard,
            file,
        }
    }

    #[cfg(not(unix))]
    {
        SnapshotResetTestLockGuard {
            _in_process_guard: in_process_guard,
        }
    }
}

#[cfg(unix)]
impl Drop for SnapshotResetTestLockGuard {
    fn drop(&mut self) {
        const LOCK_UN: i32 = 8;

        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }

        let _ = unsafe { flock(self.file.as_raw_fd(), LOCK_UN) };
    }
}

#[cfg(unix)]
impl Drop for RuntimeSuiteSubprocessLockGuard {
    fn drop(&mut self) {
        const LOCK_UN: i32 = 8;

        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }

        let _ = unsafe { flock(self.file.as_raw_fd(), LOCK_UN) };
    }
}

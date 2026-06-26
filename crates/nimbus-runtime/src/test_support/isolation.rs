use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::IsolatedRuntimeTestCase;

pub(crate) struct RuntimeSuiteLockGuard {
    _permit: OwnedSemaphorePermit,
}

pub(crate) async fn acquire_runtime_suite_lock() -> RuntimeSuiteLockGuard {
    RuntimeSuiteLockGuard {
        _permit: runtime_suite_semaphore()
            .clone()
            .acquire_owned()
            .await
            .expect("runtime suite semaphore should stay open"),
    }
}

pub(crate) fn acquire_runtime_suite_lock_blocking() -> RuntimeSuiteLockGuard {
    let semaphore = runtime_suite_semaphore().clone();
    loop {
        match semaphore.clone().try_acquire_owned() {
            Ok(permit) => return RuntimeSuiteLockGuard { _permit: permit },
            Err(tokio::sync::TryAcquireError::NoPermits) => {
                std::thread::park_timeout(std::time::Duration::from_millis(1));
            }
            Err(tokio::sync::TryAcquireError::Closed) => {
                panic!("runtime suite semaphore should stay open");
            }
        }
    }
}

fn runtime_suite_semaphore() -> &'static Arc<Semaphore> {
    static IN_PROCESS_LOCK: OnceLock<Arc<Semaphore>> = OnceLock::new();
    IN_PROCESS_LOCK.get_or_init(|| Arc::new(Semaphore::new(1)))
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
    let _guard = acquire_runtime_suite_lock_blocking();
    let _subprocess_guard = acquire_runtime_suite_subprocess_lock();
    let tmp_dir = create_runtime_subprocess_tmp_dir(case);
    let output = std::process::Command::new(
        std::env::current_exe().expect("current test binary path should resolve"),
    )
    .current_dir(env!("CARGO_MANIFEST_DIR"))
    .env("RUST_TEST_THREADS", "1")
    .env("TERM", "xterm-256color")
    .env("TMPDIR", &tmp_dir)
    .env("TEMP", &tmp_dir)
    .env("TMP", &tmp_dir)
    .env_remove("NODE_OPTIONS")
    .env_remove("NODE_TLS_REJECT_UNAUTHORIZED")
    .arg("--ignored")
    .arg("--exact")
    .arg(case.subprocess_test_name())
    .arg("--nocapture")
    .output()
    .expect("isolated runtime test subprocess should launch");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    assert!(
        output.status.success(),
        "{} (exit status: {})\nstdout:\n{}\nstderr:\n{}",
        case.failure_context("isolated runtime test subprocess should succeed"),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Run a CRASH-CONTROL test (one that aborts BY DESIGN) in a fresh subprocess and assert the
/// child died BY SIGNAL (SIGABRT/SIGBUS/SIGSEGV — the cross-profile RO-heap crash). This is
/// the inverse of `run_v8_sensitive_runtime_test_in_subprocess`: a control that EXITS NORMALLY
/// (success OR a plain test failure) fails the parent — that means either the guarded crash
/// REGRESSED (bug returned) or the control was defanged. Either way the oracle must go RED.
/// `max_attempts` handles racy controls (a concurrent race that crashes ~99% per run): retry
/// up to N times and pass on the first signal-death; a deterministic control uses a small N.
#[cfg(feature = "v8-pointer-compression")]
pub(crate) fn run_v8_crash_control_in_subprocess(case: IsolatedRuntimeTestCase, max_attempts: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        let _guard = acquire_runtime_suite_lock_blocking();
        let _subprocess_guard = acquire_runtime_suite_subprocess_lock();
        let mut last_observation = String::new();
        for attempt in 1..=max_attempts.max(1) {
            let tmp_dir = create_runtime_subprocess_tmp_dir(case);
            let output = std::process::Command::new(
                std::env::current_exe().expect("current test binary path should resolve"),
            )
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("RUST_TEST_THREADS", "1")
            .env("TMPDIR", &tmp_dir)
            .env("TEMP", &tmp_dir)
            .env("TMP", &tmp_dir)
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_TLS_REJECT_UNAUTHORIZED")
            .arg("--ignored")
            .arg("--exact")
            .arg(case.subprocess_test_name())
            .arg("--nocapture")
            .output()
            .expect("crash-control subprocess should launch");
            let _ = std::fs::remove_dir_all(&tmp_dir);
            // SIGABRT=6 (libc++ `vector.h:415` abort, the OOB direction), SIGBUS=10
            // (wrong-object deref direction), SIGSEGV=11 (defensive). Any is the control
            // crashing as required.
            if matches!(output.status.signal(), Some(6 | 10 | 11)) {
                return;
            }
            last_observation = format!(
                "attempt {attempt}/{}: status {} (signal {:?})\nstdout:\n{}\nstderr:\n{}",
                max_attempts.max(1),
                output.status,
                output.status.signal(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
        panic!(
            "{} — the crash control did NOT abort by signal across {} attempt(s). Either the \
             guarded cross-profile RO-heap crash REGRESSED (the bug returned and should crash) \
             or the control was defanged. A control that can pass without crashing is the \
             vacuous oracle this guards against.\n{last_observation}",
            case.failure_context("crash control must abort by SIGABRT/SIGBUS"),
            max_attempts.max(1),
        );
    }
    // Signal-death assertion is Unix-only; the cage feature ships only on Unix targets.
    #[cfg(not(unix))]
    {
        let _ = (case, max_attempts);
    }
}

fn create_runtime_subprocess_tmp_dir(case: IsolatedRuntimeTestCase) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    dir.push(format!(
        "nimbus-runtime-subprocess-{}-{}-{timestamp}",
        std::process::id(),
        sanitize_tmp_component(case.metadata().id())
    ));
    std::fs::create_dir_all(&dir).expect("runtime subprocess tmp dir should be created");
    dir
}

fn sanitize_tmp_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect()
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

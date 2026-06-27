use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::IsolatedRuntimeTestCase;

/// Hand-declared `flock(2)` advisory whole-file locking. The crate avoids a `libc`/`rustix`
/// dependency, so the syscall and its flag constants (identical across the Unix targets we run
/// on) are declared ONCE here and wrapped in safe helpers, rather than repeated at each of the
/// four lock/unlock sites below.
#[cfg(unix)]
mod file_lock {
    const LOCK_EX: i32 = 2; // exclusive lock
    const LOCK_UN: i32 = 8; // release lock

    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }

    /// Acquire an exclusive advisory lock on `fd`, blocking until held. Returns the raw
    /// `flock(2)` status (0 on success).
    pub(super) fn lock_exclusive(fd: i32) -> i32 {
        unsafe { flock(fd, LOCK_EX) }
    }

    /// Release the advisory lock on `fd`; best-effort (a failing unlock on teardown is ignored).
    pub(super) fn unlock(fd: i32) {
        let _ = unsafe { flock(fd, LOCK_UN) };
    }
}

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
        let status = file_lock::lock_exclusive(file.as_raw_fd());
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
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            // A control must crash for the RIGHT reason. The shared-RO-heap/cage crashes have a
            // known signature family; require it so a crash for an UNRELATED reason (a panic in
            // setup, an OOM) cannot read as "bug reproduced". `CAGE INVARIANT VIOLATED` is the
            // shipped first-installer guard (anchor::assert_cage_install_ordering): under the
            // pointer-compression cage it deterministically `process::abort`s (SIGABRT) the instant a
            // NodeFull superset snapshot would deserialize against a non-superset-first cage — i.e. it
            // IS the cross-profile cage crash, caught one frame before V8's own `ReadOnlyDeserializer`
            // abort. So it counts as a cage signature: the controls that race a WebStandard isolate in
            // first now abort via the guard (deterministic) instead of the racy V8_Fatal.
            let has_cage_signature = [
                "vector.h:415",
                "Hardening",
                "Unknown external reference",
                "DeserializeStringTable",
                "ReadReadOnlyHeapRef",
                "SharedHeapDeserializer",
                "CAGE INVARIANT VIOLATED",
                "Check failed: index < size()",
            ]
            .iter()
            .any(|sig| stdout.contains(sig) || stderr.contains(sig));
            // Cage crashes die by signal; which signal depends on BOTH the crash direction and
            // the OS. SIGABRT/SIGTRAP carry a message (vector.h:415 / "Unknown external
            // reference"), so require the cage signature; SIGBUS/SIGSEGV are the wrong-object
            // dereferences and abort SILENTLY, so the signal itself is the signature. SIGBUS is 7
            // on Linux but 10 on Darwin/BSD, and this lane runs on both (macOS dev plus the
            // ubuntu-24.04 `rust-runtime-ptrcomp-check` CI job), so resolve it per target instead
            // of hardcoding the macOS value (which would silently never match a Linux SIGBUS).
            const SIGTRAP: i32 = 5;
            const SIGABRT: i32 = 6;
            const SIGSEGV: i32 = 11;
            #[cfg(target_os = "linux")]
            const SIGBUS: i32 = 7;
            #[cfg(not(target_os = "linux"))]
            const SIGBUS: i32 = 10;
            let is_cage_crash = match output.status.signal() {
                Some(s) if s == SIGTRAP || s == SIGABRT => has_cage_signature,
                Some(s) if s == SIGBUS || s == SIGSEGV => true,
                _ => false,
            };
            if is_cage_crash {
                return;
            }
            last_observation = format!(
                "attempt {attempt}/{}: status {} (signal {:?}, cage_signature={has_cage_signature})\nstdout:\n{stdout}\nstderr:\n{stderr}",
                max_attempts.max(1),
                output.status,
                output.status.signal(),
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
        let path = std::env::temp_dir().join("nimbus-runtime-snapshot-reset-test.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .expect("snapshot reset test lockfile should open");
        let status = file_lock::lock_exclusive(file.as_raw_fd());
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
        file_lock::unlock(self.file.as_raw_fd());
    }
}

#[cfg(unix)]
impl Drop for RuntimeSuiteSubprocessLockGuard {
    fn drop(&mut self) {
        file_lock::unlock(self.file.as_raw_fd());
    }
}

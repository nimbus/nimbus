//! Shared blocking bridge from `?Send` `deno_fs::File`/`FileSystem` impls to
//! the `Send + Sync` byte plane (`nimbus_blob::BlobStore`).
//!
//! `deno_fs::File` and `deno_fs::FileSystem` methods are `?Send` — they run
//! synchronously on whatever thread is driving the isolate (a plain OS
//! thread, or a worker thread belonging to some *other* Tokio runtime, e.g.
//! `nimbus-server`'s). `BlobStore` methods are `async` and `Send`. Crossing
//! that boundary needs somewhere to poll the `BlobStore` future and a way to
//! hand its result back to a synchronous caller.
//!
//! The FCW2 audit found `cas_ro.rs` and `object/mod.rs` each building a brand
//! new `tokio::runtime::Runtime` — and, whenever already inside a runtime
//! context, spawning a brand new OS thread to host it — on *every* blocking
//! call. This module replaces both call sites with one lazily-initialized
//! runtime, shared for the process lifetime, reached through
//! [`block_on_byte_plane`].
//!
//! The bridge spawns the future onto the shared runtime and blocks the
//! calling thread on a plain [`std::sync::mpsc`] channel rather than Tokio's
//! own blocking-recv helpers (`Runtime::block_on`,
//! `oneshot::Receiver::blocking_recv`). Those helpers consult a thread-local
//! marker and panic ("Cannot block the current thread from within a
//! runtime") if the calling thread already has *any* Tokio runtime context
//! entered — which is exactly the case this bridge must also support (a
//! caller running on a worker thread of the server's own runtime).
//! `std::sync::mpsc::Receiver::recv` parks the OS thread directly; it does
//! not consult that marker, so the same call path is safe whether the
//! caller is a plain thread or a foreign runtime's worker thread. That is
//! why one code path suffices where the two per-call implementations each
//! needed an "already inside a runtime" branch.
//!
//! # Hazards
//!
//! - **Never call `block_on_byte_plane` from within a future already running
//!   on this bridge.** The shared runtime has a small, fixed worker pool
//!   (see [`shared_runtime`]); a bridged future that re-enters
//!   `block_on_byte_plane` competes with the very pool it is running on for
//!   a free worker. With enough concurrent re-entrant callers this exhausts
//!   the pool and deadlocks every caller, bridged or not.
//! - **A runtime-build failure is permanent for the process.** The runtime
//!   is built once behind a [`OnceLock`]; if `Builder::build()` fails on
//!   first use, every subsequent call reuses that same cached error rather
//!   than retrying the build.
//! - **There is deliberately no receive timeout.** If a spawned future never
//!   completes (a wedged byte-plane call), `rx.recv()` blocks the calling
//!   isolate thread forever — exactly as any other synchronous FS call
//!   would wedge that thread. Callers get no special leniency here; treat a
//!   hang the same way you would a hung syscall.

use std::future::Future;
use std::io;
use std::sync::OnceLock;

use deno_io::fs::FsResult;
use tokio::runtime::Runtime;

/// Returns the shared byte-plane bridge runtime, building it on first use.
///
/// This must be a multi-threaded runtime (even with a single worker thread):
/// a `current_thread` runtime only polls spawned tasks while something calls
/// `block_on` *on that same runtime*, which nothing here ever does — we only
/// `spawn` onto it and block the caller on a channel instead. A `spawn`-only
/// `current_thread` runtime would queue the task and never run it, deadlocking
/// every caller. `new_multi_thread` dedicates real worker threads that drive
/// spawned tasks on their own, independent of any `block_on` call.
///
/// The pool has more than one worker (`available_parallelism`, capped at 4)
/// so concurrent byte-plane calls from different isolates make progress in
/// parallel instead of serializing behind a single worker thread. The cap is
/// deliberately small: every task this runtime ever drives is I/O-wait bound
/// (a `BlobStore` call), not CPU-bound, so a handful of workers is enough to
/// keep them all in flight without growing the pool unbounded per process.
fn shared_runtime() -> Result<&'static Runtime, io::Error> {
    static RUNTIME: OnceLock<io::Result<Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            let worker_threads = std::thread::available_parallelism()
                .map(|n| n.get().min(4))
                .unwrap_or(1);
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(worker_threads)
                .enable_all()
                .build()
        })
        .as_ref()
        .map_err(|error| io::Error::other(format!("build byte-plane bridge runtime: {error}")))
}

/// Runs `future` to completion on the shared byte-plane bridge runtime,
/// blocking the calling thread until it finishes.
///
/// Safe to call both from a plain thread and from a worker thread of some
/// other Tokio runtime — see the module docs for why.
pub(crate) fn block_on_byte_plane<T, F>(future: F) -> FsResult<T>
where
    T: Send + 'static,
    F: Future<Output = FsResult<T>> + Send + 'static,
{
    let runtime = shared_runtime()?;
    let (tx, rx) = std::sync::mpsc::channel();
    runtime.spawn(async move {
        // The receiver may already be gone (caller dropped mid-flight); a
        // failed send just means nobody is waiting for the result anymore.
        let _ = tx.send(future.await);
    });
    match rx.recv() {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("byte-plane bridge task panicked before completing").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_future_and_returns_its_result() {
        let result = block_on_byte_plane(async { Ok(21 * 2) });
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn propagates_future_errors() {
        let result: FsResult<()> = block_on_byte_plane(async {
            Err(io::Error::new(io::ErrorKind::NotFound, "missing").into())
        });
        let error = result.unwrap_err();
        assert!(error.to_string().contains("missing"));
    }

    #[test]
    fn survives_repeated_calls_reusing_the_same_shared_runtime() {
        for i in 0..8 {
            let result = block_on_byte_plane(async move { Ok(i) });
            assert_eq!(result.unwrap(), i);
        }
    }

    #[test]
    fn concurrent_byte_plane_calls_make_progress() {
        // Eight OS threads each drive a byte-plane call that sleeps ~50ms
        // before returning its id. A single-worker bridge runtime would
        // serialize these behind one worker (~400ms total); a small pool
        // lets them overlap. The bound is generous to stay CI-safe while
        // still proving parallelism, not just eventual completion.
        const CALLERS: usize = 8;
        const SLEEP: std::time::Duration = std::time::Duration::from_millis(50);

        let start = std::time::Instant::now();
        let handles: Vec<_> = (0..CALLERS)
            .map(|id| {
                std::thread::spawn(move || {
                    block_on_byte_plane(async move {
                        tokio::time::sleep(SLEEP).await;
                        Ok(id)
                    })
                    .unwrap()
                })
            })
            .collect();

        let mut ids: Vec<usize> = handles
            .into_iter()
            .map(|handle| handle.join().expect("caller thread must not panic"))
            .collect();
        ids.sort_unstable();

        assert_eq!(ids, (0..CALLERS).collect::<Vec<_>>());
        assert!(
            start.elapsed() < std::time::Duration::from_millis(350),
            "concurrent byte-plane calls took {:?}, expected well under {CALLERS} x {SLEEP:?} \
             serialized",
            start.elapsed()
        );
    }
}

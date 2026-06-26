//! Process-lifetime NodeFull RO-heap ANCHOR (Option A crash fix).
//!
//! The single pointer-compression cage installs ONE shared read-only heap from the FIRST
//! isolate to deserialize. NodeFull's RO heap is a SUPERSET of WebStandard's: a NodeFull
//! snapshot deserialized against a smaller-installed RO heap OOBs (`vector.h:415`), and a
//! WebStandard snapshot against NodeFull's RO heap SIGBUSes (wrong object). Option A keeps
//! only NodeFull snapshotted; WebStandard is built unsnapshotted (`create_runtime(None)`) +
//! code-cache, so it never deserializes against the shared RO heap.
//!
//! For that to be safe, NodeFull MUST install the cage RO heap FIRST. This module GUARANTEES
//! it (force NodeFull-first, not "crash on WebStandard-first"): it builds one NodeFull isolate
//! at process init and PINS it — isolate AND its OS thread — for the process lifetime.
//!
//! WHY PINNED, NOT DISPOSE-AFTER-INSTALL (proven, not assumed): the cage RO-heap install does
//! NOT survive the installing thread's exit. A naive same-thread test
//! (`anchor_ro_heap_persists_past_isolate_disposal_same_thread`) shows the heap surviving mere
//! ISOLATE disposal and is MISLEADING; the decider
//! (`disposed_anchor_thread_exit_makes_crash_return`) builds NodeFull on a thread that then
//! EXITS and shows the cross-profile crash RETURN. So the anchor must keep its isolate + thread
//! resident; being first is necessary but not sufficient. A fail-closed floor then asserts that
//! no isolate is ever built before the anchor is installed — a regression catch for a future
//! init reorder, which should never fire in correct operation because install blocks before the
//! pool serves.
//!
//! COUPLING NOTE (for a future maintainer): this anchor exists *because* both profiles share
//! ONE process's V8 cage (the single-process model). The process-global OnceLock/atomics are
//! correct only under that model. If nimbus ever moves to process-per-profile (sandbox option
//! B — each profile its own OS process/cage), this mechanism becomes UNNECESSARY and should be
//! REMOVED: a WebStandard-only process has no NodeFull to anchor and no cross-profile RO
//! conflict. It is NOT a load-bearing invariant to preserve across that refactor — it is the
//! cost of sharing a cage, and it goes away when the cage is no longer shared.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::host::{HostBridge, HostCallRequest};
use crate::limits::{RuntimeLimits, RuntimePolicy};
use crate::runtime::{NimbusRuntime, RuntimeBundle};

/// HostBridge for the anchor isolate. The anchor is BUILT (to install the cage RO heap) but
/// never INVOKED — construction makes no host calls, proven by the
/// `anchor_nodefull_build_host_call_count` regression test (count == 0). This bridge must
/// therefore never be called; it fails LOUD so a future construction-time host call is caught
/// instantly rather than silently masked behind a benign `Null`.
#[derive(Debug)]
struct AnchorNoopHost;

impl HostBridge for AnchorNoopHost {
    fn call(&self, _request: HostCallRequest) -> crate::error::Result<serde_json::Value> {
        panic!(
            "anchor HostBridge was invoked, but the NodeFull anchor must only CONSTRUCT an \
             isolate (never run JS or make host calls). A host call here means construction \
             changed — re-validate the anchor's host-call-free assumption before relying on it."
        );
    }
}

static ANCHOR_ENABLED: AtomicBool = AtomicBool::new(false);
static ANCHOR_INSTALLED: AtomicBool = AtomicBool::new(false);
static ANCHOR: OnceLock<()> = OnceLock::new();

thread_local! {
    static IN_ANCHOR_BUILD: Cell<bool> = const { Cell::new(false) };
}

/// Install the NodeFull RO-heap anchor. Idempotent. Call ONCE at process init, BEFORE the
/// pool serves any request, so the cage's shared RO heap is NodeFull's superset — making a
/// WebStandard-first install structurally unreachable. Spawns the anchor isolate on a thread
/// that PARKS for the process lifetime (the install does not survive that thread's exit), and
/// BLOCKS until the RO heap is installed.
pub(crate) fn install_nodefull_anchor(host: Arc<dyn HostBridge>) {
    ANCHOR.get_or_init(|| {
        // Arm the floor BEFORE the anchor build so a racing non-anchor build would be
        // caught; the anchor build itself is exempt via IN_ANCHOR_BUILD.
        ANCHOR_ENABLED.store(true, Ordering::SeqCst);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        std::thread::Builder::new()
            .name("nimbus-nodefull-anchor".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("anchor tokio runtime should build");
                rt.block_on(async move {
                    IN_ANCHOR_BUILD.with(|c| c.set(true));
                    let owner = NimbusRuntime::with_policy(
                        host,
                        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
                    );
                    let bundle = RuntimeBundle::virtual_anchor();
                    let snap = owner
                        .bootstrap_snapshot()
                        .expect("nodefull anchor snapshot should build");
                    let _anchor = owner
                        .create_runtime_from_snapshot(&bundle, snap)
                        .expect("nodefull anchor should install the superset RO heap");
                    ANCHOR_INSTALLED.store(true, Ordering::SeqCst);
                    let _ = ready_tx.send(());
                    // PIN for the process lifetime. The cage RO-heap install does NOT survive
                    // the installing thread's exit (proven by
                    // `disposed_anchor_thread_exit_makes_crash_return`), so keep `_anchor`
                    // alive and this thread parked forever. The anchor is NOT a warm-pool
                    // entry, so eviction never reaches it.
                    loop {
                        std::thread::park();
                    }
                });
            })
            .expect("anchor thread should spawn");
        ready_rx
            .recv()
            .expect("anchor should install before serving");
    });
}

/// Production entry: arm the NodeFull RO-heap anchor with an internal fail-loud host. Call at
/// process/worker startup (e.g. from `V8RuntimeBackend::create`) BEFORE the pool exists or
/// serves. Blocks until the anchor's superset RO heap is installed, so pool fill / serving
/// cannot race the install.
pub(crate) fn enable_and_arm_nodefull_anchor() {
    install_nodefull_anchor(Arc::new(AnchorNoopHost));
}

/// Fail-closed FLOOR (a regression assertion, NOT the live path). Every isolate build must
/// occur after the anchor is installed. In correct operation this never fires, because
/// `install_nodefull_anchor` runs (and blocks) at process init before any isolate creation.
/// Armed only once the anchor system is in use (`ANCHOR_ENABLED`), so raw/crash-repro
/// constructions that intentionally run without an anchor are unaffected.
pub(crate) fn assert_anchor_floor() {
    if IN_ANCHOR_BUILD.with(|c| c.get()) {
        return; // the anchor build itself IS the NodeFull-first install
    }
    if ANCHOR_ENABLED.load(Ordering::SeqCst) && !ANCHOR_INSTALLED.load(Ordering::SeqCst) {
        panic!(
            "ANCHOR INVARIANT VIOLATED: an isolate was built before the NodeFull RO-heap \
             anchor finished installing. Reorder process init so install_nodefull_anchor() \
             completes before any isolate creation (guards the cross-profile RO-heap crash)."
        );
    }
}

#[cfg(test)]
pub(crate) fn anchor_installed_for_test() -> bool {
    ANCHOR_INSTALLED.load(Ordering::SeqCst)
}

/// Test-only: arm the floor (ANCHOR_ENABLED) WITHOUT installing the anchor, to prove the
/// fail-closed assertion actually fires on a non-anchor build. Process-global; use only in
/// a process-isolated (`--exact` / subprocess) test.
#[cfg(test)]
pub(crate) fn arm_floor_without_install_for_test() {
    ANCHOR_ENABLED.store(true, Ordering::SeqCst);
}

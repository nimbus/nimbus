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
#[cfg(feature = "v8-pointer-compression")]
use std::sync::atomic::AtomicU8;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::host::{HostBridge, HostCallRequest};
use crate::limits::{RuntimeCompatibilityTarget, RuntimeLimits, RuntimePolicy};
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

/// Kind of the FIRST shared-cage read-only-heap installer this process: `0` = none yet, `1` =
/// superset (a NodeFull snapshot), `2` = non-superset (an unsnapshotted / WebStandard profile). Only
/// meaningful under the pointer-compression shared cage.
#[cfg(feature = "v8-pointer-compression")]
static FIRST_CAGE_INSTALLER_KIND: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static IN_ANCHOR_BUILD: Cell<bool> = const { Cell::new(false) };
}

/// SHIPPED cage invariant guard (Option A), active under `v8-pointer-compression` (the single shared
/// cage; the config release ships). The FIRST isolate to construct installs the cage's shared
/// read-only heap, fixing its layout for every later isolate. A NodeFull *snapshot* installs the
/// SUPERSET RO heap (`installs_superset == true`); a WebStandard / unsnapshotted profile installs a
/// non-superset one. The cross-profile crash is precisely: a NodeFull superset snapshot deserialized
/// AFTER a non-superset isolate already fixed the cage — V8 aborts inside `ReadOnlyDeserializer`
/// (`Check failed: magic_number_ == SerializedData::kMagicNumber`), a rare, timing-dependent, SILENT
/// `V8_Fatal`. This converts that into a DETERMINISTIC, LOUD `process::abort` at the exact construction
/// that would crash, and — unlike `assert_anchor_floor` (gated on `ANCHOR_ENABLED`) — it covers the window
/// BEFORE the anchor is armed. A pure-unsnapshotted process (no NodeFull snapshot ever) never trips
/// it, and a deliberately-snapshotted control isolate installing FIRST is allowed (the legacy crash
/// the oracle reproduces is unaffected). MUST be called UNDER the shared-RO-heap serialize lock so
/// the recorded first installer matches the actual install order.
pub(crate) fn assert_cage_install_ordering(
    installs_superset: bool,
    target: RuntimeCompatibilityTarget,
) {
    #[cfg(feature = "v8-pointer-compression")]
    {
        let kind: u8 = if installs_superset { 1 } else { 2 };
        // Record this as the first installer iff none recorded yet; otherwise keep the first's kind.
        let _ =
            FIRST_CAGE_INSTALLER_KIND.compare_exchange(0, kind, Ordering::SeqCst, Ordering::SeqCst);
        if installs_superset && FIRST_CAGE_INSTALLER_KIND.load(Ordering::SeqCst) == 2 {
            eprintln!(
                "CAGE INVARIANT VIOLATED: a NodeFull superset snapshot (target={target:?}) is being \
                 deserialized into the pointer-compression shared cage, but the cage's read-only heap \
                 was FIRST installed by a non-superset (unsnapshotted / WebStandard) isolate. V8 would \
                 abort in ReadOnlyDeserializer (the cross-profile magic crash). The NodeFull anchor \
                 must install the superset RO heap FIRST. This deterministic guard caught the ordering \
                 violation that would otherwise be a rare, silent V8_Fatal."
            );
            // ABORT (SIGABRT), not panic: this breach immediately precedes an unrecoverable V8_Fatal,
            // so fail LOUD and UNMASKABLE — no `catch_unwind` can swallow it, and it matches the V8
            // crash signal class the cage crash-oracle controls assert on.
            std::process::abort();
        }
    }
    #[cfg(not(feature = "v8-pointer-compression"))]
    let _ = (installs_superset, target);
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
                    // Test-only: widen the ANCHOR_ENABLED..ANCHOR_INSTALLED window so a test
                    // can deterministically prove the arming path blocks until install.
                    #[cfg(test)]
                    {
                        let ms = test_hooks::ANCHOR_INSTALL_DELAY_MS.load(Ordering::SeqCst);
                        if ms > 0 {
                            std::thread::sleep(std::time::Duration::from_millis(ms));
                        }
                    }
                    let owner = NimbusRuntime::with_policy(
                        host,
                        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
                        crate::RuntimeEgressPosture::CoarsePermissions,
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
        ready_rx.recv().expect(
            "NodeFull RO-heap anchor build FAILED: the anchor thread panicked before signalling \
             ready and dropped the channel, so the superset RO heap was never installed. The \
             process is intentionally wedged here rather than serving — building any isolate now \
             would risk the cross-profile cage crash this anchor prevents. This is fail-closed, \
             NOT a deadlock; the anchor thread's panic is logged above with the root cause.",
        );
    });
}

/// Production entry: arm the NodeFull RO-heap anchor with an internal fail-loud host. Call at
/// process/worker startup (the production caller is `V8RuntimeBackendFactory::create` in
/// `backends/v8/mod.rs`) BEFORE the pool exists or
/// serves. Blocks until the anchor's superset RO heap is installed, so pool fill / serving
/// cannot race the install.
pub(crate) fn enable_and_arm_nodefull_anchor() {
    install_nodefull_anchor(Arc::new(AnchorNoopHost));
}

/// VERIFICATION ENTRY POINT (not a diagnostic): force-install the COMMITTED embedded anchor snapshot
/// (the cfg-selected `.bin` feature-off / `.pc.bin` feature-on) and construct a NodeFull isolate from
/// it on the current thread. This is the ONLY way to exercise the EMBEDDED anchor install under the
/// pointer-compression cage in a test — the cfg(test) cage oracle cannot, because its snapshot carries
/// a test-only extension, so its provenance mismatches the committed production blob and it falls back
/// to a runtime build. Called from the `tests/embedded_anchor.rs` integration test, which links
/// NON-cfg(test) nimbus-runtime, so `try_embedded` computes the production provenance, matches the
/// committed blob, and the anchor installs FROM it (the serving path). Returns `Err` if the blob fails
/// provenance/parse; a V8 read-only-heap incompatibility would abort (V8_Fatal) — the very failure the
/// embedding must not have. Under the cage, this isolate is the first installer (NodeFull superset),
/// so `assert_cage_install_ordering` records it and stays silent.
pub fn smoke_install_committed_embedded_anchor() -> std::result::Result<(), String> {
    let snapshot = crate::backends::v8::try_embedded_node22_anchor_snapshot(
        crate::backends::v8::EMBEDDED_NODE22_ANCHOR_SNAPSHOT,
    )
    .ok_or_else(|| {
        "embedded blob failed provenance/parse (try_embedded returned None)".to_string()
    })?;
    let owner = NimbusRuntime::with_policy(
        Arc::new(AnchorNoopHost),
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let bundle = RuntimeBundle::virtual_anchor();
    owner
        .create_runtime_from_snapshot(&bundle, &snapshot)
        .map_err(|error| format!("create_runtime_from_snapshot failed: {error}"))?;
    Ok(())
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

// The test-only seams are re-exported so call sites still read as `anchor::<hook>`; the `use`
// precedes the module so `#[cfg(test)] mod test_hooks` stays the file's last item (satisfies
// clippy::items_after_test_module).
#[cfg(test)]
pub(crate) use test_hooks::{
    anchor_enabled_for_test, anchor_installed_for_test, arm_floor_without_install_for_test,
    set_anchor_install_delay_ms_for_test,
};

/// Test-only seams into the process-global anchor state. The anchor's correctness rests on
/// process-global `OnceLock`/atomics that cannot be dependency-injected, so the tests that prove
/// its install protocol (blocks-until-installed, floor-fires, dormant-pre-arm) need a few hooks
/// into that state. They are grouped here, compile out of production entirely, and re-exported
/// above so call sites still read as `anchor::<hook>`.
#[cfg(test)]
mod test_hooks {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{ANCHOR_ENABLED, ANCHOR_INSTALLED};

    /// Artificial delay inserted into the anchor build (after `ANCHOR_ENABLED`, before
    /// `ANCHOR_INSTALLED`) so a test can WIDEN the install window deterministically and prove the
    /// production arming path blocks until install completes. Read by `install_nodefull_anchor`.
    pub(super) static ANCHOR_INSTALL_DELAY_MS: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn set_anchor_install_delay_ms_for_test(ms: u64) {
        ANCHOR_INSTALL_DELAY_MS.store(ms, Ordering::SeqCst);
    }

    pub(crate) fn anchor_enabled_for_test() -> bool {
        ANCHOR_ENABLED.load(Ordering::SeqCst)
    }

    pub(crate) fn anchor_installed_for_test() -> bool {
        ANCHOR_INSTALLED.load(Ordering::SeqCst)
    }

    /// Arm the floor (`ANCHOR_ENABLED`) WITHOUT installing the anchor, to prove the fail-closed
    /// assertion actually fires on a non-anchor build. Process-global; use only in a
    /// process-isolated (`--exact` / subprocess) test.
    pub(crate) fn arm_floor_without_install_for_test() {
        ANCHOR_ENABLED.store(true, Ordering::SeqCst);
    }
}

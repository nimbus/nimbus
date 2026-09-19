//! Near-heap-limit classification for V8 isolates.
//!
//! V8 calls the near-heap-limit callback for two different causes:
//!
//! 1. The JS heap reaches its limit: `Heap::CheckHeapLimitReached`, repeated
//!    ineffective mark-compacts near the limit, or the last-resort GC after a
//!    failed JS heap allocation.
//! 2. An ArrayBuffer backing-store allocation fails. V8 then runs a
//!    last-resort GC (`Heap::AllocateExternalBackingStore` ->
//!    `HeapAllocator::RetryCustomAllocate` ->
//!    `Heap::CollectAllAvailableGarbage(kLastResort)`), and that GC calls the
//!    callback before the final retry, whatever the heap usage is.
//!
//! Node.js registers no near-heap-limit callback by default, so for cause 2 it
//! throws `RangeError: Array buffer allocation failed` and continues. The
//! nimbus ArrayBuffer allocator records cause 2, so that the invocation
//! terminates only for cause 1 and a failed allocation stays a catchable
//! `RangeError`.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use super::embedder::v8;

/// No backing-store allocation waits for its last-resort GC.
const IDLE: u8 = 0;
/// A backing-store allocation failed, and V8 has not yet run the last-resort
/// GC for it.
const FAILED: u8 = 1;
/// The callback classified the last-resort GC. The next allocation is the
/// final retry of V8 for the same request.
const FINAL_RETRY: u8 = 2;

/// V8 treats the JS heap as near its limit at `--ineffective_gc_size_threshold`
/// (0.95) of the old-generation limit. The classifier uses the same ratio.
const NEAR_LIMIT_NUMERATOR: usize = 19;
const NEAR_LIMIT_DENOMINATOR: usize = 20;

/// Why V8 called the near-heap-limit callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NearHeapLimitCause {
    /// The JS heap is at, or near, its limit. The invocation must terminate.
    JsHeapLimit,
    /// A backing-store allocation failed while the JS heap is well below its
    /// limit. V8 fails the allocation with a `RangeError`.
    FailedBackingStoreAllocation,
}

/// Records failed ArrayBuffer backing-store allocations of one isolate.
///
/// The ArrayBuffer allocator writes it and the near-heap-limit callback reads
/// it. Both run on the isolate thread, but the allocator must be `Send + Sync`.
#[derive(Clone, Debug, Default)]
pub(crate) struct BackingStoreAllocationState(Arc<AtomicU8>);

impl BackingStoreAllocationState {
    #[cfg(test)]
    fn record_allocation(&self, allocated: bool) {
        record_allocation(&self.0, allocated);
    }

    /// Classifies one near-heap-limit callback from the recorded allocation
    /// state and the committed heap size.
    fn classify(&self, committed_heap_bytes: usize, current_limit: usize) -> NearHeapLimitCause {
        if self.0.load(Ordering::SeqCst) != FAILED
            || committed_heap_bytes >= near_limit_bytes(current_limit)
        {
            return NearHeapLimitCause::JsHeapLimit;
        }
        match self
            .0
            .compare_exchange(FAILED, FINAL_RETRY, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => NearHeapLimitCause::FailedBackingStoreAllocation,
            Err(_) => NearHeapLimitCause::JsHeapLimit,
        }
    }

    /// Returns an ArrayBuffer allocator with the semantics of the V8 default
    /// allocator that records its results in this state.
    pub(crate) fn array_buffer_allocator(&self) -> v8::UniqueRef<v8::Allocator> {
        static VTABLE: v8::RustAllocatorVtable<AtomicU8> = v8::RustAllocatorVtable {
            allocate,
            allocate_uninitialized,
            free,
            drop: drop_state,
        };
        let handle = Arc::into_raw(self.0.clone());
        // SAFETY: `handle` comes from `Arc::into_raw` and `drop_state` releases
        // it once, when V8 destroys the allocator.
        unsafe { v8::new_rust_allocator(handle, &VTABLE) }
    }
}

fn near_limit_bytes(current_limit: usize) -> usize {
    current_limit / NEAR_LIMIT_DENOMINATOR * NEAR_LIMIT_NUMERATOR
}

fn record_allocation(state: &AtomicU8, allocated: bool) {
    if allocated {
        state.store(IDLE, Ordering::SeqCst);
    } else if state
        .compare_exchange(FINAL_RETRY, IDLE, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        state.store(FAILED, Ordering::SeqCst);
    }
}

fn record(state: &AtomicU8, data: *mut c_void) -> *mut c_void {
    record_allocation(state, !data.is_null());
    data
}

// The V8 default allocator (`v8::ArrayBuffer::Allocator::NewDefaultAllocator`
// without the sandbox) uses `calloc`, `malloc` and `free`. These functions keep
// the same semantics.
unsafe extern "C" fn allocate(state: &AtomicU8, len: usize) -> *mut c_void {
    // SAFETY: `calloc` accepts any length and returns null on failure.
    record(state, unsafe { libc::calloc(len, 1) })
}

unsafe extern "C" fn allocate_uninitialized(state: &AtomicU8, len: usize) -> *mut c_void {
    // SAFETY: `malloc` accepts any length and returns null on failure.
    record(state, unsafe { libc::malloc(len) })
}

unsafe extern "C" fn free(_state: &AtomicU8, data: *mut c_void, _len: usize) {
    // SAFETY: V8 frees only pointers that `allocate` or `allocate_uninitialized`
    // returned.
    unsafe { libc::free(data) }
}

unsafe extern "C" fn drop_state(state: *const AtomicU8) {
    // SAFETY: `state` is the pointer from `Arc::into_raw` in
    // `array_buffer_allocator`, and V8 calls `drop` once.
    drop(unsafe { Arc::from_raw(state) });
}

/// Classifies near-heap-limit callbacks for one isolate.
pub(crate) struct NearHeapLimitClassifier {
    state: BackingStoreAllocationState,
    isolate: v8::UnsafeRawIsolatePtr,
}

impl NearHeapLimitClassifier {
    /// Returns the classifier for `runtime`. A runtime that has no recorded
    /// allocator state classifies every callback as `JsHeapLimit`.
    pub(crate) fn for_runtime(runtime: &mut super::embedder::JsRuntime) -> Self {
        let state = runtime
            .op_state()
            .borrow()
            .try_borrow::<BackingStoreAllocationState>()
            .cloned()
            .unwrap_or_default();
        // SAFETY: the classifier lives in the near-heap-limit callback of this
        // isolate, and V8 calls that callback only while the isolate is alive.
        let isolate = unsafe { runtime.v8_isolate().as_raw_isolate_ptr() };
        Self { state, isolate }
    }

    /// Classifies one near-heap-limit callback. V8 calls it on the isolate
    /// thread.
    pub(crate) fn classify(&self, current_limit: usize) -> NearHeapLimitCause {
        // SAFETY: see `for_runtime`. `v8::Isolate` does not own the isolate, and
        // V8 permits heap statistics reads in the near-heap-limit callback.
        let mut isolate = unsafe { v8::Isolate::from_raw_isolate_ptr(self.isolate) };
        let committed_heap_bytes = isolate.get_heap_statistics().total_heap_size();
        self.state.classify(committed_heap_bytes, current_limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 128 << 20;

    fn state_after(results: &[bool]) -> BackingStoreAllocationState {
        let state = BackingStoreAllocationState::default();
        for allocated in results {
            state.record_allocation(*allocated);
        }
        state
    }

    #[test]
    fn a_callback_without_a_failed_allocation_is_a_js_heap_limit() {
        assert_eq!(
            state_after(&[]).classify(0, LIMIT),
            NearHeapLimitCause::JsHeapLimit
        );
        assert_eq!(
            state_after(&[false, true]).classify(0, LIMIT),
            NearHeapLimitCause::JsHeapLimit
        );
    }

    #[test]
    fn a_failed_allocation_below_the_near_limit_ratio_is_not_a_js_heap_limit() {
        let state = state_after(&[false, false]);
        assert_eq!(
            state.classify(near_limit_bytes(LIMIT) - 1, LIMIT),
            NearHeapLimitCause::FailedBackingStoreAllocation
        );
    }

    #[test]
    fn a_failed_allocation_near_the_limit_is_a_js_heap_limit() {
        let state = state_after(&[false]);
        assert_eq!(
            state.classify(near_limit_bytes(LIMIT), LIMIT),
            NearHeapLimitCause::JsHeapLimit
        );
    }

    #[test]
    fn the_final_retry_failure_leaves_no_stale_record() {
        let state = state_after(&[false]);
        assert_eq!(
            state.classify(0, LIMIT),
            NearHeapLimitCause::FailedBackingStoreAllocation
        );
        // V8 retries once after the last-resort GC. The retry fails too.
        state.record_allocation(false);
        assert_eq!(state.classify(0, LIMIT), NearHeapLimitCause::JsHeapLimit);
    }

    #[test]
    fn one_failed_allocation_skips_termination_once() {
        let state = state_after(&[false]);
        assert_eq!(
            state.classify(0, LIMIT),
            NearHeapLimitCause::FailedBackingStoreAllocation
        );
        assert_eq!(state.classify(0, LIMIT), NearHeapLimitCause::JsHeapLimit);
    }

    #[test]
    fn the_allocator_records_failed_and_successful_allocations() {
        let state = BackingStoreAllocationState::default();
        let data = record(&state.0, std::ptr::null_mut());
        assert!(data.is_null());
        assert_eq!(state.0.load(Ordering::SeqCst), FAILED);

        // SAFETY: the test allocates and frees one byte through the vtable
        // functions.
        unsafe {
            let data = allocate(&state.0, 1);
            assert!(!data.is_null());
            assert_eq!(*data.cast::<u8>(), 0);
            free(&state.0, data, 1);
        }
        assert_eq!(state.0.load(Ordering::SeqCst), IDLE);
    }
}

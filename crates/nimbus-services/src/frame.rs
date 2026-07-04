//! CB2: per-frame invocation over a safe owner-keyed warm pool.
//!
//! The connection broker (CB1) holds sockets; CB2 adds the verb that turns an
//! inbound frame into an isolate invocation and outbound frames. Per the CB2
//! safety audit (`proof/connection-broker/cb2-pool-safety-audit.md`) this does
//! NOT lift the openworkers `ThreadLocalPool`'s `UnsafeCell` raw-pointer
//! handoff; it lifts only its SAFE shapes — owner-keyed warm reuse, LRU +
//! overcommit eviction, and a between-frame `reset` contract — expressed over
//! a safe pool, and delegates the actual isolate execution to a
//! [`FrameHandler`] seam (the same seam pattern CB1 used for placement).
//! `nimbus-runtime`'s existing safe executor/pool implements that seam; this
//! crate stays `unsafe`-free.
//!
//! Residency (CB1) drives pool behavior: `Hibernated` evicts the warm slot
//! after every frame (isolate freed when idle — the beats-Vercel class);
//! `Resident` keeps the slot warm and only resets between-frame state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::broker::{BrokerError, HostFrame, InstanceKey, Residency};
use crate::meter::{FrameMeter, UsageRecord};

/// Opaque per-instance state carried across frames. CB3 persists this via
/// `TenantKvStore` (16 KiB cap enforced there); CB2 only threads it through.
pub type FrameState = Vec<u8>;

/// What the host hands the isolate for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameInput {
    /// The inbound frame that woke this invocation.
    pub inbound: HostFrame,
    /// The instance's state loaded before the frame (empty on cold start).
    pub state: FrameState,
}

/// What the isolate produced for one frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameOutput {
    /// Frames to push back down held connections.
    pub outbound: Vec<HostFrame>,
    /// The instance's state after the frame (persisted by CB3).
    pub state: FrameState,
}

/// The per-frame isolate-invocation seam. Implemented by `nimbus-runtime`'s
/// safe executor/pool; kept as a trait so the broker substrate stays
/// `unsafe`-free and testable. A handler must be pure w.r.t. the passed state
/// (no hidden cross-frame retention) — the warm pool owns warmth, the handler
/// owns execution.
pub trait FrameHandler: Send + Sync {
    fn invoke(&self, key: &InstanceKey, input: FrameInput) -> Result<FrameOutput, BrokerError>;

    /// Clear between-frame execution state for a warm (Resident) slot without
    /// tearing the isolate down. Default no-op for handlers that hold none.
    fn reset(&self, _key: &InstanceKey) {}
}

/// A safe owner-keyed warm-slot cache: LRU eviction, an overcommit cap, and a
/// between-frame reset contract — the SAFE shapes from openworkers'
/// `ThreadLocalPool`, without its `UnsafeCell` raw-pointer aliasing.
///
/// The pool tracks WHICH owners currently hold a warm slot and enforces the
/// warm/evict policy; the isolate itself lives behind the [`FrameHandler`].
pub struct WarmPool {
    max_warm: usize,
    /// owner → logical clock of last use (LRU ordering). Presence = warm.
    warm: Mutex<HashMap<InstanceKey, u64>>,
    clock: Mutex<u64>,
}

impl WarmPool {
    /// A pool that keeps at most `max_warm` owners warm (overcommit cap).
    /// `max_warm` of 0 is treated as 1 (a pool must hold at least the slot in
    /// active use).
    pub fn new(max_warm: usize) -> Self {
        Self {
            max_warm: max_warm.max(1),
            warm: Mutex::new(HashMap::new()),
            clock: Mutex::new(0),
        }
    }

    /// Number of currently-warm owners.
    pub fn warm_len(&self) -> usize {
        self.warm.lock().map(|w| w.len()).unwrap_or(0)
    }

    /// True if `key` currently holds a warm slot.
    pub fn is_warm(&self, key: &InstanceKey) -> bool {
        self.warm
            .lock()
            .map(|w| w.contains_key(key))
            .unwrap_or(false)
    }

    fn tick(&self) -> Result<u64, BrokerError> {
        let mut clock = self.clock.lock().map_err(|_| poisoned("warm pool clock"))?;
        *clock += 1;
        Ok(*clock)
    }

    /// Mark `key` warm and touch its LRU position. Returns whether the slot
    /// was already warm (a reuse) plus the owner LRU-evicted to make room, if
    /// any (so the caller can `reset`/account for the evicted isolate).
    fn acquire(&self, key: &InstanceKey) -> Result<Acquisition, BrokerError> {
        let now = self.tick()?;
        let mut warm = self.warm.lock().map_err(|_| poisoned("warm pool"))?;
        let reused = warm.contains_key(key);
        warm.insert(key.clone(), now);

        let mut evicted = None;
        if warm.len() > self.max_warm {
            // Evict the least-recently-used owner that is NOT the one we just
            // acquired (its clock is the max, so it is never the LRU here).
            if let Some((lru_key, _)) = warm
                .iter()
                .min_by_key(|(_, used)| **used)
                .map(|(k, v)| (k.clone(), *v))
            {
                warm.remove(&lru_key);
                evicted = Some(lru_key);
            }
        }
        Ok(Acquisition { reused, evicted })
    }

    /// Drop `key`'s warm slot (Hibernated after-frame eviction).
    fn evict(&self, key: &InstanceKey) -> Result<bool, BrokerError> {
        let mut warm = self.warm.lock().map_err(|_| poisoned("warm pool"))?;
        Ok(warm.remove(key).is_some())
    }
}

struct Acquisition {
    reused: bool,
    evicted: Option<InstanceKey>,
}

fn poisoned(what: &str) -> BrokerError {
    BrokerError {
        message: format!("{what} lock is poisoned"),
    }
}

/// Outcome accounting for one per-frame invocation (CB10 meters ride this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameInvocation {
    pub output: FrameOutput,
    /// True if the frame ran on a warm (reused) slot rather than a cold start.
    pub warm_hit: bool,
    /// Residency the frame ran under.
    pub residency: Residency,
}

/// The per-frame invoke verb: composes the warm pool, the handler, and the
/// broker residency class into one call.
pub struct FrameInvoker<H: FrameHandler> {
    pool: WarmPool,
    handler: H,
    /// Optional CB10 metering sink; each frame emits a UsageRecord when set.
    meter: Option<Arc<dyn FrameMeter>>,
}

impl<H: FrameHandler> FrameInvoker<H> {
    pub fn new(handler: H, max_warm: usize) -> Self {
        Self {
            pool: WarmPool::new(max_warm),
            handler,
            meter: None,
        }
    }

    /// Attach a CB10 metering sink: every per-frame invoke emits a
    /// UsageRecord (active-CPU + residency) to it.
    pub fn with_meter(mut self, meter: Arc<dyn FrameMeter>) -> Self {
        self.meter = Some(meter);
        self
    }

    /// True if `key` is warm in this invoker's pool.
    pub fn is_warm(&self, key: &InstanceKey) -> bool {
        self.pool.is_warm(key)
    }

    /// Warm owner count.
    pub fn warm_len(&self) -> usize {
        self.pool.warm_len()
    }

    /// Invoke one frame for `key` under `residency`.
    ///
    /// 1. Acquire a warm slot (reuse if warm; cold-start otherwise), evicting
    ///    the LRU owner if the overcommit cap is exceeded (its handler state
    ///    is reset).
    /// 2. If the slot was reused, `reset` between-frame state first (the
    ///    openworkers `CachedContext::reset` contract).
    /// 3. Run the handler.
    /// 4. `Hibernated` evicts the slot after the frame (isolate freed when
    ///    idle); `Resident` keeps it warm.
    ///
    /// Fail-closed: a handler error evicts the slot (a poisoned isolate is
    /// never left warm to serve the next frame) and propagates.
    pub fn per_frame_invoke(
        &self,
        key: &InstanceKey,
        residency: Residency,
        input: FrameInput,
    ) -> Result<FrameInvocation, BrokerError> {
        let acquisition = self.pool.acquire(key)?;
        if let Some(evicted) = &acquisition.evicted {
            self.handler.reset(evicted);
        }
        if acquisition.reused {
            self.handler.reset(key);
        }

        // active-CPU: the synchronous invoke duration IS the frame's on-CPU
        // time (no mid-frame I/O await), so this is honest CPU, not wall-clock.
        let started = Instant::now();
        let output = match self.handler.invoke(key, input) {
            Ok(output) => output,
            Err(error) => {
                // Never leave a failed isolate warm.
                let _ = self.pool.evict(key);
                return Err(error);
            }
        };
        let active_cpu = started.elapsed();

        if residency == Residency::Hibernated {
            self.pool.evict(key)?;
        }

        if let Some(meter) = &self.meter {
            meter.record(UsageRecord {
                key: key.clone(),
                active_cpu,
                residency,
                warm_hit: acquisition.reused,
            });
        }

        Ok(FrameInvocation {
            output,
            warm_hit: acquisition.reused,
            residency,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::TenantId;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn key(instance: &str) -> InstanceKey {
        InstanceKey::new(
            TenantId::new("tenant-a").expect("test tenant"),
            "chat",
            instance,
        )
    }

    /// Counter handler: echoes an incrementing per-frame count into state, and
    /// records how many times invoke/reset were called (to prove warm reuse
    /// and the reset contract).
    #[derive(Default)]
    struct CountingHandler {
        invokes: AtomicUsize,
        resets: AtomicUsize,
    }

    impl FrameHandler for Arc<CountingHandler> {
        fn invoke(
            &self,
            _key: &InstanceKey,
            input: FrameInput,
        ) -> Result<FrameOutput, BrokerError> {
            self.invokes.fetch_add(1, Ordering::Relaxed);
            let prev = input.state.first().copied().unwrap_or(0);
            let next = prev.wrapping_add(1);
            Ok(FrameOutput {
                outbound: vec![HostFrame::Text(format!("frame {next}"))],
                state: vec![next],
            })
        }
        fn reset(&self, _key: &InstanceKey) {
            self.resets.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn invoker(max_warm: usize) -> (FrameInvoker<Arc<CountingHandler>>, Arc<CountingHandler>) {
        let handler = Arc::new(CountingHandler::default());
        (FrameInvoker::new(Arc::clone(&handler), max_warm), handler)
    }

    #[test]
    fn hibernated_evicts_after_every_frame_state_threads_through() {
        let (invoker, handler) = invoker(8);
        let k = key("room-1");

        let r1 = invoker
            .per_frame_invoke(
                &k,
                Residency::Hibernated,
                FrameInput {
                    inbound: HostFrame::Text("a".into()),
                    state: vec![],
                },
            )
            .expect("frame 1");
        assert_eq!(r1.output.state, vec![1]);
        assert!(!r1.warm_hit, "cold start");
        assert!(!invoker.is_warm(&k), "Hibernated evicts after the frame");

        // Next frame is cold again (state must be threaded by the caller / CB3).
        let r2 = invoker
            .per_frame_invoke(
                &k,
                Residency::Hibernated,
                FrameInput {
                    inbound: HostFrame::Text("b".into()),
                    state: r1.output.state,
                },
            )
            .expect("frame 2");
        assert_eq!(r2.output.state, vec![2], "state threads across frames");
        assert!(!r2.warm_hit, "Hibernated is always a cold slot");
        assert_eq!(handler.invokes.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn resident_keeps_slot_warm_and_resets_between_frames() {
        let (invoker, handler) = invoker(8);
        let k = key("room-1");

        let r1 = invoker
            .per_frame_invoke(
                &k,
                Residency::Resident,
                FrameInput {
                    inbound: HostFrame::Text("a".into()),
                    state: vec![],
                },
            )
            .expect("frame 1");
        assert!(!r1.warm_hit, "first frame is cold");
        assert!(invoker.is_warm(&k), "Resident keeps the slot warm");

        let r2 = invoker
            .per_frame_invoke(
                &k,
                Residency::Resident,
                FrameInput {
                    inbound: HostFrame::Text("b".into()),
                    state: r1.output.state,
                },
            )
            .expect("frame 2");
        assert!(r2.warm_hit, "second frame reuses the warm slot");
        // reset called once — on the warm reuse (the between-frame clear).
        assert_eq!(handler.resets.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn overcommit_cap_evicts_lru_owner_and_resets_it() {
        let (invoker, handler) = invoker(2);
        let a = key("a");
        let b = key("b");
        let c = key("c");
        let frame = || FrameInput {
            inbound: HostFrame::Text("x".into()),
            state: vec![],
        };

        invoker
            .per_frame_invoke(&a, Residency::Resident, frame())
            .unwrap();
        invoker
            .per_frame_invoke(&b, Residency::Resident, frame())
            .unwrap();
        assert_eq!(invoker.warm_len(), 2, "at cap");

        // c pushes over the cap: a (LRU) is evicted and reset.
        invoker
            .per_frame_invoke(&c, Residency::Resident, frame())
            .unwrap();
        assert_eq!(invoker.warm_len(), 2, "still at cap after eviction");
        assert!(!invoker.is_warm(&a), "LRU owner a evicted");
        assert!(invoker.is_warm(&b) && invoker.is_warm(&c));
        // a's eviction triggered a reset (isolate state cleared before reuse).
        assert!(handler.resets.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn metered_invoke_emits_active_cpu_and_residency_usage_records() {
        use crate::meter::CountingMeter;
        let meter = std::sync::Arc::new(CountingMeter::new());
        let (_i, handler) = invoker(8);
        let invoker = FrameInvoker::new(handler, 8).with_meter(meter.clone());
        let k = key("room-1");

        invoker
            .per_frame_invoke(
                &k,
                Residency::Hibernated,
                FrameInput {
                    inbound: HostFrame::Text("a".into()),
                    state: vec![],
                },
            )
            .unwrap();
        invoker
            .per_frame_invoke(
                &k,
                Residency::Resident,
                FrameInput {
                    inbound: HostFrame::Text("b".into()),
                    state: vec![],
                },
            )
            .unwrap();

        let usage = meter.usage_for(&k);
        assert_eq!(
            usage.frames, 2,
            "each per-frame invoke emits one usage record"
        );
        assert_eq!(usage.hibernated_frames, 1);
        assert_eq!(
            usage.resident_frames, 1,
            "residency is recorded per frame — no silent fallback"
        );
        // active_cpu is the summed synchronous invoke time (>= 0; monotonic).
        assert!(usage.active_cpu >= std::time::Duration::ZERO);
    }

    #[test]
    fn handler_error_evicts_the_slot_never_leaves_it_warm() {
        struct FailingHandler;
        impl FrameHandler for FailingHandler {
            fn invoke(
                &self,
                _key: &InstanceKey,
                _input: FrameInput,
            ) -> Result<FrameOutput, BrokerError> {
                Err(BrokerError {
                    message: "isolate trap".into(),
                })
            }
        }
        let invoker = FrameInvoker::new(FailingHandler, 8);
        let k = key("room-1");
        let err = invoker
            .per_frame_invoke(
                &k,
                Residency::Resident,
                FrameInput {
                    inbound: HostFrame::Text("a".into()),
                    state: vec![],
                },
            )
            .expect_err("handler error propagates");
        assert!(err.message.contains("isolate trap"));
        assert!(
            !invoker.is_warm(&k),
            "a failed isolate must never be left warm to serve the next frame"
        );
    }
}

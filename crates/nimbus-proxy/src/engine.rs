//! Node-scoped egress engine: the composition root that owns the shared
//! [`ProxySubstrate`] and the per-workload PEP registry.
//!
//! One `EgressEngine` per node. The engine's map is a **lifecycle registry** —
//! it is touched at register/deregister/reload time only and is never consulted
//! at accept or on the request path. Each accepted connection is handled by the
//! [`WorkloadPep`]'s own accept task, which closes over that PEP's captured
//! context; the request handler literally cannot name another workload's state,
//! and it cannot name this map (enforced by the EE1 reachability lint in
//! `tests.rs` and the plan verifier).
//!
//! The map is keyed by the opaque [`nimbus_core::WorkloadId`] — never a
//! sandbox-layer id type — so `nimbus-proxy` never depends on `nimbus-sandbox`.
//! Sandbox-layer publishing machinery (trust-anchor files, roots, port
//! allocation) stays in `nimbus-sandbox` and is injected: the engine carries an
//! opaque per-entry `attachment` and exposes a lock-holding
//! [`RegistrationSlot`], so the caller's publish step and the map insert happen
//! under one lock hold (a published artifact can never belong to a different
//! PEP than the one registered).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use nimbus_core::WorkloadId;

use crate::error::{EgressProxyError, Result};
use crate::substrate::ProxySubstrate;
use crate::worker::WorkloadPep;

struct EngineEntry<A> {
    pep: WorkloadPep,
    attachment: A,
}

/// Node-scoped owner of the shared proxy substrate and the per-workload PEP
/// lifecycle registry. `A` is an opaque per-entry attachment owned by the
/// caller (e.g. the sandbox layer's published trust-anchor record); the engine
/// never inspects it.
pub struct EgressEngine<A = ()> {
    substrate: ProxySubstrate,
    peps: Mutex<HashMap<WorkloadId, EngineEntry<A>>>,
}

impl<A> EgressEngine<A> {
    /// Create an engine on the shared node-wide substrate.
    pub fn new() -> Self {
        Self::with_substrate(ProxySubstrate::shared())
    }

    /// Create an engine on an explicit substrate (tests / dedicated runtimes).
    pub fn with_substrate(substrate: ProxySubstrate) -> Self {
        Self {
            substrate,
            peps: Mutex::new(HashMap::new()),
        }
    }

    /// The substrate this engine runs its PEPs on.
    pub fn substrate(&self) -> &ProxySubstrate {
        &self.substrate
    }

    /// True if a PEP is registered for `id`.
    pub fn contains(&self, id: &WorkloadId) -> Result<bool> {
        Ok(self.lock()?.contains_key(id))
    }

    /// Number of registered PEPs (the node-wide feature seam: fan-out,
    /// metrics, and fairness iterate lifecycle state, never request state).
    pub fn len(&self) -> Result<usize> {
        Ok(self.lock()?.len())
    }

    /// True if no PEPs are registered.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.lock()?.is_empty())
    }

    /// Reserve the registration slot for `id`, holding the registry lock.
    ///
    /// Returns `Ok(None)` if a PEP is already registered (the caller treats
    /// that as already-running). While the returned slot is alive the caller
    /// can run its publish step (e.g. write a trust-anchor file) with the
    /// guarantee that no concurrent register/deregister for any workload can
    /// interleave; committing the slot inserts the PEP under the same lock
    /// hold. Dropping the slot without committing releases the reservation
    /// (the caller unwinds its own publish step).
    pub fn try_reserve(&self, id: WorkloadId) -> Result<Option<RegistrationSlot<'_, A>>> {
        let guard = self.lock()?;
        if guard.contains_key(&id) {
            return Ok(None);
        }
        Ok(Some(RegistrationSlot { guard, id }))
    }

    /// Remove and return the PEP (and its attachment) registered for `id`.
    ///
    /// Returns `Ok(None)` if none is registered. The caller drops the PEP to
    /// stop it, then unwinds its published artifacts using the attachment.
    pub fn deregister(&self, id: &WorkloadId) -> Result<Option<(WorkloadPep, A)>> {
        Ok(self
            .lock()?
            .remove(id)
            .map(|entry| (entry.pep, entry.attachment)))
    }

    /// Run `f` against the PEP registered for `id`, under the registry lock.
    ///
    /// Lifecycle-only accessor (reload, readiness, addresses): no PEP handle
    /// escapes the registry, and the request path never calls this — accept
    /// tasks are spawned by the PEP itself and own their captured context.
    pub fn with_pep<R>(
        &self,
        id: &WorkloadId,
        f: impl FnOnce(&WorkloadPep) -> R,
    ) -> Result<Option<R>> {
        Ok(self.lock()?.get(id).map(|entry| f(&entry.pep)))
    }

    fn lock(&self) -> Result<MutexGuard<'_, HashMap<WorkloadId, EngineEntry<A>>>> {
        self.peps
            .lock()
            .map_err(|_| EgressProxyError::OperationFailed {
                message: "egress engine registry lock is poisoned".to_owned(),
            })
    }
}

impl<A> Default for EgressEngine<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// A reserved, lock-holding registration slot for one workload id.
///
/// Exists so the caller's publish step and the map insert are atomic with
/// respect to every other lifecycle operation — the same single-lock-hold
/// contract the sandbox registry established for trust-anchor publication.
pub struct RegistrationSlot<'a, A> {
    guard: MutexGuard<'a, HashMap<WorkloadId, EngineEntry<A>>>,
    id: WorkloadId,
}

impl<A> RegistrationSlot<'_, A> {
    /// The workload id this slot reserves.
    pub fn id(&self) -> &WorkloadId {
        &self.id
    }

    /// Commit the reservation: register `pep` (with the caller's opaque
    /// `attachment`) under the lock held since [`EgressEngine::try_reserve`].
    pub fn commit(mut self, pep: WorkloadPep, attachment: A) {
        self.guard
            .insert(self.id.clone(), EngineEntry { pep, attachment });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::WorkloadPepConfig;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn test_pep() -> WorkloadPep {
        WorkloadPep::start(
            WorkloadPepConfig::without_active_policy()
                .with_bind_addr(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)),
        )
        .expect("test PEP should start on an ephemeral port")
    }

    fn wid(raw: &str) -> WorkloadId {
        WorkloadId::new(raw).expect("test workload id")
    }

    #[test]
    fn reserve_commit_registers_and_deregister_returns_attachment() {
        let engine: EgressEngine<u32> = EgressEngine::new();
        let id = wid("workload-a");

        let slot = engine
            .try_reserve(id.clone())
            .expect("lock healthy")
            .expect("slot free");
        assert_eq!(slot.id(), &id);
        slot.commit(test_pep(), 7);

        assert!(engine.contains(&id).unwrap());
        assert_eq!(engine.len().unwrap(), 1);

        let (pep, attachment) = engine
            .deregister(&id)
            .expect("lock healthy")
            .expect("entry present");
        assert_eq!(attachment, 7);
        drop(pep);
        assert!(!engine.contains(&id).unwrap());
        assert!(engine.is_empty().unwrap());
    }

    #[test]
    fn second_reserve_for_same_id_is_refused_while_registered() {
        let engine: EgressEngine = EgressEngine::new();
        let id = wid("workload-b");
        engine
            .try_reserve(id.clone())
            .unwrap()
            .expect("first reservation free")
            .commit(test_pep(), ());

        assert!(
            engine.try_reserve(id.clone()).unwrap().is_none(),
            "an occupied id must not hand out a second slot"
        );

        // After deregistration the id is reusable.
        let (pep, ()) = engine.deregister(&id).unwrap().expect("registered");
        drop(pep);
        assert!(engine.try_reserve(id).unwrap().is_some());
    }

    #[test]
    fn dropping_slot_without_commit_releases_reservation() {
        let engine: EgressEngine = EgressEngine::new();
        let id = wid("workload-c");
        let slot = engine.try_reserve(id.clone()).unwrap().expect("free");
        drop(slot);
        assert!(!engine.contains(&id).unwrap());
        assert!(
            engine.try_reserve(id).unwrap().is_some(),
            "an uncommitted reservation must not leak"
        );
    }

    #[test]
    fn with_pep_exposes_lifecycle_reads_without_escaping_handles() {
        let engine: EgressEngine = EgressEngine::new();
        let id = wid("workload-d");
        engine
            .try_reserve(id.clone())
            .unwrap()
            .expect("free")
            .commit(test_pep(), ());

        let addr = engine
            .with_pep(&id, |pep| pep.local_addr())
            .unwrap()
            .expect("registered");
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_ne!(addr.port(), 0);

        assert!(
            engine
                .with_pep(&wid("workload-absent"), |pep| pep.local_addr())
                .unwrap()
                .is_none(),
            "absent id reads as None, not an error"
        );
    }

    #[test]
    fn deregister_absent_id_is_none_not_error() {
        let engine: EgressEngine = EgressEngine::new();
        assert!(
            engine
                .deregister(&wid("never-registered"))
                .unwrap()
                .is_none()
        );
    }
}

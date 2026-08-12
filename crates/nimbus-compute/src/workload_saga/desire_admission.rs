//! Provider admission guard held across Engine desire CAS operations.
//!
//! Compute owns when a running desire needs admission. A provider adapter owns
//! the process-safe exclusion mechanism and returns an opaque RAII permit.

use std::future::Future;
use std::pin::Pin;

use nimbus_workloads::{
    WorkloadDesiredDigest, WorkloadExecutionProviderId, WorkloadGeneration,
    WorkloadProvisionSourceDigest, WorkloadSagaKey,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadDesireAdmissionRequest {
    key: WorkloadSagaKey,
    execution_provider_id: WorkloadExecutionProviderId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source_digest: WorkloadProvisionSourceDigest,
}

impl WorkloadDesireAdmissionRequest {
    pub fn new(
        key: WorkloadSagaKey,
        execution_provider_id: WorkloadExecutionProviderId,
        generation: WorkloadGeneration,
        desired_digest: WorkloadDesiredDigest,
        source_digest: WorkloadProvisionSourceDigest,
    ) -> Self {
        Self {
            key,
            execution_provider_id,
            generation,
            desired_digest,
            source_digest,
        }
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        &self.execution_provider_id
    }

    pub const fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub const fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        self.source_digest
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorkloadDesireAdmissionError {
    #[error("workload desire admission is fenced by physical-machine stop")]
    Fenced,
    #[error("workload desire admission authority is unavailable")]
    Unavailable,
    #[error("workload desire admission authority is ambiguous")]
    Ambiguous,
    #[error("workload desire admission authority is corrupt")]
    Corrupt,
    #[error("workload desire admission uses a stale machine generation")]
    Stale,
    #[error("workload desire admission crosses machine identity")]
    Crossed,
}

/// Opaque provider-owned lock lifetime. Dropping it releases admission.
pub trait WorkloadDesireAdmissionPermit: Send {}

pub type WorkloadDesireAdmissionFuture<'a> = Pin<
    Box<
        dyn Future<
                Output = Result<
                    Box<dyn WorkloadDesireAdmissionPermit>,
                    WorkloadDesireAdmissionError,
                >,
            > + Send
            + 'a,
    >,
>;

pub trait WorkloadDesireAdmissionGuard: Send + Sync + 'static {
    fn acquire<'a>(
        &'a self,
        request: &'a WorkloadDesireAdmissionRequest,
    ) -> WorkloadDesireAdmissionFuture<'a>;
}

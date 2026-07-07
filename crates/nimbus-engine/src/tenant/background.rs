//! Shared scaffolding for tenant-owned background pipelines.
//!
//! Every dedicated-thread delivery pipeline on a tenant (subscription
//! delivery, trigger candidates, trigger execution) is built from the same
//! two pieces: a blocking work queue (`WorkQueue`) and an OS-thread worker
//! lifecycle (`BackgroundWorker`). Pipelines with extra structure (e.g.
//! trigger execution's ready-at ordered retry queue) keep their own
//! specialized queue type and adapt only the worker lifecycle.

mod queue;
mod worker;

pub(crate) use queue::WorkQueue;
pub(crate) use worker::BackgroundWorker;

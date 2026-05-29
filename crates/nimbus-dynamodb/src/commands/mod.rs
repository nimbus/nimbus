//! DynamoDB operation handlers, grouped by family. Each handler operates over a
//! tenant-scoped `Service` and returns extenddb-core typed I/O. `dispatch.rs`
//! authenticates the request (access key → tenant) and routes `X-Amz-Target`
//! operations to these handlers via a `DispatchContext`.

pub mod batch;
pub mod control_plane;
pub mod discovery;
pub mod item;
pub mod query;
pub mod transact;

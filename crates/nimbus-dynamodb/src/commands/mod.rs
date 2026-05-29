//! DynamoDB operation handlers, grouped by family. Each handler operates over a
//! tenant-scoped `Service` and returns extenddb-core typed I/O. `dispatch.rs`
//! routes `X-Amz-Target` operations to these (wiring lands with D0.8 auth).

pub mod control_plane;

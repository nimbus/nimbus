//! Re-export shim: the canonical `Clock`/`SystemClock`/`ManualClock` seam
//! now lives in `nimbus_core::clock` (CO7), beside `Timestamp`. Kept here so
//! every existing `nimbus_storage::simulation::{Clock, SystemClock,
//! ManualClock}` consumer (pervasive in engine/storage tests) compiles
//! unchanged.

pub use nimbus_core::clock::{Clock, ManualClock, SystemClock};

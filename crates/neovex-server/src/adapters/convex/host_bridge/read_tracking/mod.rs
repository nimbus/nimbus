//! Convex runtime read-recording glue on top of the shared read-tracking core.
//!
//! The shared `execution::read_tracking` module owns the canonical runtime
//! read-set semantics. This subtree only translates Convex-specific query
//! builder state and host-call results into those shared primitives.

use super::*;

mod builders;
mod indexes;
mod recording;

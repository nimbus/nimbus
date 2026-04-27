//! Convex host-bridge recording helpers for already-executed runtime reads.
//!
//! These helpers should only map Convex query/document results into the shared
//! read-tracking core; they should not grow their own intersection or commit
//! logic.

use super::*;

mod primitives;
mod queries;

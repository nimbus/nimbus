#![forbid(unsafe_code)]

//! Feature-gated facade for Nimbus adapter crates.
//!
//! This crate intentionally contains only re-exports. Adapter implementation
//! stays in each owning crate, and transport composition stays with the
//! application integration layer.

#[cfg(feature = "cloud-functions")]
pub mod cloud_functions {
    pub use nimbus_cloud_functions::*;
}

#[cfg(feature = "convex")]
pub mod convex {
    pub use nimbus_convex::*;
}

#[cfg(feature = "dynamodb")]
pub mod dynamodb {
    pub use nimbus_dynamodb::*;
}

#[cfg(feature = "firebase")]
pub mod firebase {
    pub use nimbus_firebase::*;
}

#[cfg(feature = "mongodb")]
pub mod mongodb {
    pub use nimbus_mongodb::*;
}

#[cfg(feature = "s3")]
pub mod s3 {
    pub use nimbus_s3::*;
}

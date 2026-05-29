//! Authentication surface for the DynamoDB adapter.
//!
//! Hosts the vendored AWS SigV4 verification module (`sigv4`). Access-key →
//! tenant resolution and the strict/lookup `auth_mode` toggle land in
//! D0.5 / D0.8 / D7.

pub mod sigv4;

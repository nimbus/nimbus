use std::time::Duration;

mod config;
mod jwt;
#[cfg(test)]
mod tests;
mod verifier;

pub(in crate::adapters::convex) use config::read_auth_config;
pub(in crate::adapters::convex) use verifier::ConvexAuthVerifier;

const CLOCK_SKEW: Duration = Duration::from_secs(30);

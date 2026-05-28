use std::time::Duration;

mod config;
mod jwt;
#[cfg(test)]
mod tests;
mod verifier;

pub use config::read_auth_config;
pub use verifier::ConvexAuthVerifier;

const CLOCK_SKEW: Duration = Duration::from_secs(30);

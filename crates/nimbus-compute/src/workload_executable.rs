//! Compute-owned codec between portable workload intent and sandbox execution.

use nimbus_sandbox::SandboxSpec;
use nimbus_workloads::{WorkloadExecutableEncoding, WorkloadExecutableIntent, WorkloadSagaError};
use thiserror::Error;

/// A strict executable-carrier conversion failure.
#[derive(Debug, Error)]
pub enum WorkloadExecutableCodecError {
    #[error("sandbox specification could not be encoded")]
    Encode(#[source] serde_json::Error),
    #[error("sandbox specification executable content could not be decoded")]
    Decode,
    #[error("sandbox specification executable content is not canonical JSON")]
    NonCanonical,
    #[error("sandbox specification executable carrier is invalid")]
    InvalidCarrier(#[source] WorkloadSagaError),
}

/// Encode one complete sandbox specification as exact compact JSON.
pub fn encode_sandbox_spec(
    spec: &SandboxSpec,
) -> Result<WorkloadExecutableIntent, WorkloadExecutableCodecError> {
    let encoded = serde_json::to_vec(spec).map_err(WorkloadExecutableCodecError::Encode)?;
    let content = String::from_utf8(encoded)
        .expect("serde_json always emits valid UTF-8 for a serializable sandbox specification");
    WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        content,
    )
    .map_err(WorkloadExecutableCodecError::InvalidCarrier)
}

/// Decode one exact canonical sandbox specification without accepting aliases.
pub fn decode_sandbox_spec(
    intent: &WorkloadExecutableIntent,
) -> Result<SandboxSpec, WorkloadExecutableCodecError> {
    match intent.encoding() {
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1 => {}
    }
    let decoded: SandboxSpec = serde_json::from_slice(intent.canonical_content().as_bytes())
        .map_err(|_| WorkloadExecutableCodecError::Decode)?;
    let decoded_canonical = String::from_utf8(
        serde_json::to_vec(&decoded).map_err(WorkloadExecutableCodecError::Encode)?,
    )
    .expect("serde_json always emits valid UTF-8 for a decoded sandbox specification");
    if decoded_canonical != intent.canonical_content() {
        return Err(WorkloadExecutableCodecError::NonCanonical);
    }
    Ok(decoded)
}

#[cfg(test)]
#[path = "workload_executable/tests.rs"]
mod tests;

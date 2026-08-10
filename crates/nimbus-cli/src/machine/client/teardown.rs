//! Strict one-shot transport for one compute-confirmed teardown phase.
//!
//! The parent adapter owns durable request-may-exist state and retry order.
//! This client sends exactly one requested mode, validates every response
//! fence, and never converts transport uncertainty into an automatic retry.

#![allow(
    dead_code,
    reason = "band 7 installs the transport seam before the band 8 parent adapter calls it"
)]

use nimbus::Error;
use nimbus_machine::api::{
    MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH, MachineApiWorkloadTeardownPhaseRequest,
    MachineApiWorkloadTeardownPhaseResponse,
};

use super::{
    MachineApiClient, extract_machine_api_json_body, machine_api_request_with_response_limit,
};

/// Bounded well above every closed correlated observation while keeping a
/// compromised or crossed guest from driving unbounded parent allocation.
const MAX_WORKLOAD_TEARDOWN_RESPONSE_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub(crate) enum MachineApiWorkloadTeardownTransportOutcome {
    Correlated(Box<MachineApiWorkloadTeardownPhaseResponse>),
    Ambiguous { reason: String },
}

impl MachineApiWorkloadTeardownTransportOutcome {
    fn ambiguous(error: impl std::fmt::Display) -> Self {
        Self::Ambiguous {
            reason: format!(
                "machine API workload teardown request has an ambiguous outcome: {error}"
            ),
        }
    }
}

impl MachineApiClient {
    pub(crate) fn teardown_workload_phase(
        &self,
        request: &MachineApiWorkloadTeardownPhaseRequest,
    ) -> Result<MachineApiWorkloadTeardownTransportOutcome, Error> {
        let encoded = serde_json::to_vec(request).map_err(|error| {
            Error::Internal(format!(
                "failed to encode the exact machine API workload teardown request: {error}"
            ))
        })?;
        self.teardown_workload_phase_prepared(request, &encoded)
    }

    /// Send the exact request bytes previously committed by the provider journal.
    pub(crate) fn teardown_workload_phase_prepared(
        &self,
        request: &MachineApiWorkloadTeardownPhaseRequest,
        prepared: &[u8],
    ) -> Result<MachineApiWorkloadTeardownTransportOutcome, Error> {
        self.service_forwarder_authority()?
            .authenticate(request.forwarder_authority())
            .map_err(|error| {
            Error::InvalidInput(format!(
                "machine API workload teardown request is crossed with the configured forwarder authority: {error}"
                ))
            })?;
        let decoded: MachineApiWorkloadTeardownPhaseRequest = serde_json::from_slice(prepared)
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "prepared machine API workload teardown request is invalid: {error}"
                ))
            })?;
        if &decoded != request {
            return Err(Error::InvalidInput(
                "prepared machine API workload teardown request is crossed with the authenticated command"
                    .to_owned(),
            ));
        }
        let (status, response) = match machine_api_request_with_response_limit(
            &self.socket_path,
            "POST",
            MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
            Some(prepared),
            self.mutation_io_timeout,
            Some(MAX_WORKLOAD_TEARDOWN_RESPONSE_BODY_BYTES),
        ) {
            Ok(response) => response,
            Err(error) => {
                return Ok(MachineApiWorkloadTeardownTransportOutcome::ambiguous(error));
            }
        };
        let body = match extract_machine_api_json_body(
            status,
            &response,
            &self.socket_path,
            MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
        ) {
            Ok(body) => body,
            Err(error) => {
                return Ok(MachineApiWorkloadTeardownTransportOutcome::ambiguous(error));
            }
        };
        let response: MachineApiWorkloadTeardownPhaseResponse = match serde_json::from_slice(body) {
            Ok(response) => response,
            Err(error) => {
                return Ok(MachineApiWorkloadTeardownTransportOutcome::ambiguous(error));
            }
        };
        if let Err(error) = response.validate_for_request(request) {
            return Ok(MachineApiWorkloadTeardownTransportOutcome::ambiguous(error));
        }
        Ok(MachineApiWorkloadTeardownTransportOutcome::Correlated(
            Box::new(response),
        ))
    }
}

#[cfg(test)]
mod tests;

//! Exact provider-observation fixtures shared by compute contract tests.

use std::net::{Ipv4Addr, SocketAddr};

use nimbus_network::{NetworkAttachmentHandle, PublishedEndpoint, PublishedEndpointHandle};
use nimbus_sandbox::{
    SandboxExecutionAttemptId, SandboxHandle, SandboxInspection, SandboxNetworkStatus,
    SandboxStatus,
};
use nimbus_workloads::WorkloadPublicationIntent;

use super::{WorkloadExecutionObservationRequest, decode_sandbox_spec};

pub(crate) fn exact_execution_inspection(
    request: &WorkloadExecutionObservationRequest,
    provider_evidence: &[u8],
) -> SandboxInspection {
    let spec = decode_sandbox_spec(request.executable())
        .expect("exact execution fixture executable should decode");
    let content = request.compiled_network_plan().content();
    let endpoint_handles = if content.publication() == WorkloadPublicationIntent::PublishWhenReady {
        content
            .listeners()
            .iter()
            .enumerate()
            .map(|(ordinal, listener)| {
                let endpoint = PublishedEndpoint::new(
                    listener.name(),
                    listener.protocol(),
                    SocketAddr::from((Ipv4Addr::LOCALHOST, 49_152 + ordinal as u16)),
                );
                let endpoint = listener
                    .guest_port()
                    .map_or(endpoint.clone(), |port| endpoint.with_guest_port(port));
                PublishedEndpointHandle::new(
                    listener.endpoint_id().clone(),
                    content.identity().generation(),
                    endpoint,
                )
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let visible_endpoints = endpoint_handles
        .iter()
        .map(|endpoint| endpoint.endpoint().clone())
        .collect();
    let network_status = content.attachment().map(|attachment| {
        SandboxNetworkStatus::new(
            Some(NetworkAttachmentHandle::new(
                attachment.attachment_id().clone(),
                content.identity().generation(),
            )),
            endpoint_handles,
        )
        .expect("exact execution fixture network status should validate")
    });

    SandboxInspection::provider_authenticated_running_with_network_status(
        SandboxHandle::new(
            request.key().tenant_id().clone(),
            nimbus_sandbox::SandboxId::new(request.execution().execution_id().as_str()),
            spec.display_name(),
            spec.backend,
            SandboxStatus::Ready,
            visible_endpoints,
        ),
        network_status,
        SandboxExecutionAttemptId::new(request.execution().attempt_id().to_string())
            .expect("exact execution fixture attempt should validate"),
        provider_evidence,
    )
}

use std::collections::BTreeSet;

use nimbus_network::{NetworkAttachmentHandle, NetworkResourceGeneration, PublishedEndpointHandle};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Rebuildable portable network identity observed for one sandbox.
///
/// This value contains no provider handle and grants no lifecycle authority.
/// Effect owners authenticate their durable state before constructing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxNetworkStatus {
    attachment: Option<NetworkAttachmentHandle>,
    published_endpoints: Vec<PublishedEndpointHandle>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SandboxNetworkStatusWire {
    attachment: Option<NetworkAttachmentHandle>,
    published_endpoints: Vec<PublishedEndpointHandle>,
}

impl SandboxNetworkStatus {
    /// Validate one provider-neutral status snapshot.
    pub fn new(
        attachment: Option<NetworkAttachmentHandle>,
        published_endpoints: impl IntoIterator<Item = PublishedEndpointHandle>,
    ) -> Result<Self, SandboxNetworkStatusError> {
        let mut published_endpoints = published_endpoints.into_iter().collect::<Vec<_>>();
        if attachment.is_none() && !published_endpoints.is_empty() {
            return Err(SandboxNetworkStatusError::EndpointWithoutAttachment);
        }

        let mut endpoint_ids = BTreeSet::new();
        let mut endpoint_names = BTreeSet::new();
        for endpoint in &published_endpoints {
            if endpoint.endpoint().name.is_empty() {
                return Err(SandboxNetworkStatusError::EmptyEndpointName);
            }
            if !endpoint_ids.insert(endpoint.endpoint_id().clone()) {
                return Err(SandboxNetworkStatusError::DuplicateEndpoint);
            }
            if !endpoint_names.insert(endpoint.endpoint().name.clone()) {
                return Err(SandboxNetworkStatusError::DuplicateEndpointName);
            }
            if attachment
                .as_ref()
                .is_some_and(|attachment| attachment.generation() != endpoint.generation())
            {
                return Err(SandboxNetworkStatusError::GenerationMismatch);
            }
        }
        published_endpoints.sort_by(|left, right| {
            left.endpoint_id()
                .cmp(right.endpoint_id())
                .then_with(|| left.endpoint().name.cmp(&right.endpoint().name))
        });
        Ok(Self {
            attachment,
            published_endpoints,
        })
    }

    /// No exact portable network observation is available.
    pub const fn empty() -> Self {
        Self {
            attachment: None,
            published_endpoints: Vec::new(),
        }
    }

    /// Address-independent attachment identity, when provider evidence proves
    /// that the attachment exists for this sandbox generation.
    pub const fn attachment(&self) -> Option<&NetworkAttachmentHandle> {
        self.attachment.as_ref()
    }

    /// Portable endpoint identities with their current observed locations.
    pub fn published_endpoints(&self) -> &[PublishedEndpointHandle] {
        &self.published_endpoints
    }

    /// The generation shared by every portable identity in this status.
    pub fn generation(&self) -> Option<NetworkResourceGeneration> {
        self.attachment
            .as_ref()
            .map(NetworkAttachmentHandle::generation)
    }

    /// Whether this status contains no exact attachment or endpoint evidence.
    pub fn is_empty(&self) -> bool {
        self.attachment.is_none() && self.published_endpoints.is_empty()
    }
}

impl TryFrom<SandboxNetworkStatusWire> for SandboxNetworkStatus {
    type Error = SandboxNetworkStatusError;

    fn try_from(wire: SandboxNetworkStatusWire) -> Result<Self, Self::Error> {
        Self::new(wire.attachment, wire.published_endpoints)
    }
}

impl<'de> Deserialize<'de> for SandboxNetworkStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        SandboxNetworkStatusWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Invalid or internally crossed portable sandbox network status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SandboxNetworkStatusError {
    #[error("sandbox endpoint status requires an attachment handle")]
    EndpointWithoutAttachment,
    #[error("sandbox endpoint status contains an empty endpoint name")]
    EmptyEndpointName,
    #[error("sandbox endpoint status contains a duplicate endpoint identity")]
    DuplicateEndpoint,
    #[error("sandbox endpoint status contains a duplicate endpoint name")]
    DuplicateEndpointName,
    #[error("sandbox network status mixes resource generations")]
    GenerationMismatch,
}

#[cfg(test)]
mod tests {
    use nimbus_network::{
        EndpointProtocol, NetworkAttachmentId, PublishedEndpoint, PublishedEndpointId,
    };

    use super::*;

    fn attachment(generation: NetworkResourceGeneration) -> NetworkAttachmentHandle {
        NetworkAttachmentHandle::new(
            NetworkAttachmentId::for_workload_attachment("status/workload", "primary"),
            generation,
        )
    }

    fn endpoint(
        id_name: &str,
        endpoint_name: &str,
        generation: NetworkResourceGeneration,
    ) -> PublishedEndpointHandle {
        PublishedEndpointHandle::new(
            PublishedEndpointId::for_workload_endpoint("status/workload", id_name),
            generation,
            PublishedEndpoint::new(
                endpoint_name,
                EndpointProtocol::Http,
                "127.0.0.1:8080".parse().expect("fixture address"),
            ),
        )
    }

    #[test]
    fn invalid_identity_shapes_fail_closed() {
        let generation = NetworkResourceGeneration::new(7);
        let other_generation = NetworkResourceGeneration::new(8);
        assert_eq!(
            SandboxNetworkStatus::new(None, [endpoint("api", "api", generation)]),
            Err(SandboxNetworkStatusError::EndpointWithoutAttachment)
        );
        assert_eq!(
            SandboxNetworkStatus::new(
                Some(attachment(generation)),
                [
                    endpoint("api", "api", generation),
                    endpoint("api", "other", generation)
                ],
            ),
            Err(SandboxNetworkStatusError::DuplicateEndpoint)
        );
        assert_eq!(
            SandboxNetworkStatus::new(
                Some(attachment(generation)),
                [
                    endpoint("api", "same", generation),
                    endpoint("admin", "same", generation)
                ],
            ),
            Err(SandboxNetworkStatusError::DuplicateEndpointName)
        );
        assert_eq!(
            SandboxNetworkStatus::new(
                Some(attachment(generation)),
                [endpoint("api", "api", other_generation)],
            ),
            Err(SandboxNetworkStatusError::GenerationMismatch)
        );
    }

    #[test]
    fn portable_wire_is_strict_and_contains_no_provider_handle_material() {
        let generation = NetworkResourceGeneration::new(7);
        let status = SandboxNetworkStatus::new(
            Some(attachment(generation)),
            [endpoint("api", "api", generation)],
        )
        .expect("status should validate");
        let json = serde_json::to_string(&status).expect("status should serialize");
        assert!(!json.contains("provider"));
        assert!(!json.contains("opaque"));
        assert_eq!(
            serde_json::from_str::<SandboxNetworkStatus>(&json).expect("status should deserialize"),
            status
        );
        let crossed = json.replacen('{', "{\"unexpected\":true,", 1);
        assert!(serde_json::from_str::<SandboxNetworkStatus>(&crossed).is_err());
    }
}

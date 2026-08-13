use serde::{Deserialize, Serialize};

use crate::{NetworkAttachmentId, NetworkResourceGeneration};

/// Portable identity and generation for one observed network attachment.
///
/// This handle deliberately excludes provider identity, opaque provider
/// material, addresses, and lifecycle authority. Effect owners can attach
/// provider-neutral observation data without exposing their durable handles.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetworkAttachmentHandle {
    attachment_id: NetworkAttachmentId,
    generation: NetworkResourceGeneration,
}

impl NetworkAttachmentHandle {
    /// Compose already-authenticated attachment identity and generation.
    pub const fn new(
        attachment_id: NetworkAttachmentId,
        generation: NetworkResourceGeneration,
    ) -> Self {
        Self {
            attachment_id,
            generation,
        }
    }

    /// Stable address-independent attachment identity.
    pub const fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    /// Desired network generation authenticated for this observation.
    pub const fn generation(&self) -> NetworkResourceGeneration {
        self.generation
    }
}

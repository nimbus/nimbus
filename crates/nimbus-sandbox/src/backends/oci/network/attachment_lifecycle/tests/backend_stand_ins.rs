//! Backend stand-ins for the attachment contract suite. The real backends live
//! in their own crates and implement only the attachment trait defaults.

use super::*;

/// Stands in for the Container backend, which implements only the attachment trait defaults.
pub(super) struct ContainerAttachmentBackend;

impl OciHostManagedAttachmentBackend for ContainerAttachmentBackend {
    const ATTACHMENT_BACKEND_KIND: AttachmentBackendKind = AttachmentBackendKind::Container;
}

impl OciMachineForwardedAttachmentBackend for ContainerAttachmentBackend {}

/// Stands in for the Krun backend, which implements only the host-managed attachment defaults.
pub(super) struct KrunAttachmentBackend;

impl OciHostManagedAttachmentBackend for KrunAttachmentBackend {
    const ATTACHMENT_BACKEND_KIND: AttachmentBackendKind = AttachmentBackendKind::Krun;
}

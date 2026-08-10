mod capabilities;
pub(crate) mod conmon;
pub mod container;
pub(crate) mod inspection;
pub mod krun;
pub(crate) mod oci;
pub(crate) mod poll;
mod readiness_probe;
#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub mod test_hooks;

pub use capabilities::{
    CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY, KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
    SandboxAttachmentRegistrationError, SandboxNetworkPlanRequirements,
    sandbox_network_plan_requirements,
};
pub use oci::network::{OciNetworkProcess, OciNetworkProcessError};

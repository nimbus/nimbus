mod delivery;
mod dependencies;
mod invalidation;
mod queue;
mod registry;

pub const DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY: usize = 256;

pub use delivery::SubscriptionUpdate;
pub use registry::{SubscriptionCleanupHandle, SubscriptionRegistration, SubscriptionRegistry};

pub(crate) use delivery::{SubscriptionDispatchStats, dispatch_subscription_work};
pub(crate) use dependencies::{SubscriptionBatchCandidate, subscription_dependencies};
pub(crate) use queue::{QueuedSubscriptionWork, merge_queued_subscription_work};

mod authorization;
mod commit_processing;
mod direct;
mod journal;

pub(crate) use authorization::enforce_mutation_authorization;
pub(in crate::engine) use commit_processing::document_bearing_commit_identity;
pub use direct::{AsyncMutationContext, MutationActor};

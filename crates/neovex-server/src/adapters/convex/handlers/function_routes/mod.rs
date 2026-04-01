use super::common::registry_and_auth;
use super::*;

mod actions;
mod mutations;
mod queries;

pub(crate) use actions::action;
pub(crate) use mutations::mutation;
pub(crate) use queries::{paginated_query, query};

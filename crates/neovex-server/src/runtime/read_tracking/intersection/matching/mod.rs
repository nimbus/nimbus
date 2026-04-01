mod filters;
mod windows;

pub(in crate::runtime::read_tracking::intersection) use filters::document_matches_predicate_read;
pub(in crate::runtime::read_tracking) use filters::filters_from_runtime_index_read;
pub(in crate::runtime::read_tracking::intersection) use windows::{
    document_matches_index_read, document_may_affect_paginated_window,
};

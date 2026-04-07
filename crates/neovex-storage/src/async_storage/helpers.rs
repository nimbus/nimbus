use neovex_core::Error;

pub(super) fn map_join_error(error: tokio::task::JoinError) -> Error {
    Error::Internal(format!("blocking storage task failed: {error}"))
}

pub(super) fn map_permit_error(_error: tokio::sync::AcquireError) -> Error {
    Error::Internal("blocking storage permit was closed".to_string())
}

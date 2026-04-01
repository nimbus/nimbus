use axum::http::{HeaderMap, header};

use crate::state::AppError;

pub(in crate::adapters::convex::auth::verifier) fn extract_bearer_token(
    headers: &HeaderMap,
) -> Result<Option<String>, AppError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|error| {
        AppError::unauthorized(format!("invalid authorization header: {error}"))
    })?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or_else(|| AppError::unauthorized("authorization header must use the Bearer scheme"))?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AppError::unauthorized(
            "authorization header must use the Bearer scheme",
        ));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::unauthorized(
            "authorization header is missing a token",
        ));
    }
    Ok(Some(token.to_string()))
}

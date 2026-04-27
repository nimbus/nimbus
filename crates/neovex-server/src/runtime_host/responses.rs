use neovex_core::{Error, Result};
use neovex_runtime::NeovexRuntimeError;
use serde::Serialize;
use serde_json::Value;

use crate::error_envelope::PublicError;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum RuntimeHostResponseEnvelope {
    Ok { value: Value },
    Error { error: Value },
}

impl RuntimeHostResponseEnvelope {
    pub(crate) fn ok(value: Value) -> Self {
        Self::Ok { value }
    }

    pub(crate) fn from_core_error(error: &Error) -> Self {
        let error = serde_json::to_value(PublicError::from_core_error(error)).unwrap_or_else(
            |serialization_error| {
                Value::String(format!(
                    "failed to serialize runtime host error `{error}`: {serialization_error}"
                ))
            },
        );
        Self::Error { error }
    }
}

pub(crate) fn encode_runtime_core_result(
    result: Result<Value>,
) -> std::result::Result<Value, NeovexRuntimeError> {
    match result {
        Ok(value) => {
            serde_json::to_value(RuntimeHostResponseEnvelope::ok(value)).map_err(Into::into)
        }
        Err(Error::Cancelled) => Err(NeovexRuntimeError::Cancelled),
        Err(error) => serde_json::to_value(RuntimeHostResponseEnvelope::from_core_error(&error))
            .map_err(Into::into),
    }
}

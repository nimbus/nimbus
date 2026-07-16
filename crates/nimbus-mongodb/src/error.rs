use super::wire::WireError;

#[derive(Debug, Clone, Copy)]
pub struct MongoErrorCode {
    pub code: i32,
    pub code_name: &'static str,
}

pub const OK: MongoErrorCode = MongoErrorCode {
    code: 0,
    code_name: "OK",
};
pub const BAD_VALUE: MongoErrorCode = MongoErrorCode {
    code: 2,
    code_name: "BadValue",
};
pub const UNAUTHORIZED: MongoErrorCode = MongoErrorCode {
    code: 13,
    code_name: "Unauthorized",
};
pub const AUTHENTICATION_FAILED: MongoErrorCode = MongoErrorCode {
    code: 18,
    code_name: "AuthenticationFailed",
};
pub const NAMESPACE_NOT_FOUND: MongoErrorCode = MongoErrorCode {
    code: 26,
    code_name: "NamespaceNotFound",
};
pub const NAMESPACE_EXISTS: MongoErrorCode = MongoErrorCode {
    code: 48,
    code_name: "NamespaceExists",
};
pub const COMMAND_NOT_FOUND: MongoErrorCode = MongoErrorCode {
    code: 59,
    code_name: "CommandNotFound",
};
pub const COMMAND_NOT_SUPPORTED: MongoErrorCode = MongoErrorCode {
    code: 115,
    code_name: "CommandNotSupported",
};
pub const TOO_MANY_LOGICAL_SESSIONS: MongoErrorCode = MongoErrorCode {
    code: 261,
    code_name: "TooManyLogicalSessions",
};
pub const WRITE_CONFLICT: MongoErrorCode = MongoErrorCode {
    code: 112,
    code_name: "WriteConflict",
};
pub const SNAPSHOT_TOO_OLD: MongoErrorCode = MongoErrorCode {
    code: 239,
    code_name: "SnapshotTooOld",
};
pub const TEMPORARILY_UNAVAILABLE: MongoErrorCode = MongoErrorCode {
    code: 365,
    code_name: "TemporarilyUnavailable",
};
pub const TRANSACTION_TOO_LARGE_FOR_CACHE: MongoErrorCode = MongoErrorCode {
    code: 388,
    code_name: "TransactionTooLargeForCache",
};
pub const RESOURCE_EXHAUSTED: MongoErrorCode = MongoErrorCode {
    code: 402,
    code_name: "ResourceExhausted",
};
pub const RATE_LIMIT_EXCEEDED: MongoErrorCode = MongoErrorCode {
    code: 449,
    code_name: "RateLimitExceeded",
};
pub const DUPLICATE_KEY: MongoErrorCode = MongoErrorCode {
    code: 11000,
    code_name: "DuplicateKey",
};
pub const INTERNAL_ERROR: MongoErrorCode = MongoErrorCode {
    code: 1,
    code_name: "InternalError",
};

#[derive(Debug, thiserror::Error)]
pub enum MongoError {
    #[error("wire error: {0}")]
    Wire(#[from] WireError),
    #[error("{code_name} (code {code}): {message}")]
    Command {
        code: i32,
        code_name: String,
        message: String,
    },
}

impl MongoError {
    pub fn command_not_found(name: &str) -> Self {
        Self::Command {
            code: COMMAND_NOT_FOUND.code,
            code_name: COMMAND_NOT_FOUND.code_name.into(),
            message: format!("no such command: '{name}'"),
        }
    }

    pub fn to_error_doc(&self) -> bson::Document {
        match self {
            Self::Wire(w) => error_doc(BAD_VALUE.code, BAD_VALUE.code_name, &w.to_string()),
            Self::Command {
                code,
                code_name,
                message,
            } => error_doc(*code, code_name, message),
        }
    }
}

impl From<nimbus_core::Error> for MongoError {
    fn from(err: nimbus_core::Error) -> Self {
        let ec = if let Some(class) = err.commit_class() {
            match class {
                nimbus_core::CommitErrorClass::Conflict => WRITE_CONFLICT,
                nimbus_core::CommitErrorClass::Overloaded
                | nimbus_core::CommitErrorClass::CommitterFull
                | nimbus_core::CommitErrorClass::RejectedBeforeExecution => TEMPORARILY_UNAVAILABLE,
                nimbus_core::CommitErrorClass::RateLimited => RATE_LIMIT_EXCEEDED,
                nimbus_core::CommitErrorClass::OutOfRetention => SNAPSHOT_TOO_OLD,
                nimbus_core::CommitErrorClass::CapExceeded => TRANSACTION_TOO_LARGE_FOR_CACHE,
            }
        } else {
            match &err {
                nimbus_core::Error::TenantNotFound(_)
                | nimbus_core::Error::DocumentNotFound(_)
                | nimbus_core::Error::ScheduledJobNotFound(_)
                | nimbus_core::Error::NotFound(_)
                | nimbus_core::Error::SchemaNotFound(_) => NAMESPACE_NOT_FOUND,
                nimbus_core::Error::AlreadyExists(msg) if msg.contains("document") => DUPLICATE_KEY,
                nimbus_core::Error::AlreadyExists(_) => NAMESPACE_EXISTS,
                nimbus_core::Error::InvalidInput(_)
                | nimbus_core::Error::MissingIndex { .. }
                | nimbus_core::Error::SchemaValidation(_)
                | nimbus_core::Error::HistoricalRead { .. }
                | nimbus_core::Error::Serialization(_) => BAD_VALUE,
                nimbus_core::Error::PermissionDenied(_) => UNAUTHORIZED,
                nimbus_core::Error::PreconditionFailed(_) => WRITE_CONFLICT,
                nimbus_core::Error::Transport(_) => TEMPORARILY_UNAVAILABLE,
                nimbus_core::Error::ResourceExhausted(_) => RESOURCE_EXHAUSTED,
                nimbus_core::Error::Storage {
                    kind:
                        nimbus_core::StorageErrorKind::Busy
                        | nimbus_core::StorageErrorKind::Transient
                        | nimbus_core::StorageErrorKind::Unavailable,
                    ..
                } => TEMPORARILY_UNAVAILABLE,
                nimbus_core::Error::Cancelled
                | nimbus_core::Error::Storage { .. }
                | nimbus_core::Error::Internal(_) => INTERNAL_ERROR,
                _ => INTERNAL_ERROR,
            }
        };
        Self::Command {
            code: ec.code,
            code_name: ec.code_name.into(),
            message: err.to_string(),
        }
    }
}

impl From<super::bson_bridge::BridgeError> for MongoError {
    fn from(err: super::bson_bridge::BridgeError) -> Self {
        Self::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: err.to_string(),
        }
    }
}

pub fn ok_doc() -> bson::Document {
    bson::doc! { "ok": 1.0 }
}

pub fn error_doc(code: i32, code_name: &str, errmsg: &str) -> bson::Document {
    bson::doc! {
        "ok": 0.0,
        "errmsg": errmsg,
        "code": code,
        "codeName": code_name,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use nimbus_core::{MutationCap, Retryability};

    fn command_vocabulary(error: nimbus_core::Error) -> (i32, String) {
        match MongoError::from(error) {
            MongoError::Command {
                code, code_name, ..
            } => (code, code_name),
            other => panic!("expected Command, got {other:?}"),
        }
    }

    #[test]
    fn mongodb_surfaces_full_commit_taxonomy() {
        let cases = [
            (
                nimbus_core::Error::retryable_conflict("race", None),
                WRITE_CONFLICT,
                Retryability::Retryable,
            ),
            (
                nimbus_core::Error::overloaded("busy"),
                TEMPORARILY_UNAVAILABLE,
                Retryability::RetryableAfterBackoff,
            ),
            (
                nimbus_core::Error::committer_full("full", 128),
                TEMPORARILY_UNAVAILABLE,
                Retryability::RetryableAfterBackoff,
            ),
            (
                nimbus_core::Error::rejected_before_execution("not started"),
                TEMPORARILY_UNAVAILABLE,
                Retryability::Retryable,
            ),
            (
                nimbus_core::Error::rate_limited("hot", Duration::from_millis(100)),
                RATE_LIMIT_EXCEEDED,
                Retryability::RetryableAfterBackoff,
            ),
            (
                nimbus_core::Error::out_of_retention("expired", None),
                SNAPSHOT_TOO_OLD,
                Retryability::RestartTransaction,
            ),
            (
                nimbus_core::Error::cap_exceeded(MutationCap::DocumentsWritten, 17, 16),
                TRANSACTION_TOO_LARGE_FOR_CACHE,
                Retryability::Terminal,
            ),
        ];

        for (error, expected, retryability) in cases {
            assert_eq!(error.retryability(), retryability, "{error}");
            let (code, code_name) = command_vocabulary(error);
            assert_eq!(code, expected.code);
            assert_eq!(code_name, expected.code_name);
        }
    }

    #[test]
    fn ok_doc_has_ok_field() {
        let doc = ok_doc();
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn error_doc_contains_fields() {
        let doc = error_doc(59, "CommandNotFound", "no such command: 'foo'");
        assert_eq!(doc.get_f64("ok").unwrap(), 0.0);
        assert_eq!(doc.get_str("errmsg").unwrap(), "no such command: 'foo'");
        assert_eq!(doc.get_i32("code").unwrap(), 59);
        assert_eq!(doc.get_str("codeName").unwrap(), "CommandNotFound");
    }

    #[test]
    fn core_not_found_maps_to_namespace_not_found() {
        let core_err =
            nimbus_core::Error::DocumentNotFound(nimbus_core::DocumentId::from_str("abc").unwrap());
        let mongo_err = MongoError::from(core_err);
        match mongo_err {
            MongoError::Command { code, .. } => assert_eq!(code, NAMESPACE_NOT_FOUND.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn core_already_exists_maps_to_namespace_exists() {
        let core_err = nimbus_core::Error::AlreadyExists("test".into());
        let mongo_err = MongoError::from(core_err);
        match mongo_err {
            MongoError::Command { code, .. } => assert_eq!(code, NAMESPACE_EXISTS.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn core_invalid_input_maps_to_bad_value() {
        let core_err = nimbus_core::Error::InvalidInput("bad field".into());
        let mongo_err = MongoError::from(core_err);
        match mongo_err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn core_permission_denied_maps_to_unauthorized() {
        let core_err = nimbus_core::Error::PermissionDenied("no access".into());
        let mongo_err = MongoError::from(core_err);
        match mongo_err {
            MongoError::Command { code, .. } => assert_eq!(code, UNAUTHORIZED.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn core_conflict_maps_to_write_conflict() {
        let core_err = nimbus_core::Error::conflict("concurrent write");
        let mongo_err = MongoError::from(core_err);
        match mongo_err {
            MongoError::Command { code, .. } => assert_eq!(code, WRITE_CONFLICT.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn core_internal_maps_to_internal_error() {
        let core_err = nimbus_core::Error::Internal("something broke".into());
        let mongo_err = MongoError::from(core_err);
        match mongo_err {
            MongoError::Command { code, .. } => assert_eq!(code, INTERNAL_ERROR.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    use std::str::FromStr;
}

use thiserror::Error;

use super::resource_names::FirestoreResourceNameError;
use super::serializer::FirestoreProtoJsonError;

/// The Firestore RPC a [`FirestoreRequestError`] was raised while parsing.
///
/// Each of the nine per-RPC request-error enums this type replaces had the
/// same shape (invalid/unsupported plus the two shared wrapped errors below);
/// `rpc` is what let callers still tell them apart, both for the rendered
/// message and for typed matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirestoreRpc {
    BatchGetDocuments,
    BatchWrite,
    Commit,
    ListCollectionIds,
    RunAggregationQuery,
    RunQuery,
    Transaction,
}

impl FirestoreRpc {
    fn name(self) -> &'static str {
        match self {
            Self::BatchGetDocuments => "BatchGetDocuments",
            Self::BatchWrite => "BatchWrite",
            Self::Commit => "Commit",
            Self::ListCollectionIds => "ListCollectionIds",
            Self::RunAggregationQuery => "RunAggregationQuery",
            Self::RunQuery => "RunQuery",
            Self::Transaction => "Transaction",
        }
    }
}

#[derive(Debug, Error)]
pub enum FirestoreRequestErrorKind {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    Unsupported(String),
    #[error(transparent)]
    InvalidResource(#[from] FirestoreResourceNameError),
    #[error(transparent)]
    InvalidValue(#[from] FirestoreProtoJsonError),
}

/// Unified replacement for the nine per-RPC `FirestoreXxxRequestError` enums.
///
/// Every one of the nine shared the same variant shape (invalid/unsupported
/// text plus the two wrapped shared errors); only the rendered RPC name (and,
/// for `transaction`, an irregular lowercase/extra-word wording) differed.
/// `rpc` carries that context so construction can still reproduce each RPC's
/// exact historical message, and so callers can still match on which RPC
/// failed without parsing the rendered string.
#[derive(Debug, Error)]
#[error("{kind}")]
pub struct FirestoreRequestError {
    pub rpc: FirestoreRpc,
    pub kind: FirestoreRequestErrorKind,
}

impl FirestoreRequestError {
    pub fn invalid_request(rpc: FirestoreRpc, message: impl Into<String>) -> Self {
        let message = message.into();
        let rendered = match rpc {
            FirestoreRpc::Transaction => {
                format!("invalid Firestore transaction request: {message}")
            }
            other => format!("invalid Firestore {} request: {message}", other.name()),
        };
        Self {
            rpc,
            kind: FirestoreRequestErrorKind::InvalidRequest(rendered),
        }
    }

    pub fn unsupported(rpc: FirestoreRpc, feature: impl Into<String>) -> Self {
        let feature = feature.into();
        let rendered = match rpc {
            FirestoreRpc::Transaction => {
                format!("unsupported Firestore transaction request feature: {feature}")
            }
            other => format!("unsupported Firestore {} feature: {feature}", other.name()),
        };
        Self {
            rpc,
            kind: FirestoreRequestErrorKind::Unsupported(rendered),
        }
    }

    pub fn invalid_resource(rpc: FirestoreRpc, error: FirestoreResourceNameError) -> Self {
        Self {
            rpc,
            kind: FirestoreRequestErrorKind::InvalidResource(error),
        }
    }

    pub fn invalid_value(rpc: FirestoreRpc, error: FirestoreProtoJsonError) -> Self {
        Self {
            rpc,
            kind: FirestoreRequestErrorKind::InvalidValue(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_historical_per_rpc_prefixes() {
        // Pins EVERY retired enum's exact Display output (verified against
        // the pre-collapse `#[error("...")]` strings in git history), both
        // forms per RPC. Transaction is the deliberate irregular: lowercase,
        // and "request feature" instead of "feature".
        let invalid: [(FirestoreRpc, &str); 7] = [
            (
                FirestoreRpc::BatchGetDocuments,
                "invalid Firestore BatchGetDocuments request: bad",
            ),
            (
                FirestoreRpc::BatchWrite,
                "invalid Firestore BatchWrite request: bad",
            ),
            (
                FirestoreRpc::Commit,
                "invalid Firestore Commit request: bad",
            ),
            (
                FirestoreRpc::ListCollectionIds,
                "invalid Firestore ListCollectionIds request: bad",
            ),
            (
                FirestoreRpc::RunAggregationQuery,
                "invalid Firestore RunAggregationQuery request: bad",
            ),
            (
                FirestoreRpc::RunQuery,
                "invalid Firestore RunQuery request: bad",
            ),
            (
                FirestoreRpc::Transaction,
                "invalid Firestore transaction request: bad",
            ),
        ];
        for (rpc, expected) in invalid {
            assert_eq!(
                FirestoreRequestError::invalid_request(rpc, "bad").to_string(),
                expected,
                "invalid_request rendering drifted for {rpc:?}"
            );
        }

        let unsupported: [(FirestoreRpc, &str); 7] = [
            (
                FirestoreRpc::BatchGetDocuments,
                "unsupported Firestore BatchGetDocuments feature: `x`",
            ),
            (
                FirestoreRpc::BatchWrite,
                "unsupported Firestore BatchWrite feature: `x`",
            ),
            (
                FirestoreRpc::Commit,
                "unsupported Firestore Commit feature: `x`",
            ),
            (
                FirestoreRpc::ListCollectionIds,
                "unsupported Firestore ListCollectionIds feature: `x`",
            ),
            (
                FirestoreRpc::RunAggregationQuery,
                "unsupported Firestore RunAggregationQuery feature: `x`",
            ),
            (
                FirestoreRpc::RunQuery,
                "unsupported Firestore RunQuery feature: `x`",
            ),
            (
                FirestoreRpc::Transaction,
                "unsupported Firestore transaction request feature: `x`",
            ),
        ];
        for (rpc, expected) in unsupported {
            assert_eq!(
                FirestoreRequestError::unsupported(rpc, "`x`").to_string(),
                expected,
                "unsupported rendering drifted for {rpc:?}"
            );
        }
    }
}

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("invalid base64 transaction bytes: {source}")]
pub struct FirestoreTransactionTokenError {
    #[source]
    source: base64::DecodeError,
}

pub(crate) fn decode(value: &str) -> Result<Vec<u8>, FirestoreTransactionTokenError> {
    BASE64_STANDARD
        .decode(value)
        .map_err(|source| FirestoreTransactionTokenError { source })
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn decodes_standard_base64_transaction_tokens() {
        assert_eq!(decode("AQID").expect("token should decode"), vec![1, 2, 3]);
    }

    #[test]
    fn rejects_invalid_transaction_tokens_with_base64_context() {
        let error = decode("!not-base64!").expect_err("invalid token should fail");

        assert!(
            error
                .to_string()
                .contains("invalid base64 transaction bytes")
        );
    }
}

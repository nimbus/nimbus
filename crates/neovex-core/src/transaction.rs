use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, Timestamp};

/// Protocol-neutral transaction mode for a server-owned session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionSessionMode {
    ReadOnly,
    ReadWrite,
}

/// Opaque transaction token handed back to transports between RPCs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TransactionSessionToken(String);

impl TransactionSessionToken {
    /// Creates a validated transaction token wrapper.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        value.into().try_into()
    }

    /// Returns the token as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for TransactionSessionToken {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TransactionSessionToken {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for TransactionSessionToken {
    type Error = Error;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        validate_transaction_session_token(&value)?;
        Ok(Self(value))
    }
}

impl From<TransactionSessionToken> for String {
    fn from(value: TransactionSessionToken) -> Self {
        value.0
    }
}

/// Metadata for an active server-owned transaction session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSession {
    pub token: TransactionSessionToken,
    pub mode: TransactionSessionMode,
    pub started_at: Timestamp,
    pub expires_at: Timestamp,
}

fn validate_transaction_session_token(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(
            "transaction session token cannot be empty".to_string(),
        ));
    }
    if value.len() > 256 {
        return Err(Error::InvalidInput(
            "transaction session token cannot exceed 256 characters".to_string(),
        ));
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Ok(());
    }

    Err(Error::InvalidInput(
        "transaction session token may only contain ASCII letters, numbers, `_`, and `-`"
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{TransactionSession, TransactionSessionMode, TransactionSessionToken};
    use crate::Timestamp;

    #[test]
    fn transaction_session_token_round_trips() {
        let token = TransactionSessionToken::new("txn_abc123_DEF").expect("token should validate");
        let session = TransactionSession {
            token: token.clone(),
            mode: TransactionSessionMode::ReadWrite,
            started_at: Timestamp(100),
            expires_at: Timestamp(500),
        };

        let encoded = serde_json::to_value(&session).expect("session should serialize");
        assert_eq!(encoded["token"], json!("txn_abc123_DEF"));
        assert_eq!(encoded["mode"], json!("read_write"));

        let decoded: TransactionSession =
            serde_json::from_value(encoded).expect("session should deserialize");
        assert_eq!(decoded, session);
        assert_eq!(token.as_str(), "txn_abc123_DEF");
    }

    #[test]
    fn transaction_session_token_rejects_invalid_values() {
        let empty = TransactionSessionToken::new("");
        let spaced = TransactionSessionToken::new("txn invalid");
        let unicode = TransactionSessionToken::new("txn_日本語");

        assert!(matches!(empty, Err(crate::Error::InvalidInput(_))));
        assert!(matches!(spaced, Err(crate::Error::InvalidInput(_))));
        assert!(matches!(unicode, Err(crate::Error::InvalidInput(_))));
    }
}

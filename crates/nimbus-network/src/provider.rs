use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::NetworkProviderId;

const MAX_PROVIDER_HANDLE_BYTES: usize = 4_096;

/// Opaque, provider-scoped durable handle for one realized network resource.
///
/// Serialization retains the opaque value for durable reconciliation. `Debug`
/// and `Display` always redact it so routine structured diagnostics cannot leak
/// provider internals or credentials.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "NetworkProviderHandleWire",
    into = "NetworkProviderHandleWire"
)]
pub struct NetworkProviderHandle {
    provider_id: NetworkProviderId,
    opaque_value: String,
}

impl NetworkProviderHandle {
    /// Construct a bounded provider handle suitable for durable storage.
    pub fn new(
        provider_id: NetworkProviderId,
        opaque_value: impl Into<String>,
    ) -> Result<Self, NetworkProviderHandleError> {
        let opaque_value = opaque_value.into();
        if opaque_value.is_empty() {
            return Err(NetworkProviderHandleError::Empty);
        }
        if opaque_value.len() > MAX_PROVIDER_HANDLE_BYTES {
            return Err(NetworkProviderHandleError::TooLong {
                max_bytes: MAX_PROVIDER_HANDLE_BYTES,
            });
        }
        if opaque_value.chars().any(char::is_control) {
            return Err(NetworkProviderHandleError::ControlCharacter);
        }
        Ok(Self {
            provider_id,
            opaque_value,
        })
    }

    /// Provider registration that owns interpretation of the opaque value.
    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }

    /// Reveal the opaque value to the owning provider adapter.
    ///
    /// Callers must not copy this value into logs, projections, or identity.
    pub fn expose_to_provider(&self) -> &str {
        &self.opaque_value
    }
}

impl fmt::Debug for NetworkProviderHandle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NetworkProviderHandle")
            .field("provider_id", &self.provider_id)
            .field("opaque_value", &"<redacted>")
            .finish()
    }
}

impl Display for NetworkProviderHandle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:<redacted>", self.provider_id)
    }
}

/// Exclusive durable compensation authority for one network launch reservation.
///
/// Exact resource identity is idempotent only for the coordinator carrying
/// this attempt-unique claim. The claim does not authorize provider effects;
/// it prevents another coordinator from treating shared `Reserved` state as
/// authority it may adopt or compensate across attachment and port resources.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkReservationClaim {
    coordinator_attempt: NetworkProviderHandle,
}

impl NetworkReservationClaim {
    /// Wrap one attempt-unique, coordinator-scoped opaque identity.
    pub fn new(coordinator_attempt: NetworkProviderHandle) -> Self {
        Self {
            coordinator_attempt,
        }
    }

    /// Opaque attempt identity interpreted only by the coordinating adapter.
    pub fn coordinator_attempt(&self) -> &NetworkProviderHandle {
        &self.coordinator_attempt
    }
}

impl fmt::Debug for NetworkReservationClaim {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NetworkReservationClaim")
            .field("coordinator_attempt", &self.coordinator_attempt)
            .finish()
    }
}

/// Stable validation failure for an opaque provider handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkProviderHandleError {
    /// Provider handles may not be empty.
    Empty,
    /// Provider handles are bounded to keep durable records and diagnostics
    /// finite.
    TooLong { max_bytes: usize },
    /// Control characters are refused to prevent multiline/log injection and
    /// ambiguous wire values.
    ControlCharacter,
}

impl Display for NetworkProviderHandleError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("network provider handle must not be empty"),
            Self::TooLong { max_bytes } => write!(
                formatter,
                "network provider handle must not exceed {max_bytes} bytes"
            ),
            Self::ControlCharacter => {
                formatter.write_str("network provider handle must not contain control characters")
            }
        }
    }
}

impl StdError for NetworkProviderHandleError {}

#[derive(Serialize, Deserialize)]
struct NetworkProviderHandleWire {
    provider_id: NetworkProviderId,
    opaque_value: String,
}

impl TryFrom<NetworkProviderHandleWire> for NetworkProviderHandle {
    type Error = NetworkProviderHandleError;

    fn try_from(value: NetworkProviderHandleWire) -> Result<Self, Self::Error> {
        Self::new(value.provider_id, value.opaque_value)
    }
}

impl From<NetworkProviderHandle> for NetworkProviderHandleWire {
    fn from(value: NetworkProviderHandle) -> Self {
        Self {
            provider_id: value.provider_id,
            opaque_value: value.opaque_value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_id() -> NetworkProviderId {
        "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse()
            .expect("fixture provider id should parse")
    }

    #[test]
    fn diagnostics_redact_but_durable_wire_round_trips() {
        let handle = NetworkProviderHandle::new(provider_id(), "secret/provider/handle")
            .expect("provider handle should validate");

        assert_eq!(
            handle.expose_to_provider(),
            "secret/provider/handle",
            "only the explicit provider accessor reveals the opaque value"
        );
        assert!(!format!("{handle:?}").contains("secret/provider/handle"));
        assert!(!handle.to_string().contains("secret/provider/handle"));

        let json = serde_json::to_string(&handle).expect("handle should serialize durably");
        assert_eq!(
            json,
            r#"{"provider_id":"netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV","opaque_value":"secret/provider/handle"}"#
        );
        assert_eq!(
            serde_json::from_str::<NetworkProviderHandle>(&json)
                .expect("handle should deserialize"),
            handle
        );

        let claim = NetworkReservationClaim::new(handle);
        assert!(
            !format!("{claim:?}").contains("secret/provider/handle"),
            "shared attachment/port claim diagnostics must retain handle redaction"
        );
        let claim_json =
            serde_json::to_string(&claim).expect("claim should serialize as durable authority");
        assert!(
            claim_json.contains("secret/provider/handle"),
            "durable claim wire must retain the opaque coordinator identity"
        );
        assert_eq!(
            serde_json::from_str::<NetworkReservationClaim>(&claim_json)
                .expect("claim should deserialize"),
            claim
        );
    }

    #[test]
    fn invalid_handle_shapes_fail_closed() {
        assert_eq!(
            NetworkProviderHandle::new(provider_id(), ""),
            Err(NetworkProviderHandleError::Empty)
        );
        assert_eq!(
            NetworkProviderHandle::new(provider_id(), "line\nbreak"),
            Err(NetworkProviderHandleError::ControlCharacter)
        );
        assert_eq!(
            NetworkProviderHandle::new(provider_id(), "x".repeat(MAX_PROVIDER_HANDLE_BYTES + 1),),
            Err(NetworkProviderHandleError::TooLong {
                max_bytes: MAX_PROVIDER_HANDLE_BYTES,
            })
        );

        let invalid_wire = format!(r#"{{"provider_id":"{}","opaque_value":""}}"#, provider_id());
        assert!(
            serde_json::from_str::<NetworkProviderHandle>(&invalid_wire)
                .expect_err("invalid durable handle must fail")
                .to_string()
                .contains("must not be empty")
        );
    }
}

use serde_json::{Map, Value};

use nimbus_auth::ApplicationAuthError;
use nimbus_core::{SystemWallClock, WallClock};

use super::super::CLOCK_SKEW;
use super::models::ParsedClaims;

pub fn validate_temporal_claims(claims: &ParsedClaims) -> Result<(), ApplicationAuthError> {
    validate_temporal_claims_with_clock(claims, &SystemWallClock)
}

fn validate_temporal_claims_with_clock(
    claims: &ParsedClaims,
    clock: &dyn WallClock,
) -> Result<(), ApplicationAuthError> {
    validate_temporal_claims_at(claims, clock.now_secs())
}

fn validate_temporal_claims_at(
    claims: &ParsedClaims,
    now: u64,
) -> Result<(), ApplicationAuthError> {
    let now_with_skew = now.saturating_add(CLOCK_SKEW.as_secs());
    let now_without_skew = now.saturating_sub(CLOCK_SKEW.as_secs());
    if let Some(not_before) = claims.not_before
        && not_before > now_with_skew
    {
        return Err(ApplicationAuthError::unauthorized(
            "auth token is not valid yet",
        ));
    }
    let expires_at = claims
        .expires_at
        .ok_or_else(|| ApplicationAuthError::unauthorized("auth token is missing an exp claim"))?;
    if expires_at <= now_without_skew {
        return Err(ApplicationAuthError::unauthorized("auth token has expired"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use nimbus_core::Timestamp;
    use serde_json::json;

    use super::*;

    fn claims(not_before: Option<u64>, expires_at: Option<u64>) -> ParsedClaims {
        serde_json::from_value(json!({
            "iss": "https://issuer.example",
            "sub": "subject",
            "aud": "nimbus",
            "nbf": not_before,
            "exp": expires_at,
        }))
        .expect("claims should deserialize")
    }

    #[test]
    fn jwt_not_before_accepts_exact_positive_skew_edge() {
        let now = 10_000;
        validate_temporal_claims_at(
            &claims(Some(now + CLOCK_SKEW.as_secs()), Some(now + 1)),
            now,
        )
        .expect("not-before at the positive skew edge should be accepted");
    }

    #[test]
    fn jwt_expiry_rejects_exact_negative_skew_edge() {
        let now = 10_000;
        let error =
            validate_temporal_claims_at(&claims(None, Some(now - CLOCK_SKEW.as_secs())), now)
                .expect_err("expiry at the negative skew edge should be rejected");
        assert!(error.to_string().contains("expired"));
    }

    struct CountingWallClock {
        samples: AtomicU64,
        now: Timestamp,
    }

    impl WallClock for CountingWallClock {
        fn now(&self) -> Timestamp {
            self.samples.fetch_add(1, Ordering::Relaxed);
            self.now
        }
    }

    #[test]
    fn jwt_temporal_validation_samples_now_once() {
        let clock = CountingWallClock {
            samples: AtomicU64::new(0),
            now: Timestamp(10_000_000),
        };

        validate_temporal_claims_with_clock(&claims(Some(9_000), Some(11_000)), &clock)
            .expect("claims should validate at the single sampled observation");

        assert_eq!(clock.samples.load(Ordering::Relaxed), 1);
    }
}

pub fn strip_known_identity_claims(claims: &mut Map<String, Value>) {
    for key in [
        "iss",
        "sub",
        "aud",
        "exp",
        "nbf",
        "iat",
        "jti",
        "name",
        "given_name",
        "family_name",
        "nickname",
        "preferred_username",
        "profile",
        "picture",
        "email",
        "email_verified",
        "gender",
        "birthdate",
        "zoneinfo",
        "locale",
        "phone_number",
        "phone_number_verified",
        "address",
        "updated_at",
        "tokenIdentifier",
    ] {
        claims.remove(key);
    }
}

pub fn extract_custom_jwt_claims(raw_claims: &Map<String, Value>) -> Map<String, Value> {
    let mut claims = Map::new();
    for (key, value) in raw_claims {
        if matches!(
            key.as_str(),
            "iss" | "sub" | "aud" | "exp" | "nbf" | "iat" | "jti"
        ) {
            continue;
        }
        flatten_custom_jwt_claim(&mut claims, key, value);
    }
    claims
}

fn flatten_custom_jwt_claim(claims: &mut Map<String, Value>, key: &str, value: &Value) {
    if let Value::Object(object) = value {
        for (nested_key, nested_value) in object {
            flatten_custom_jwt_claim(claims, &format!("{key}.{nested_key}"), nested_value);
        }
    } else {
        claims.insert(key.to_string(), value.clone());
    }
}

pub fn extract_address_claim(value: Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value),
        Value::Object(object) => object
            .get("formatted")
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}

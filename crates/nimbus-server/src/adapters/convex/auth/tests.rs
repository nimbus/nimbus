use nimbus_runtime::VerifiedUserIdentityKind;
use serde_json::{Map, Value, json};

use super::jwt::ParsedClaims;

#[test]
fn custom_jwt_convex_projection_stays_narrower_than_verified_identity() {
    let raw_claims = serde_json::from_value::<Map<String, Value>>(json!({
        "iss": "https://issuer.example.com",
        "sub": "user-123",
        "aud": "nimbus-test",
        "exp": 2_000_000_000u64,
        "email": "ada@example.com",
        "given_name": "Ada",
        "updated_at": 1710000000,
        "address": {
            "formatted": "123 Analytical Engine Way"
        },
        "profile": "https://example.com/ada",
        "role": "admin",
        "nested": {
            "team": "platform"
        }
    }))
    .expect("raw custom jwt claims should parse");
    let claims: ParsedClaims = serde_json::from_value(Value::Object(raw_claims.clone()))
        .expect("parsed claims should deserialize");

    let verified = claims
        .clone()
        .into_verified_identity(VerifiedUserIdentityKind::CustomJwt);
    let convex = claims.into_convex_custom_jwt_identity(&raw_claims);

    assert_eq!(verified.email.as_deref(), Some("ada@example.com"));
    assert_eq!(verified.given_name.as_deref(), Some("Ada"));
    assert_eq!(
        verified.address.as_deref(),
        Some("123 Analytical Engine Way")
    );
    assert_eq!(verified.updated_at.as_deref(), Some("1710000000"));
    assert_eq!(
        verified.profile_url.as_deref(),
        Some("https://example.com/ada")
    );

    assert_eq!(convex.email, None);
    assert_eq!(convex.given_name, None);
    assert_eq!(convex.address, None);
    assert_eq!(convex.updated_at, None);
    assert_eq!(convex.profile_url, None);
    assert_eq!(
        convex.custom_claims.get("email"),
        Some(&json!("ada@example.com"))
    );
    assert_eq!(convex.custom_claims.get("given_name"), Some(&json!("Ada")));
    assert_eq!(
        convex.custom_claims.get("updated_at"),
        Some(&json!(1710000000))
    );
    assert_eq!(
        convex.custom_claims.get("address.formatted"),
        Some(&json!("123 Analytical Engine Way"))
    );
    assert_eq!(convex.custom_claims.get("address"), None);
    assert_eq!(
        convex.custom_claims.get("profile"),
        Some(&json!("https://example.com/ada"))
    );
    assert_eq!(
        convex.custom_claims.get("nested.team"),
        Some(&json!("platform"))
    );
}

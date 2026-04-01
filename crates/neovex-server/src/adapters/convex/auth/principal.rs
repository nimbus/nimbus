use neovex_core::PrincipalContext;
use neovex_runtime::InvocationAuth;
use serde::Serialize;
use serde_json::{Map, Value};

pub(in crate::adapters::convex) fn normalize_principal_context(
    auth: Option<&InvocationAuth>,
) -> PrincipalContext {
    let Some(auth) = auth else {
        return PrincipalContext::anonymous();
    };

    PrincipalContext {
        authenticated: auth.identity.is_some() || auth.verified_identity.is_some(),
        claims: auth.identity.as_ref().map(claims_map).unwrap_or_default(),
        verified_claims: auth
            .verified_identity
            .as_ref()
            .map(claims_map)
            .unwrap_or_default(),
    }
}

fn claims_map<T>(value: &T) -> Map<String, Value>
where
    T: Serialize,
{
    match serde_json::to_value(value).expect("principal claims should serialize") {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

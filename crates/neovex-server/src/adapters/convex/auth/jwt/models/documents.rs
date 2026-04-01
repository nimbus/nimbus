use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex::auth) struct OidcDiscoveryDocument {
    pub(in crate::adapters::convex::auth) issuer: String,
    pub(in crate::adapters::convex::auth) jwks_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex::auth) struct JsonWebKeySet {
    pub(in crate::adapters::convex::auth::jwt) keys: Vec<JsonWebKey>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex::auth) struct JsonWebKey {
    pub(in crate::adapters::convex::auth) kid: Option<String>,
    pub(in crate::adapters::convex::auth) kty: String,
    pub(in crate::adapters::convex::auth) alg: Option<String>,
    #[serde(rename = "use")]
    pub(in crate::adapters::convex::auth) use_: Option<String>,
    pub(in crate::adapters::convex::auth) n: Option<String>,
    pub(in crate::adapters::convex::auth) e: Option<String>,
    pub(in crate::adapters::convex::auth) crv: Option<String>,
    pub(in crate::adapters::convex::auth) x: Option<String>,
    pub(in crate::adapters::convex::auth) y: Option<String>,
}

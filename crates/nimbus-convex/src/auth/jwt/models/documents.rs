use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct OidcDiscoveryDocument {
    pub issuer: String,
    pub jwks_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonWebKeySet {
    pub keys: Vec<JsonWebKey>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonWebKey {
    pub kid: Option<String>,
    pub kty: String,
    pub alg: Option<String>,
    #[serde(rename = "use")]
    pub use_: Option<String>,
    pub n: Option<String>,
    pub e: Option<String>,
    pub crv: Option<String>,
    pub x: Option<String>,
    pub y: Option<String>,
}

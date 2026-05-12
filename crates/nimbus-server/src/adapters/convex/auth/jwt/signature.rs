use ring::signature;

use crate::state::AppError;

use super::models::{JsonWebKey, JsonWebKeySet, JwtHeader, ParsedJwt, ParsedJwtAlgorithm};
use super::parsing::decode_base64_url;

pub(in crate::adapters::convex::auth) fn select_jwk<'a>(
    jwks: &'a JsonWebKeySet,
    header: &JwtHeader,
) -> Result<&'a JsonWebKey, AppError> {
    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| AppError::unauthorized("auth token is missing a JWT kid header"))?;
    jwks.keys
        .iter()
        .find(|key| {
            key.kid.as_deref() == Some(kid)
                && key.use_.as_deref().is_none_or(|use_| use_ == "sig")
                && key
                    .alg
                    .as_deref()
                    .is_none_or(|alg| alg == header.algorithm.as_str())
        })
        .ok_or_else(|| {
            AppError::unauthorized("auth token kid did not match any configured JWKS signing key")
        })
}

pub(in crate::adapters::convex::auth) fn verify_signature(
    parsed: &ParsedJwt,
    jwk: &JsonWebKey,
) -> Result<(), AppError> {
    match parsed.header.algorithm {
        ParsedJwtAlgorithm::RS256 => verify_rsa_signature(parsed, jwk),
        ParsedJwtAlgorithm::ES256 => verify_es256_signature(parsed, jwk),
        ParsedJwtAlgorithm::EdDsa => verify_ed25519_signature(parsed, jwk),
    }
}

fn verify_rsa_signature(parsed: &ParsedJwt, jwk: &JsonWebKey) -> Result<(), AppError> {
    if jwk.kty != "RSA" {
        return Err(AppError::unauthorized("JWKS key type did not match RS256"));
    }
    let modulus = decode_base64_url(
        jwk.n
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("RSA JWKS key is missing modulus"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid RSA modulus: {error}")))?;
    let exponent = decode_base64_url(
        jwk.e
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("RSA JWKS key is missing exponent"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid RSA exponent: {error}")))?;
    let key = signature::RsaPublicKeyComponents {
        n: &modulus,
        e: &exponent,
    };
    key.verify(
        &signature::RSA_PKCS1_2048_8192_SHA256,
        parsed.signing_input.as_bytes(),
        &parsed.signature,
    )
    .map_err(|_| AppError::unauthorized("invalid JWT signature"))
}

fn verify_es256_signature(parsed: &ParsedJwt, jwk: &JsonWebKey) -> Result<(), AppError> {
    if jwk.kty != "EC" {
        return Err(AppError::unauthorized("JWKS key type did not match ES256"));
    }
    if jwk.crv.as_deref() != Some("P-256") {
        return Err(AppError::unauthorized("ES256 requires a P-256 JWKS key"));
    }
    let x = decode_base64_url(
        jwk.x
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("EC JWKS key is missing x coordinate"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid EC x coordinate: {error}")))?;
    let y = decode_base64_url(
        jwk.y
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("EC JWKS key is missing y coordinate"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid EC y coordinate: {error}")))?;
    let mut public_key = Vec::with_capacity(1 + x.len() + y.len());
    public_key.push(0x04);
    public_key.extend_from_slice(&x);
    public_key.extend_from_slice(&y);
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key)
        .verify(parsed.signing_input.as_bytes(), &parsed.signature)
        .map_err(|_| AppError::unauthorized("invalid JWT signature"))
}

fn verify_ed25519_signature(parsed: &ParsedJwt, jwk: &JsonWebKey) -> Result<(), AppError> {
    if jwk.kty != "OKP" {
        return Err(AppError::unauthorized("JWKS key type did not match EdDSA"));
    }
    if jwk.crv.as_deref() != Some("Ed25519") {
        return Err(AppError::unauthorized("EdDSA requires an Ed25519 JWKS key"));
    }
    let public_key = decode_base64_url(
        jwk.x
            .as_deref()
            .ok_or_else(|| AppError::unauthorized("Ed25519 JWKS key is missing x coordinate"))?,
    )
    .map_err(|error| AppError::unauthorized(format!("invalid Ed25519 public key: {error}")))?;
    signature::UnparsedPublicKey::new(&signature::ED25519, public_key)
        .verify(parsed.signing_input.as_bytes(), &parsed.signature)
        .map_err(|_| AppError::unauthorized("invalid JWT signature"))
}

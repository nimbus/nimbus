//! JOSE compact serialization for SI3 local-development OIDC JWTs.
//!
//! The SI plan prefers `jsonwebtoken` for minting; SI3 instead assembles
//! the JOSE compact serialization directly and signs THROUGH the
//! `IdentitySigner` seam, because handing private-key material to
//! jsonwebtoken's `EncodingKey` would bypass the seam SI2 built (file
//! locking, rotation, stale-key denial, and the future FIPS/HS1 signer
//! swap). This is ~30 lines of assembly, not a re-implementation of JWT
//! parsing/verification — the convex-verifier warning is about
//! verification infrastructure, which SI3 does not build. Verification-side
//! code shipped later (SI5) still prefers jsonwebtoken/openidconnect.
//! Independent verification in tests uses ring's `UnparsedPublicKey`
//! (already in-tree; the same primitive the JWS spec requires).

use std::borrow::Cow;
use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use nimbus_crypto::{IdentitySigner, SigningError};
use serde::Serialize;

use crate::CredentialClaims;
use crate::registration::workload_subject_to_spiffe_id;

#[derive(Debug, Serialize)]
pub(crate) struct JoseHeader<'a> {
    alg: &'static str,
    typ: &'static str,
    kid: &'a str,
}

impl<'a> JoseHeader<'a> {
    fn new(kid: &'a str) -> Self {
        Self {
            alg: "EdDSA",
            typ: "JWT",
            kid,
        }
    }
}

#[derive(Debug, Serialize)]
struct JwtPayload<'a> {
    iss: &'a str,
    sub: Cow<'a, str>,
    aud: JwtAudience<'a>,
    exp: u64,
    iat: u64,
    jti: &'a str,
    nimbus_decision_id: &'a str,
    nimbus_workload_subject: &'a str,
    nimbus_workload_audit_projection: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    nimbus_node_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nimbus_machine_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nimbus_sandbox_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nimbus_invocation_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum JwtAudience<'a> {
    Single(&'a str),
    Array(Vec<&'a str>),
}

impl<'a> JwtPayload<'a> {
    fn oidc(issuer: &'a str, claims: &'a CredentialClaims) -> Self {
        // JWT payloads omit absent placement claims because providers commonly
        // reject explicit nulls. The internal/audit serialization of
        // `CredentialClaims` deliberately keeps those nulls for a stable audit
        // shape, and keeps `nimbus_issued_at_ms` internal while this payload
        // exposes RFC 7519 NumericDate `iat` seconds.
        Self::from_parts(
            issuer,
            Cow::Borrowed(claims.sub()),
            JwtAudience::Single(claims.aud()),
            claims,
        )
    }

    fn jwt_svid(trust_domain: &'a str, claims: &'a CredentialClaims) -> Result<Self, JwtMintError> {
        let subject =
            workload_subject_to_spiffe_id(trust_domain, claims.sub()).ok_or_else(|| {
                JwtMintError::InvalidWorkloadSubject {
                    subject: claims.sub().to_string(),
                }
            })?;
        Ok(Self::from_parts(
            trust_domain,
            Cow::Owned(subject),
            JwtAudience::Array(vec![claims.aud()]),
            claims,
        ))
    }

    fn from_parts(
        issuer: &'a str,
        subject: Cow<'a, str>,
        audience: JwtAudience<'a>,
        claims: &'a CredentialClaims,
    ) -> Self {
        Self {
            iss: issuer,
            sub: subject,
            aud: audience,
            exp: claims.exp_epoch_ms() / 1000,
            iat: claims.iat_epoch_ms() / 1000,
            jti: claims.jti(),
            nimbus_decision_id: claims.nimbus_decision_id(),
            nimbus_workload_subject: claims.nimbus_workload_subject(),
            nimbus_workload_audit_projection: claims.nimbus_workload_audit_projection(),
            nimbus_node_id: claims.nimbus_node_id(),
            nimbus_machine_id: claims.nimbus_machine_id(),
            nimbus_sandbox_id: claims.nimbus_sandbox_id(),
            nimbus_invocation_id: claims.nimbus_invocation_id(),
        }
    }
}

pub(crate) fn mint_oidc_jwt(
    issuer: &str,
    claims: &CredentialClaims,
    signer: &dyn IdentitySigner,
) -> Result<String, JwtMintError> {
    let payload = JwtPayload::oidc(issuer, claims);
    mint_compact_jwt(&payload, signer)
}

pub(crate) fn mint_jwt_svid(
    trust_domain: &str,
    claims: &CredentialClaims,
    signer: &dyn IdentitySigner,
) -> Result<String, JwtMintError> {
    let payload = JwtPayload::jwt_svid(trust_domain, claims)?;
    mint_compact_jwt(&payload, signer)
}

fn mint_compact_jwt(
    payload: &JwtPayload<'_>,
    signer: &dyn IdentitySigner,
) -> Result<String, JwtMintError> {
    let public_key = signer.public_key();
    let kid = public_key.fingerprint();
    let header = JoseHeader::new(&kid);

    let header = encode_json(&header)?;
    let payload = encode_json(payload)?;
    let signing_input = format!("{header}.{payload}");
    let signature = signer
        .sign(signing_input.as_bytes())
        .map_err(JwtMintError::Sign)?;
    if signature.key_id() != kid {
        return Err(JwtMintError::SignatureKeyMismatch);
    }
    let signature = URL_SAFE_NO_PAD.encode(signature.signature_bytes());
    Ok(format!("{signing_input}.{signature}"))
}

fn encode_json(value: &impl Serialize) -> Result<String, JwtMintError> {
    serde_json::to_vec(value)
        .map(|json| URL_SAFE_NO_PAD.encode(json))
        .map_err(JwtMintError::Serialize)
}

#[derive(Debug)]
pub(crate) enum JwtMintError {
    Serialize(serde_json::Error),
    Sign(SigningError),
    SignatureKeyMismatch,
    InvalidWorkloadSubject { subject: String },
}

impl JwtMintError {
    pub(crate) fn tenant_safe_message(&self) -> String {
        match self {
            Self::Serialize(_) => "JWT serialization failed".to_string(),
            Self::Sign(_) => "signer failed to produce identity signature".to_string(),
            Self::SignatureKeyMismatch => {
                "signer returned identity signature for an unexpected key".to_string()
            }
            Self::InvalidWorkloadSubject { .. } => {
                "workload subject could not be rendered as a SPIFFE JWT-SVID subject".to_string()
            }
        }
    }
}

impl fmt::Display for JwtMintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(_) => write!(formatter, "JWT serialization failed"),
            Self::Sign(_) => write!(formatter, "signer failed to produce identity signature"),
            Self::SignatureKeyMismatch => {
                write!(
                    formatter,
                    "signer returned identity signature for an unexpected key"
                )
            }
            Self::InvalidWorkloadSubject { subject } => {
                write!(
                    formatter,
                    "workload subject `{subject}` could not be rendered as a SPIFFE JWT-SVID subject"
                )
            }
        }
    }
}

impl std::error::Error for JwtMintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialize(error) => Some(error),
            Self::Sign(error) => Some(error),
            Self::SignatureKeyMismatch | Self::InvalidWorkloadSubject { .. } => None,
        }
    }
}

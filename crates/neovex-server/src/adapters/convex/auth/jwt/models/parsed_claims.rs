use neovex_runtime::{RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::super::claims::{
    extract_address_claim, extract_custom_jwt_claims, strip_known_identity_claims,
};

#[derive(Debug, Clone, Deserialize)]
pub(in crate::adapters::convex::auth) struct ParsedClaims {
    #[serde(rename = "iss")]
    pub(in crate::adapters::convex::auth) issuer: String,
    #[serde(rename = "sub")]
    pub(in crate::adapters::convex::auth) subject: String,
    #[serde(
        rename = "aud",
        default,
        deserialize_with = "super::super::parsing::deserialize_audiences"
    )]
    pub(in crate::adapters::convex::auth) audiences: Vec<String>,
    #[serde(rename = "exp")]
    pub(in crate::adapters::convex::auth) expires_at: Option<u64>,
    #[serde(rename = "nbf")]
    pub(in crate::adapters::convex::auth) not_before: Option<u64>,
    name: Option<String>,
    #[serde(rename = "given_name")]
    given_name: Option<String>,
    #[serde(rename = "family_name")]
    family_name: Option<String>,
    nickname: Option<String>,
    #[serde(rename = "preferred_username")]
    preferred_username: Option<String>,
    #[serde(rename = "profile")]
    profile_url: Option<String>,
    #[serde(rename = "picture")]
    picture_url: Option<String>,
    email: Option<String>,
    #[serde(rename = "email_verified")]
    email_verified: Option<bool>,
    gender: Option<String>,
    #[serde(rename = "birthdate")]
    birthday: Option<String>,
    #[serde(rename = "zoneinfo")]
    timezone: Option<String>,
    #[serde(rename = "locale")]
    language: Option<String>,
    #[serde(rename = "phone_number")]
    phone_number: Option<String>,
    #[serde(rename = "phone_number_verified")]
    phone_number_verified: Option<bool>,
    address: Option<Value>,
    #[serde(rename = "updated_at")]
    updated_at: Option<Value>,
    #[serde(flatten)]
    other_claims: Map<String, Value>,
}

impl ParsedClaims {
    pub(in crate::adapters::convex::auth) fn into_verified_identity(
        mut self,
        kind: VerifiedUserIdentityKind,
    ) -> VerifiedUserIdentity {
        strip_known_identity_claims(&mut self.other_claims);
        VerifiedUserIdentity {
            kind,
            token_identifier: format!("{}|{}", self.issuer, self.subject),
            subject: self.subject,
            issuer: self.issuer,
            name: self.name,
            given_name: self.given_name,
            family_name: self.family_name,
            nickname: self.nickname,
            preferred_username: self.preferred_username,
            profile_url: self.profile_url,
            picture_url: self.picture_url,
            email: self.email,
            email_verified: self.email_verified,
            gender: self.gender,
            birthday: self.birthday,
            timezone: self.timezone,
            language: self.language,
            phone_number: self.phone_number,
            phone_number_verified: self.phone_number_verified,
            address: self.address.and_then(extract_address_claim),
            updated_at: self.updated_at.map(stringify_identity_value),
            custom_claims: self.other_claims,
        }
    }

    pub(in crate::adapters::convex::auth) fn into_convex_oidc_identity(
        mut self,
    ) -> RuntimeUserIdentity {
        strip_known_identity_claims(&mut self.other_claims);
        RuntimeUserIdentity {
            token_identifier: format!("{}|{}", self.issuer, self.subject),
            subject: self.subject,
            issuer: self.issuer,
            name: self.name,
            given_name: self.given_name,
            family_name: self.family_name,
            nickname: self.nickname,
            preferred_username: self.preferred_username,
            profile_url: self.profile_url,
            picture_url: self.picture_url,
            email: self.email,
            email_verified: self.email_verified,
            gender: self.gender,
            birthday: self.birthday,
            timezone: self.timezone,
            language: self.language,
            phone_number: self.phone_number,
            phone_number_verified: self.phone_number_verified,
            address: self.address.and_then(extract_address_claim),
            updated_at: self.updated_at.map(stringify_identity_value),
            custom_claims: self.other_claims,
        }
    }

    pub(in crate::adapters::convex::auth) fn into_convex_custom_jwt_identity(
        self,
        raw_claims: &Map<String, Value>,
    ) -> RuntimeUserIdentity {
        RuntimeUserIdentity {
            token_identifier: format!("{}|{}", self.issuer, self.subject),
            subject: self.subject,
            issuer: self.issuer,
            name: None,
            given_name: None,
            family_name: None,
            nickname: None,
            preferred_username: None,
            profile_url: None,
            picture_url: None,
            email: None,
            email_verified: None,
            gender: None,
            birthday: None,
            timezone: None,
            language: None,
            phone_number: None,
            phone_number_verified: None,
            address: None,
            updated_at: None,
            custom_claims: extract_custom_jwt_claims(raw_claims),
        }
    }
}

fn stringify_identity_value(value: Value) -> String {
    match value {
        Value::String(value) => value,
        Value::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

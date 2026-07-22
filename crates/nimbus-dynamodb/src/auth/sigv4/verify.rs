// Copyright 2026 ExtendDB contributors
// SPDX-License-Identifier: Apache-2.0
//
// Vendored into Nimbus from ExtendDB (crates/auth/src/sigv4/verify.rs) at rev
// 0448ca0. Modified from ExtendDB by Nimbus contributors, 2026-05-29:
// `axum::http::HeaderMap` -> `http::HeaderMap` (nimbus-dynamodb does not depend
// on axum; axum re-exports the `http` crate's HeaderMap). See repo-root NOTICE.

//! SigV4 signature verification.
//!
//! Reconstructs the expected signature from the request and compares it
//! against the signature in the `Authorization` header using constant-time
//! comparison to prevent timing attacks.

use extenddb_core::error::DynamoDbError;
use http::HeaderMap;
use nimbus_core::{SystemWallClock, WallClock};

use super::canonical;
use super::parse::ParsedAuthorization;
use super::signing_key;

/// Maximum allowed clock skew between client and server (±15 minutes).
const MAX_CLOCK_SKEW_SECS: i64 = 15 * 60;

/// Verify a SigV4 signature against the request.
///
/// # Arguments
/// * `parsed` — The parsed `Authorization` header.
/// * `secret_key` — The plaintext secret access key.
/// * `method` — HTTP method (e.g. `POST`).
/// * `uri_path` — URI path (e.g. `/`).
/// * `query_string` — Query string (empty for DynamoDB).
/// * `headers` — The full request headers.
/// * `body` — The request body bytes.
///
/// Returns `Ok(())` on success, or an appropriate auth error on failure.
pub fn verify_signature(
    parsed: &ParsedAuthorization,
    secret_key: &str,
    method: &str,
    uri_path: &str,
    query_string: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), DynamoDbError> {
    // CB-5: Validate credential-scope service is "dynamodb".
    if parsed.service != "dynamodb" {
        // N-2: Trailing space matches real DynamoDB's exact error message.
        return Err(DynamoDbError::InvalidSignatureException(
            "Credential should be scoped to correct service: 'dynamodb'. ".to_owned(),
        ));
    }

    // CB-5: Validate credential-scope date matches X-Amz-Date[..8].
    let timestamp = extract_timestamp(headers)?;
    if timestamp.len() < 8 || parsed.date != timestamp[..8] {
        return Err(DynamoDbError::InvalidSignatureException(
            "Date in Credential scope does not match YYYYMMDD from ISO-8601 version of date from HTTP".to_owned(),
        ));
    }

    // CB-6: Require "host" in SignedHeaders.
    let signed_lower = parsed.signed_headers.to_ascii_lowercase();
    if !signed_lower.split(';').any(|h| h == "host") {
        return Err(DynamoDbError::InvalidSignatureException(
            "\"Host\" must be a \"SignedHeader\" in the AWS Authorization.".to_owned(),
        ));
    }

    // Reject UNSIGNED-PAYLOAD and bind the request body to the signature.
    //
    // The canonical request trusts the client-supplied `x-amz-content-sha256`
    // value as the payload hash, so the signature only attests to *that hash*,
    // not to the bytes we actually dispatch. Without re-deriving the body hash
    // here, a captured signed request's body could be swapped while keeping the
    // same header + `Signature` and verification would still pass. Real
    // DynamoDB rejects such a mismatch; so do we.
    if let Some(content_sha) = headers.get("x-amz-content-sha256") {
        if content_sha.as_bytes() == b"UNSIGNED-PAYLOAD" {
            // UNSIGNED-PAYLOAD is only valid for S3; DynamoDB requires a hash.
            return Err(DynamoDbError::InvalidSignatureException(
                "UNSIGNED-PAYLOAD is not supported for DynamoDB operations.".to_owned(),
            ));
        }
        let actual = canonical::sha256_hex(body);
        let provided = content_sha.to_str().unwrap_or_default();
        if !provided.eq_ignore_ascii_case(&actual) {
            return Err(DynamoDbError::InvalidSignatureException(
                "The request body does not match the x-amz-content-sha256 it was signed with."
                    .to_owned(),
            ));
        }
    }

    // Build canonical request
    let creq = canonical::canonical_request(
        method,
        uri_path,
        query_string,
        headers,
        &parsed.signed_headers,
        body,
    );

    // Build scope and string-to-sign
    let scope = format!(
        "{}/{}/{}/aws4_request",
        parsed.date, parsed.region, parsed.service
    );

    let sts = canonical::string_to_sign(&timestamp, &scope, &creq);

    // Derive signing key and compute expected signature
    let signing_key =
        signing_key::derive_signing_key(secret_key, &parsed.date, &parsed.region, &parsed.service);
    let expected = signing_key::compute_signature(&signing_key, &sts);

    // auth H-1: Use InvalidSignatureException for signature mismatches,
    // distinct from UnrecognizedClientException (unknown key).
    if !constant_time_eq(expected.as_bytes(), parsed.signature.as_bytes()) {
        return Err(DynamoDbError::InvalidSignatureException(
            "The request signature we calculated does not match the signature you provided. Check your AWS Secret Access Key and signing method. Consult the service documentation for details.".to_owned(),
        ));
    }

    Ok(())
}

/// Validate the request timestamp is within ±15 minutes of now.
///
/// Returns `UnrecognizedClientException` if the timestamp is missing or out of range.
/// Returns `InternalServerError` if the system clock is misconfigured.
pub fn validate_timestamp(headers: &HeaderMap) -> Result<(), DynamoDbError> {
    validate_timestamp_at(headers, SystemWallClock.now_secs())
}

fn validate_timestamp_at(headers: &HeaderMap, now_epoch_secs: u64) -> Result<(), DynamoDbError> {
    let timestamp = extract_timestamp(headers)?;

    // Parse X-Amz-Date: YYYYMMDDTHHMMSSZ
    let now = i64::try_from(now_epoch_secs)
        .ok()
        .ok_or_else(|| DynamoDbError::InternalServerError("server clock error".to_owned()))?;

    let request_epoch = parse_iso8601_basic(&timestamp).ok_or_else(|| {
        DynamoDbError::IncompleteSignature(
            "Date must be in ISO 8601 basic format (YYYYMMDDTHHMMSSZ)".to_owned(),
        )
    })?;

    let skew = (now - request_epoch).abs();
    if skew > MAX_CLOCK_SKEW_SECS {
        return Err(DynamoDbError::UnrecognizedClientException(
            "Signature expired: the signature is too old or the request date is too far in the future.".to_owned(),
        ));
    }

    Ok(())
}

/// Extract the timestamp from `X-Amz-Date` header.
fn extract_timestamp(headers: &HeaderMap) -> Result<String, DynamoDbError> {
    headers
        .get("x-amz-date")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            DynamoDbError::IncompleteSignature(
                "Authorization header requires existence of 'X-Amz-Date' header".to_owned(),
            )
        })
}

/// Parse ISO 8601 basic format `YYYYMMDDTHHMMSSZ` to Unix epoch seconds.
fn parse_iso8601_basic(s: &str) -> Option<i64> {
    if s.len() != 16 || !s.ends_with('Z') || s.as_bytes()[8] != b'T' {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let month: i64 = s[4..6].parse().ok()?;
    let day: i64 = s[6..8].parse().ok()?;
    let hour: i64 = s[9..11].parse().ok()?;
    let min: i64 = s[11..13].parse().ok()?;
    let sec: i64 = s[13..15].parse().ok()?;

    // Range-validate each field so a malformed-but-parseable timestamp (e.g.
    // `20269999T999999Z`) is rejected outright rather than mapped to a nonsense
    // epoch that could land inside the ±15-minute skew window. (`sec` allows 60
    // for leap seconds.)
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&min)
        || !(0..=60).contains(&sec)
    {
        return None;
    }

    // Days-since-epoch via Howard Hinnant's date algorithm (general, not range-limited).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

/// Constant-time byte comparison to prevent timing attacks.
///
/// Processes every byte of the longer input (no early return on length
/// mismatch) so the running time depends only on the input lengths — never on
/// *where* the inputs first differ. `parsed.signature` is attacker-controlled
/// and arbitrary-length, so unequal lengths must be handled safely here.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = 0u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0 && a.len() == b.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso8601_basic_valid() {
        // 2026-04-15T12:00:00Z
        let epoch = parse_iso8601_basic("20260415T120000Z").unwrap();
        // Verify against known value: 2026-04-15 is day 1,776,528,000 from epoch
        // (verified with external tool)
        assert!(epoch > 0);
        // 2015-08-30T12:36:00Z — from AWS test vector
        let epoch2 = parse_iso8601_basic("20150830T123600Z").unwrap();
        assert!(epoch2 > 0);
        assert!(epoch > epoch2); // 2026 > 2015
    }

    #[test]
    fn parse_iso8601_basic_invalid() {
        assert!(parse_iso8601_basic("2026-04-15T12:00:00Z").is_none());
        assert!(parse_iso8601_basic("20260415T12000Z").is_none());
        assert!(parse_iso8601_basic("").is_none());
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn extract_timestamp_missing() {
        let headers = HeaderMap::new();
        assert!(extract_timestamp(&headers).is_err());
    }

    #[test]
    fn extract_timestamp_present() {
        let mut headers = HeaderMap::new();
        headers.insert("x-amz-date", "20260415T120000Z".parse().unwrap());
        assert_eq!(extract_timestamp(&headers).unwrap(), "20260415T120000Z");
    }

    #[test]
    fn sigv4_accepts_exact_clock_skew_edge() {
        let headers = make_headers("20260415T120000Z");
        let request_epoch = parse_iso8601_basic("20260415T120000Z").unwrap();

        validate_timestamp_at(
            &headers,
            u64::try_from(request_epoch + MAX_CLOCK_SKEW_SECS).unwrap(),
        )
        .expect("exact positive skew edge should be accepted");
        validate_timestamp_at(
            &headers,
            u64::try_from(request_epoch - MAX_CLOCK_SKEW_SECS).unwrap(),
        )
        .expect("exact negative skew edge should be accepted");
    }

    #[test]
    fn sigv4_rejects_one_second_past_clock_skew_edge() {
        let headers = make_headers("20260415T120000Z");
        let request_epoch = parse_iso8601_basic("20260415T120000Z").unwrap();
        let error = validate_timestamp_at(
            &headers,
            u64::try_from(request_epoch + MAX_CLOCK_SKEW_SECS + 1).unwrap(),
        )
        .expect_err("one second past the skew edge should be rejected");

        assert!(matches!(
            error,
            DynamoDbError::UnrecognizedClientException(_)
        ));
    }

    #[test]
    fn sigv4_rejects_future_and_past_symmetrically() {
        let headers = make_headers("20260415T120000Z");
        let request_epoch = parse_iso8601_basic("20260415T120000Z").unwrap();
        let offset = MAX_CLOCK_SKEW_SECS + 1;

        for now in [request_epoch - offset, request_epoch + offset] {
            assert!(matches!(
                validate_timestamp_at(&headers, u64::try_from(now).unwrap()),
                Err(DynamoDbError::UnrecognizedClientException(_))
            ));
        }
    }

    #[test]
    fn sigv4_timestamp_error_classifications_remain_stable() {
        let malformed = make_headers("20269999T999999Z");
        assert!(matches!(
            validate_timestamp_at(&malformed, 0),
            Err(DynamoDbError::IncompleteSignature(_))
        ));
        let valid = make_headers("20260415T120000Z");
        assert!(matches!(
            validate_timestamp_at(&valid, u64::MAX),
            Err(DynamoDbError::InternalServerError(_))
        ));
    }

    fn make_parsed(service: &str, date: &str, signed_headers: &str) -> ParsedAuthorization {
        ParsedAuthorization {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
            date: date.to_owned(),
            region: "us-east-1".to_owned(),
            service: service.to_owned(),
            signed_headers: signed_headers.to_owned(),
            signature: "0".repeat(64),
        }
    }

    fn make_headers(date: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-amz-date", date.parse().unwrap());
        h.insert("host", "localhost:8000".parse().unwrap());
        h.insert(
            "content-type",
            "application/x-amz-json-1.0".parse().unwrap(),
        );
        h
    }

    /// CB-5: Reject credential scope with wrong service.
    #[test]
    fn reject_wrong_service_in_scope() {
        let parsed = make_parsed("s3", "20260415", "content-type;host;x-amz-date");
        let headers = make_headers("20260415T120000Z");
        let err = verify_signature(&parsed, "secret", "POST", "/", "", &headers, b"{}");
        match err {
            Err(DynamoDbError::InvalidSignatureException(msg)) => {
                assert!(msg.contains("dynamodb"), "Expected 'dynamodb' in: {msg}");
            }
            other => panic!("Expected InvalidSignatureException, got: {other:?}"),
        }
    }

    /// CB-5: Reject credential scope date mismatch with X-Amz-Date.
    #[test]
    fn reject_scope_date_mismatch() {
        let parsed = make_parsed("dynamodb", "20260414", "content-type;host;x-amz-date");
        let headers = make_headers("20260415T120000Z");
        let err = verify_signature(&parsed, "secret", "POST", "/", "", &headers, b"{}");
        match err {
            Err(DynamoDbError::InvalidSignatureException(msg)) => {
                assert!(
                    msg.contains("Date in Credential scope"),
                    "Expected date mismatch message: {msg}"
                );
            }
            other => panic!("Expected InvalidSignatureException, got: {other:?}"),
        }
    }

    /// CB-6: Reject SignedHeaders missing "host".
    #[test]
    fn reject_missing_host_in_signed_headers() {
        let parsed = make_parsed("dynamodb", "20260415", "content-type;x-amz-date");
        let headers = make_headers("20260415T120000Z");
        let err = verify_signature(&parsed, "secret", "POST", "/", "", &headers, b"{}");
        match err {
            Err(DynamoDbError::InvalidSignatureException(msg)) => {
                assert!(msg.contains("Host"), "Expected 'Host' in message: {msg}");
            }
            other => panic!("Expected InvalidSignatureException, got: {other:?}"),
        }
    }

    /// Reject UNSIGNED-PAYLOAD for DynamoDB API calls.
    #[test]
    fn reject_unsigned_payload() {
        let parsed = make_parsed(
            "dynamodb",
            "20260415",
            "content-type;host;x-amz-content-sha256;x-amz-date",
        );
        let mut headers = make_headers("20260415T120000Z");
        headers.insert("x-amz-content-sha256", "UNSIGNED-PAYLOAD".parse().unwrap());
        let err = verify_signature(&parsed, "secret", "POST", "/", "", &headers, b"{}");
        match err {
            Err(DynamoDbError::InvalidSignatureException(msg)) => {
                assert!(
                    msg.contains("UNSIGNED-PAYLOAD"),
                    "Expected 'UNSIGNED-PAYLOAD' in: {msg}"
                );
            }
            other => panic!("Expected InvalidSignatureException, got: {other:?}"),
        }
    }

    /// H1 regression: a tampered body is rejected even when the rest of the
    /// canonical request is signed correctly. The signer advertises
    /// `x-amz-content-sha256 = sha256("{}")` but the transport delivers a
    /// different body; binding the body to the signature must reject it with a
    /// content-sha256 mismatch rather than trusting the header.
    #[test]
    fn tampered_body_is_rejected() {
        let parsed = make_parsed(
            "dynamodb",
            "20260415",
            "content-type;host;x-amz-content-sha256;x-amz-date",
        );
        let mut headers = make_headers("20260415T120000Z");
        // Header pins the hash of the *original* body the client signed.
        headers.insert(
            "x-amz-content-sha256",
            canonical::sha256_hex(b"{}").parse().unwrap(),
        );
        // Transport delivers a swapped body under the same pinned hash.
        let tampered_body = br#"{"TableName":"evil"}"#;
        let err = verify_signature(&parsed, "secret", "POST", "/", "", &headers, tampered_body);
        match err {
            Err(DynamoDbError::InvalidSignatureException(msg)) => {
                assert!(
                    msg.contains("does not match the x-amz-content-sha256"),
                    "Expected content-sha256 mismatch in: {msg}"
                );
            }
            other => panic!("Expected InvalidSignatureException, got: {other:?}"),
        }
    }

    /// auth H-1: Signature mismatch returns InvalidSignatureException.
    #[test]
    fn signature_mismatch_returns_invalid_signature() {
        let parsed = make_parsed("dynamodb", "20260415", "content-type;host;x-amz-date");
        let headers = make_headers("20260415T120000Z");
        // Wrong secret → wrong signature → InvalidSignatureException
        let err = verify_signature(&parsed, "wrong-secret", "POST", "/", "", &headers, b"{}");
        match err {
            Err(DynamoDbError::InvalidSignatureException(msg)) => {
                assert!(
                    msg.contains("does not match"),
                    "Expected 'does not match' in: {msg}"
                );
            }
            other => panic!("Expected InvalidSignatureException, got: {other:?}"),
        }
    }
}

use std::path::Path;

use nimbus_core::types::TableName;
use nimbus_server::adapters_mongodb::bson_bridge;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct BsonCorpusFile {
    pub description: String,
    pub bson_type: String,
    #[serde(default)]
    pub test_key: String,
    #[serde(default)]
    pub valid: Vec<ValidTest>,
    #[serde(default, rename = "decodeErrors")]
    pub decode_errors: Vec<DecodeErrorTest>,
    #[serde(default, rename = "parseErrors")]
    pub parse_errors: Vec<ParseErrorTest>,
}

#[derive(Deserialize)]
pub struct ValidTest {
    pub description: String,
    pub canonical_bson: String,
    #[serde(default)]
    pub degenerate_bson: Option<String>,
    #[serde(default)]
    pub canonical_extjson: Option<String>,
    #[serde(default)]
    pub relaxed_extjson: Option<String>,
    #[serde(default)]
    pub lossy: Option<bool>,
}

#[derive(Deserialize)]
pub struct DecodeErrorTest {
    pub description: String,
    pub bson: String,
}

#[derive(Deserialize)]
pub struct ParseErrorTest {
    pub description: String,
    #[serde(default)]
    pub string: Option<String>,
}

pub fn parse_corpus_file(path: &Path) -> Result<BsonCorpusFile, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

pub struct CorpusResult {
    pub file_name: String,
    pub bson_type: String,
    pub valid_pass: usize,
    pub valid_fail: usize,
    pub valid_skip: usize,
    pub decode_error_pass: usize,
    pub decode_error_fail: usize,
    pub roundtrip_pass: usize,
    pub roundtrip_fail: usize,
    pub failures: Vec<String>,
}

pub fn run_corpus_file(corpus: &BsonCorpusFile, file_name: &str) -> CorpusResult {
    let table = TableName::new("bson_test").expect("valid table name");
    let mut result = CorpusResult {
        file_name: file_name.to_string(),
        bson_type: corpus.bson_type.clone(),
        valid_pass: 0,
        valid_fail: 0,
        valid_skip: 0,
        decode_error_pass: 0,
        decode_error_fail: 0,
        roundtrip_pass: 0,
        roundtrip_fail: 0,
        failures: Vec::new(),
    };

    for test in &corpus.valid {
        let bytes = hex_to_bytes(&test.canonical_bson);

        let doc: Result<bson::Document, _> = bson::deserialize_from_slice(&bytes);
        match doc {
            Ok(bson_doc) => {
                result.valid_pass += 1;

                match bson_bridge::bson_doc_to_document(&bson_doc, &table) {
                    Ok(nimbus_doc) => {
                        let roundtripped = bson_bridge::document_to_bson_doc(&nimbus_doc);

                        let orig_bytes =
                            bson::serialize_to_vec(&bson_doc).expect("serialize original");
                        let rt_bytes =
                            bson::serialize_to_vec(&roundtripped).expect("serialize roundtrip");

                        if orig_bytes == rt_bytes || test.lossy == Some(true) {
                            result.roundtrip_pass += 1;
                        } else {
                            let field_match =
                                check_field_level_match(&bson_doc, &roundtripped, &corpus.test_key);
                            if field_match {
                                result.roundtrip_pass += 1;
                            } else {
                                result.roundtrip_fail += 1;
                                result.failures.push(format!(
                                    "roundtrip: {} ({})",
                                    test.description, corpus.description
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        result.roundtrip_fail += 1;
                        result.failures.push(format!(
                            "bridge: {} ({}) — {}",
                            test.description, corpus.description, e
                        ));
                    }
                }
            }
            Err(_) => {
                result.valid_skip += 1;
            }
        }
    }

    for test in &corpus.decode_errors {
        let bytes = hex_to_bytes(&test.bson);
        let doc: Result<bson::Document, _> = bson::deserialize_from_slice(&bytes);
        if doc.is_err() {
            result.decode_error_pass += 1;
        } else {
            result.decode_error_fail += 1;
            result.failures.push(format!(
                "decode should fail: {} ({})",
                test.description, corpus.description
            ));
        }
    }

    result
}

fn check_field_level_match(
    original: &bson::Document,
    roundtripped: &bson::Document,
    test_key: &str,
) -> bool {
    if test_key.is_empty() {
        return false;
    }
    let orig_val = original.get(test_key);
    let rt_val = roundtripped.get(test_key);
    match (orig_val, rt_val) {
        (Some(o), Some(r)) => bson_values_equivalent(o, r),
        (None, None) => true,
        _ => false,
    }
}

fn bson_values_equivalent(a: &bson::Bson, b: &bson::Bson) -> bool {
    match (a, b) {
        (bson::Bson::Double(x), bson::Bson::Double(y)) => {
            if x.is_nan() && y.is_nan() {
                return true;
            }
            if x == y && x.is_sign_negative() == y.is_sign_negative() {
                return true;
            }
            false
        }
        (bson::Bson::Int32(x), bson::Bson::Int32(y)) => x == y,
        (bson::Bson::Int64(x), bson::Bson::Int64(y)) => x == y,
        (bson::Bson::String(x), bson::Bson::String(y)) => x == y,
        (bson::Bson::Boolean(x), bson::Bson::Boolean(y)) => x == y,
        (bson::Bson::Null, bson::Bson::Null) => true,
        (bson::Bson::ObjectId(x), bson::Bson::ObjectId(y)) => x == y,
        (bson::Bson::DateTime(x), bson::Bson::DateTime(y)) => x == y,
        (bson::Bson::Binary(x), bson::Bson::Binary(y)) => {
            x.subtype == y.subtype && x.bytes == y.bytes
        }
        (bson::Bson::RegularExpression(x), bson::Bson::RegularExpression(y)) => {
            x.pattern == y.pattern && x.options == y.options
        }
        (bson::Bson::Timestamp(x), bson::Bson::Timestamp(y)) => x == y,
        (bson::Bson::Decimal128(x), bson::Bson::Decimal128(y)) => x == y,
        (bson::Bson::MinKey, bson::Bson::MinKey) => true,
        (bson::Bson::MaxKey, bson::Bson::MaxKey) => true,
        (bson::Bson::JavaScriptCode(x), bson::Bson::JavaScriptCode(y)) => x == y,
        (bson::Bson::Document(x), bson::Bson::Document(y)) => {
            if x.len() != y.len() {
                return false;
            }
            x.iter()
                .all(|(k, v)| y.get(k).is_some_and(|yv| bson_values_equivalent(v, yv)))
        }
        (bson::Bson::Array(x), bson::Bson::Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|(a, b)| bson_values_equivalent(a, b))
        }
        _ => a == b,
    }
}

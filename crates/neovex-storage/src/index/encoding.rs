use neovex_core::Result;
use serde_json::Value;

/// Encodes a scalar JSON value to bytes that preserve lexicographic order.
pub fn encode_index_value(value: &Value) -> Result<Vec<u8>> {
    match value {
        Value::Null => Ok(vec![0x00]),
        Value::Bool(false) => Ok(vec![0x01, 0x00]),
        Value::Bool(true) => Ok(vec![0x01, 0x01]),
        Value::Number(number) => {
            let float = number.as_f64().ok_or_else(|| {
                neovex_core::Error::InvalidInput("unsupported numeric index value".to_string())
            })?;
            let mut bytes = float.to_bits().to_be_bytes();
            if float.is_sign_positive() || float == 0.0 {
                bytes[0] ^= 0x80;
            } else {
                for byte in &mut bytes {
                    *byte = !*byte;
                }
            }
            let mut encoded = vec![0x02];
            encoded.extend_from_slice(&bytes);
            Ok(encoded)
        }
        Value::String(string) => {
            let mut encoded = Vec::with_capacity(2 + string.len());
            encoded.push(0x03);
            for byte in string.as_bytes() {
                match byte {
                    0x00 => encoded.extend_from_slice(&[0x00, 0xFF]),
                    other => encoded.push(*other),
                }
            }
            encoded.extend_from_slice(&[0x00, 0x00]);
            Ok(encoded)
        }
        _ => Err(neovex_core::Error::InvalidInput(
            "only null, boolean, number, and string fields are indexable in phase 2".to_string(),
        )),
    }
}

/// Encodes an ordered tuple of scalar JSON values for composite index scans.
pub fn encode_index_tuple(values: &[Value]) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    for value in values {
        encoded.extend_from_slice(&encode_index_value(value)?);
    }
    Ok(encoded)
}

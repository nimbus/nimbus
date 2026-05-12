use std::cmp::Ordering as CompareOrdering;
use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use nimbus_core::{Filter, FilterOp};
use rmp::Marker;
use rmp::decode::{read_array_len, read_map_len, read_marker, read_str_len};
use serde_json::Value;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScanStats {
    pub scanned_rows: u64,
    pub decoded_rows: u64,
    pub pushdown_rejected_rows: u64,
}

pub(super) struct ScanMetrics {
    scanned_rows: AtomicU64,
    decoded_rows: AtomicU64,
    pushdown_rejected_rows: AtomicU64,
}

impl ScanMetrics {
    pub(super) fn new() -> Self {
        Self {
            scanned_rows: AtomicU64::new(0),
            decoded_rows: AtomicU64::new(0),
            pushdown_rejected_rows: AtomicU64::new(0),
        }
    }

    pub(super) fn record_scanned_row(&self) {
        self.scanned_rows.fetch_add(1, AtomicOrdering::Relaxed);
    }

    pub(super) fn record_decoded_row(&self) {
        self.decoded_rows.fetch_add(1, AtomicOrdering::Relaxed);
    }

    pub(super) fn record_pushdown_rejected_row(&self) {
        self.pushdown_rejected_rows
            .fetch_add(1, AtomicOrdering::Relaxed);
    }

    #[cfg(test)]
    pub(super) fn stats(&self) -> ScanStats {
        ScanStats {
            scanned_rows: self.scanned_rows.load(AtomicOrdering::Relaxed),
            decoded_rows: self.decoded_rows.load(AtomicOrdering::Relaxed),
            pushdown_rejected_rows: self.pushdown_rejected_rows.load(AtomicOrdering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ScanPushdown {
    filters: Vec<SupportedPushdownFilter>,
}

#[derive(Debug, Clone)]
struct SupportedPushdownFilter {
    field: String,
    op: FilterOp,
    value: Value,
}

impl ScanPushdown {
    pub(super) fn compile(filters: &[Filter]) -> Option<Self> {
        let filters = filters
            .iter()
            .filter_map(|filter| match filter.op {
                FilterOp::Eq if is_scalar_value(&filter.value) => Some(SupportedPushdownFilter {
                    field: filter.field.clone(),
                    op: filter.op,
                    value: filter.value.clone(),
                }),
                FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte
                    if is_range_comparable_value(&filter.value) =>
                {
                    Some(SupportedPushdownFilter {
                        field: filter.field.clone(),
                        op: filter.op,
                        value: filter.value.clone(),
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if filters.is_empty() {
            None
        } else {
            Some(Self { filters })
        }
    }

    pub(super) fn rejects_document_bytes(&self, bytes: &[u8]) -> bool {
        let target_fields = self
            .filters
            .iter()
            .map(|filter| filter.field.as_str())
            .collect::<Vec<_>>();
        let Some(probed_fields) = probe_document_fields_from_msgpack(bytes, &target_fields) else {
            return false;
        };

        self.filters.iter().any(|filter| {
            let Some(field_value) = probed_fields.get(filter.field.as_str()) else {
                return true;
            };
            match filter.op {
                FilterOp::Eq => field_value != &filter.value,
                FilterOp::Gt => match compare_pushdown_values(field_value, &filter.value) {
                    Some(CompareOrdering::Greater) => false,
                    Some(_) => true,
                    None => false,
                },
                FilterOp::Gte => match compare_pushdown_values(field_value, &filter.value) {
                    Some(CompareOrdering::Greater | CompareOrdering::Equal) => false,
                    Some(CompareOrdering::Less) => true,
                    None => false,
                },
                FilterOp::Lt => match compare_pushdown_values(field_value, &filter.value) {
                    Some(CompareOrdering::Less) => false,
                    Some(_) => true,
                    None => false,
                },
                FilterOp::Lte => match compare_pushdown_values(field_value, &filter.value) {
                    Some(CompareOrdering::Less | CompareOrdering::Equal) => false,
                    Some(CompareOrdering::Greater) => true,
                    None => false,
                },
                FilterOp::Neq => false,
            }
        })
    }
}

fn is_scalar_value(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Number(_)
    )
}

fn is_range_comparable_value(value: &Value) -> bool {
    matches!(value, Value::String(_))
        || matches!(value, Value::Number(number) if number.as_f64().is_some())
}

fn compare_pushdown_values(left: &Value, right: &Value) -> Option<CompareOrdering> {
    match (left, right) {
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => {
            let left = left.as_f64()?;
            let right = right.as_f64()?;
            left.partial_cmp(&right)
        }
        _ => None,
    }
}

fn probe_document_fields_from_msgpack(
    bytes: &[u8],
    fields: &[&str],
) -> Option<serde_json::Map<String, Value>> {
    if fields.is_empty() {
        return Some(serde_json::Map::new());
    }

    let mut cursor = Cursor::new(bytes);
    let array_len = read_array_len(&mut cursor).ok()?;
    if array_len != 5 && array_len != 6 {
        return None;
    }

    skip_msgpack_value(&mut cursor).ok()?;
    skip_msgpack_value(&mut cursor).ok()?;
    skip_msgpack_value(&mut cursor).ok()?;
    skip_msgpack_value(&mut cursor).ok()?;

    let field_count = read_map_len(&mut cursor).ok()?;
    let mut remaining = fields.len();
    let mut collected = serde_json::Map::with_capacity(fields.len());
    for _ in 0..field_count {
        let key = read_msgpack_string(&mut cursor).ok()?;
        if fields.contains(&key.as_str()) {
            let value: Value = rmp_serde::from_read(&mut cursor).ok()?;
            if collected.insert(key, value).is_none() {
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    break;
                }
            }
        } else {
            skip_msgpack_value(&mut cursor).ok()?;
        }
    }

    Some(collected)
}

fn read_msgpack_string(cursor: &mut Cursor<&[u8]>) -> std::io::Result<String> {
    let len = read_str_len(cursor).map_err(map_value_read_error)?;
    let mut bytes = vec![0_u8; len as usize];
    cursor.read_exact(&mut bytes)?;
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn skip_msgpack_value(cursor: &mut Cursor<&[u8]>) -> std::io::Result<()> {
    match read_marker(cursor).map_err(map_marker_read_error)? {
        Marker::Null | Marker::True | Marker::False | Marker::FixPos(_) | Marker::FixNeg(_) => {
            Ok(())
        }
        Marker::U8 | Marker::I8 => skip_bytes(cursor, 1),
        Marker::U16 | Marker::I16 => skip_bytes(cursor, 2),
        Marker::U32 | Marker::I32 | Marker::F32 => skip_bytes(cursor, 4),
        Marker::U64 | Marker::I64 | Marker::F64 => skip_bytes(cursor, 8),
        Marker::FixStr(len) => skip_bytes(cursor, u64::from(len)),
        Marker::Str8 => skip_sized_bytes(cursor, 1),
        Marker::Str16 => skip_sized_bytes(cursor, 2),
        Marker::Str32 => skip_sized_bytes(cursor, 4),
        Marker::Bin8 => skip_sized_bytes(cursor, 1),
        Marker::Bin16 => skip_sized_bytes(cursor, 2),
        Marker::Bin32 => skip_sized_bytes(cursor, 4),
        Marker::FixArray(len) => skip_msgpack_array(cursor, u32::from(len)),
        Marker::Array16 => {
            let len = u32::from(read_u16(cursor)?);
            skip_msgpack_array(cursor, len)
        }
        Marker::Array32 => {
            let len = read_u32(cursor)?;
            skip_msgpack_array(cursor, len)
        }
        Marker::FixMap(len) => skip_msgpack_map(cursor, u32::from(len)),
        Marker::Map16 => {
            let len = u32::from(read_u16(cursor)?);
            skip_msgpack_map(cursor, len)
        }
        Marker::Map32 => {
            let len = read_u32(cursor)?;
            skip_msgpack_map(cursor, len)
        }
        Marker::FixExt1 => skip_bytes(cursor, 2),
        Marker::FixExt2 => skip_bytes(cursor, 3),
        Marker::FixExt4 => skip_bytes(cursor, 5),
        Marker::FixExt8 => skip_bytes(cursor, 9),
        Marker::FixExt16 => skip_bytes(cursor, 17),
        Marker::Ext8 => {
            let len = u64::from(read_u8(cursor)?);
            skip_ext(cursor, len)
        }
        Marker::Ext16 => {
            let len = u64::from(read_u16(cursor)?);
            skip_ext(cursor, len)
        }
        Marker::Ext32 => {
            let len = u64::from(read_u32(cursor)?);
            skip_ext(cursor, len)
        }
        Marker::Reserved => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "reserved MessagePack marker",
        )),
    }
}

fn skip_msgpack_array(cursor: &mut Cursor<&[u8]>, len: u32) -> std::io::Result<()> {
    for _ in 0..len {
        skip_msgpack_value(cursor)?;
    }
    Ok(())
}

fn skip_msgpack_map(cursor: &mut Cursor<&[u8]>, len: u32) -> std::io::Result<()> {
    for _ in 0..len {
        skip_msgpack_value(cursor)?;
        skip_msgpack_value(cursor)?;
    }
    Ok(())
}

fn skip_sized_bytes(cursor: &mut Cursor<&[u8]>, size_len: usize) -> std::io::Result<()> {
    let len = match size_len {
        1 => u64::from(read_u8(cursor)?),
        2 => u64::from(read_u16(cursor)?),
        4 => u64::from(read_u32(cursor)?),
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsupported MessagePack size prefix width",
            ));
        }
    };
    skip_bytes(cursor, len)
}

fn skip_ext(cursor: &mut Cursor<&[u8]>, len: u64) -> std::io::Result<()> {
    skip_bytes(cursor, len.saturating_add(1))
}

fn skip_bytes(cursor: &mut Cursor<&[u8]>, len: u64) -> std::io::Result<()> {
    let current = cursor.position();
    let end = current.saturating_add(len);
    if end > cursor.get_ref().len() as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "unexpected end of MessagePack value",
        ));
    }
    cursor.set_position(end);
    Ok(())
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> std::io::Result<u8> {
    let mut value = [0_u8; 1];
    cursor.read_exact(&mut value)?;
    Ok(value[0])
}

fn read_u16(cursor: &mut Cursor<&[u8]>) -> std::io::Result<u16> {
    let mut value = [0_u8; 2];
    cursor.read_exact(&mut value)?;
    Ok(u16::from_be_bytes(value))
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> std::io::Result<u32> {
    let mut value = [0_u8; 4];
    cursor.read_exact(&mut value)?;
    Ok(u32::from_be_bytes(value))
}

fn map_marker_read_error(error: rmp::decode::MarkerReadError<std::io::Error>) -> std::io::Error {
    error.0
}

fn map_value_read_error(error: rmp::decode::ValueReadError<std::io::Error>) -> std::io::Error {
    match error {
        rmp::decode::ValueReadError::InvalidMarkerRead(error)
        | rmp::decode::ValueReadError::InvalidDataRead(error) => error,
        rmp::decode::ValueReadError::TypeMismatch(marker) => std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unexpected MessagePack marker while reading string: {marker:?}"),
        ),
    }
}

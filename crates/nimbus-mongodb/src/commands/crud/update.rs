use nimbus_core::{
    ArrayPopSide, AtomicWrite, BitwiseOperation, Document, FieldTransform, FieldTransformOperation,
    NumericValue, WriteKey, WriteSetMode,
};

use super::super::super::error::{BAD_VALUE, MongoError};
use super::filter::bson_to_filter_value;

pub(super) fn build_replacement_write(
    write_key: WriteKey,
    u_doc: &bson::Document,
) -> Result<AtomicWrite, MongoError> {
    let mut fields = serde_json::Map::new();
    for (k, v) in u_doc.iter() {
        if k == "_id" {
            continue;
        }
        fields.insert(k.to_string(), bson_to_filter_value(v));
    }

    Ok(AtomicWrite::Set {
        key: write_key,
        document: fields,
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: Default::default(),
        transforms: vec![],
    })
}

pub(super) fn build_operator_write(
    write_key: WriteKey,
    u_doc: &bson::Document,
    current_doc: Option<&Document>,
) -> Result<AtomicWrite, MongoError> {
    let mut field_patch = serde_json::Map::new();
    let mut mask = Vec::new();
    let mut transforms = Vec::new();

    for (op, val) in u_doc.iter() {
        let op_doc = match val.as_document() {
            Some(d) => d,
            None => {
                return Err(MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: format!("update operator {op} requires a document value"),
                });
            }
        };

        match op.as_str() {
            "$set" => {
                for (field, fval) in op_doc.iter() {
                    field_patch.insert(field.to_string(), bson_to_filter_value(fval));
                    mask.push(field.to_string());
                }
            }
            "$unset" => {
                for (field, _) in op_doc.iter() {
                    field_patch.insert(field.to_string(), serde_json::Value::Null);
                    mask.push(field.to_string());
                }
            }
            "$rename" => {
                for (old_name, new_name_bson) in op_doc.iter() {
                    if let bson::Bson::String(new_name) = new_name_bson {
                        let old_val = current_doc
                            .and_then(|d| d.get_field(old_name))
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        field_patch.insert(old_name.to_string(), serde_json::Value::Null);
                        field_patch.insert(new_name.to_string(), old_val);
                        mask.push(old_name.to_string());
                        mask.push(new_name.to_string());
                    }
                }
            }
            "$setOnInsert" => {}
            "$currentDate" => {
                for (field, _) in op_doc.iter() {
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::ServerTimestamp,
                    });
                }
            }
            "$inc" => {
                for (field, inc_val) in op_doc.iter() {
                    let operand = bson_to_numeric_value(inc_val)?;
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::Increment { operand },
                    });
                }
            }
            "$min" => {
                for (field, min_val) in op_doc.iter() {
                    let operand = bson_to_numeric_value(min_val)?;
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::Minimum { operand },
                    });
                }
            }
            "$max" => {
                for (field, max_val) in op_doc.iter() {
                    let operand = bson_to_numeric_value(max_val)?;
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::Maximum { operand },
                    });
                }
            }
            "$mul" => {
                for (field, mul_val) in op_doc.iter() {
                    let operand = bson_to_numeric_value(mul_val)?;
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::Multiply { operand },
                    });
                }
            }
            "$addToSet" => {
                for (field, add_val) in op_doc.iter() {
                    let values = match add_val.as_document().and_then(|d| d.get("$each")) {
                        Some(bson::Bson::Array(arr)) => {
                            arr.iter().map(bson_to_filter_value).collect()
                        }
                        _ => vec![bson_to_filter_value(add_val)],
                    };
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::AppendMissingElements { values },
                    });
                }
            }
            "$push" => {
                for (field, push_val) in op_doc.iter() {
                    let values: Vec<serde_json::Value> =
                        match push_val.as_document().and_then(|d| d.get("$each")) {
                            Some(bson::Bson::Array(arr)) => {
                                arr.iter().map(bson_to_filter_value).collect()
                            }
                            _ => vec![bson_to_filter_value(push_val)],
                        };
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::AppendElements { values },
                    });
                }
            }
            "$pull" => {
                for (field, pull_val) in op_doc.iter() {
                    let remove_val = bson_to_filter_value(pull_val);
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::RemoveAllFromArray {
                            values: vec![remove_val],
                        },
                    });
                }
            }
            "$pullAll" => {
                for (field, pull_vals) in op_doc.iter() {
                    let values = match pull_vals {
                        bson::Bson::Array(arr) => arr.iter().map(bson_to_filter_value).collect(),
                        _ => vec![bson_to_filter_value(pull_vals)],
                    };
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::RemoveAllFromArray { values },
                    });
                }
            }
            "$pop" => {
                for (field, pop_val) in op_doc.iter() {
                    let side = match pop_val {
                        bson::Bson::Int32(1) | bson::Bson::Int64(1) => ArrayPopSide::Last,
                        bson::Bson::Int32(-1) | bson::Bson::Int64(-1) => ArrayPopSide::First,
                        _ => {
                            return Err(MongoError::Command {
                                code: BAD_VALUE.code,
                                code_name: BAD_VALUE.code_name.into(),
                                message: "$pop requires a value of 1 or -1".into(),
                            });
                        }
                    };
                    transforms.push(FieldTransform {
                        field: field.to_string(),
                        transform: FieldTransformOperation::PopArray { side },
                    });
                }
            }
            "$bit" => {
                for (field, bit_val) in op_doc.iter() {
                    let bit_doc = bit_val.as_document().ok_or_else(|| MongoError::Command {
                        code: BAD_VALUE.code,
                        code_name: BAD_VALUE.code_name.into(),
                        message: "$bit requires a document value".into(),
                    })?;
                    for (bit_op, bit_operand) in bit_doc.iter() {
                        let operand = bson_to_i64(bit_operand, "$bit requires integer operands")?;
                        let operation = match bit_op.as_str() {
                            "and" => BitwiseOperation::And,
                            "or" => BitwiseOperation::Or,
                            "xor" => BitwiseOperation::Xor,
                            _ => {
                                return Err(MongoError::Command {
                                    code: BAD_VALUE.code,
                                    code_name: BAD_VALUE.code_name.into(),
                                    message: format!("unsupported $bit operator: {bit_op}"),
                                });
                            }
                        };
                        transforms.push(FieldTransform {
                            field: field.to_string(),
                            transform: FieldTransformOperation::Bitwise { operation, operand },
                        });
                    }
                }
            }
            other => {
                return Err(MongoError::Command {
                    code: BAD_VALUE.code,
                    code_name: BAD_VALUE.code_name.into(),
                    message: format!("unsupported update operator: {other}"),
                });
            }
        }
    }

    if !transforms.is_empty() && field_patch.is_empty() {
        Ok(AtomicWrite::Transform {
            key: write_key,
            transforms,
            precondition: Default::default(),
        })
    } else if transforms.is_empty() {
        Ok(AtomicWrite::Patch {
            key: write_key,
            field_patch,
            typed_fields: Default::default(),
            mask,
            precondition: Default::default(),
            transforms: vec![],
        })
    } else {
        Ok(AtomicWrite::Patch {
            key: write_key,
            field_patch,
            typed_fields: Default::default(),
            mask,
            precondition: Default::default(),
            transforms,
        })
    }
}

pub(super) fn bson_to_numeric_value(value: &bson::Bson) -> Result<NumericValue, MongoError> {
    match value {
        bson::Bson::Int32(n) => Ok(NumericValue::Integer { value: *n as i64 }),
        bson::Bson::Int64(n) => Ok(NumericValue::Integer { value: *n }),
        bson::Bson::Double(f) if f.is_finite() => Ok(NumericValue::Double { value: *f }),
        bson::Bson::Double(_) => Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "numeric update operator requires a finite numeric value".into(),
        }),
        _ => Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "numeric update operator requires a numeric value".into(),
        }),
    }
}

fn bson_to_i64(value: &bson::Bson, message: &str) -> Result<i64, MongoError> {
    match value {
        bson::Bson::Int32(n) => Ok(*n as i64),
        bson::Bson::Int64(n) => Ok(*n),
        _ => Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: message.to_string(),
        }),
    }
}

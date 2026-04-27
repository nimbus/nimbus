use std::cmp::Ordering;

use neovex_core::{Document, Error, OrderBy, OrderDirection, Result};
use serde_json::Value;

use super::filtering::compare_values;

pub(super) fn sort_documents(documents: &mut [Document], order: Option<&OrderBy>) -> Result<()> {
    match order {
        Some(order) => {
            let field = order.field.clone();
            let direction = order.direction;
            validate_order_domain(documents.iter(), &field)?;
            documents.sort_by(|left, right| {
                let ordering = compare_order_field(left.get_field(&field), right.get_field(&field))
                    .expect("ordering inputs should be validated before sorting");
                let ordering = match direction {
                    OrderDirection::Asc => ordering,
                    OrderDirection::Desc => ordering.reverse(),
                };
                ordering.then_with(|| left.id.cmp(&right.id))
            });
        }
        None => {
            documents.sort_by_key(|left| left.id.clone());
        }
    }
    Ok(())
}

pub(super) fn compare_order_field(left: Option<&Value>, right: Option<&Value>) -> Result<Ordering> {
    match (left, right) {
        (Some(left), Some(right)) => compare_values(left, right),
        (Some(_), None) => Ok(Ordering::Less),
        (None, Some(_)) => Ok(Ordering::Greater),
        (None, None) => Ok(Ordering::Equal),
    }
}

fn validate_order_domain<'a>(
    documents: impl Iterator<Item = &'a Document>,
    field: &str,
) -> Result<()> {
    let mut observed_kind = None;
    for document in documents {
        let Some(value) = document.get_field(field) else {
            continue;
        };
        let kind = order_value_kind(value)?;
        if let Some(previous) = observed_kind {
            if previous != kind {
                return Err(Error::InvalidInput(
                    "ordering cannot mix string and number values in the same field".to_string(),
                ));
            }
        } else {
            observed_kind = Some(kind);
        }
    }
    Ok(())
}

fn order_value_kind(value: &Value) -> Result<OrderValueKind> {
    match value {
        Value::String(_) => Ok(OrderValueKind::String),
        Value::Number(number) if number.as_f64().is_some() => Ok(OrderValueKind::Number),
        _ => Err(Error::InvalidInput(
            "ordering only supports string and number fields in phase 1".to_string(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderValueKind {
    String,
    Number,
}

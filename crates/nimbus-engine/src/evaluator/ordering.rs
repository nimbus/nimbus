use std::cmp::Ordering;

use nimbus_core::{Document, Error, OrderBy, OrderDirection, Result};
use serde_json::Value;

use super::filtering::compare_values;

pub(super) fn sort_documents(documents: &mut [Document], order: Option<&OrderBy>) -> Result<()> {
    match order {
        Some(order) => {
            let field = order.field.clone();
            let direction = order.direction;
            let mut keyed = collect_ordered_documents(documents.iter().cloned(), &field)?;
            keyed.sort_by(|(left_key, left), (right_key, right)| {
                let ordering = left_key.cmp(right_key);
                let ordering = match direction {
                    OrderDirection::Asc => ordering,
                    OrderDirection::Desc => ordering.reverse(),
                };
                ordering.then_with(|| left.id.cmp(&right.id))
            });
            for (slot, (_, document)) in documents.iter_mut().zip(keyed) {
                *slot = document;
            }
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

fn collect_ordered_documents(
    documents: impl Iterator<Item = Document>,
    field: &str,
) -> Result<Vec<(OrderSortKey, Document)>> {
    let mut keyed = Vec::new();
    let mut observed_kind = None;
    for document in documents {
        let key = order_sort_key(document.get_field(field))?;
        if let Some(kind) = key.kind() {
            if let Some(previous) = observed_kind {
                if previous != kind {
                    return Err(Error::InvalidInput(
                        "ordering cannot mix string and number values in the same field"
                            .to_string(),
                    ));
                }
            } else {
                observed_kind = Some(kind);
            }
        }
        keyed.push((key, document));
    }
    Ok(keyed)
}

fn order_sort_key(value: Option<&Value>) -> Result<OrderSortKey> {
    match value {
        Some(Value::String(value)) => Ok(OrderSortKey::Scalar(OrderScalarSortKey::String(
            value.clone(),
        ))),
        Some(Value::Number(number)) => {
            let value = number.as_f64().ok_or_else(|| {
                Error::InvalidInput(
                    "ordering only supports string and number fields in phase 1".to_string(),
                )
            })?;
            Ok(OrderSortKey::Scalar(OrderScalarSortKey::Number(
                NumericSortKey(value),
            )))
        }
        Some(_) => Err(Error::InvalidInput(
            "ordering only supports string and number fields in phase 1".to_string(),
        )),
        None => Ok(OrderSortKey::Missing),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderValueKind {
    String,
    Number,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OrderSortKey {
    Scalar(OrderScalarSortKey),
    Missing,
}

impl OrderSortKey {
    fn kind(&self) -> Option<OrderValueKind> {
        match self {
            Self::Scalar(OrderScalarSortKey::String(_)) => Some(OrderValueKind::String),
            Self::Scalar(OrderScalarSortKey::Number(_)) => Some(OrderValueKind::Number),
            Self::Missing => None,
        }
    }
}

impl PartialOrd for OrderSortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderSortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Scalar(left), Self::Scalar(right)) => left.cmp(right),
            (Self::Scalar(_), Self::Missing) => Ordering::Less,
            (Self::Missing, Self::Scalar(_)) => Ordering::Greater,
            (Self::Missing, Self::Missing) => Ordering::Equal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OrderScalarSortKey {
    Number(NumericSortKey),
    String(String),
}

impl PartialOrd for OrderScalarSortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderScalarSortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.cmp(right),
            (Self::String(left), Self::String(right)) => left.cmp(right),
            (Self::Number(_), Self::String(_)) => Ordering::Less,
            (Self::String(_), Self::Number(_)) => Ordering::Greater,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NumericSortKey(f64);

impl Eq for NumericSortKey {}

impl PartialOrd for NumericSortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NumericSortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

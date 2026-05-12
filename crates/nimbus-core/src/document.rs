use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::typed_scalar::{TypedFieldMap, TypedScalarValue};
use crate::types::{DocumentId, TableName, Timestamp};

/// A schemaless document stored in a logical table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub table: TableName,
    pub creation_time: Timestamp,
    pub update_time: Timestamp,
    pub fields: serde_json::Map<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub typed_fields: TypedFieldMap,
}

impl Document {
    /// Creates a new document with generated system fields.
    pub fn new(table: TableName, fields: serde_json::Map<String, Value>) -> Self {
        Self::with_id(DocumentId::new(), table, fields)
    }

    /// Creates a new document with an explicit identifier.
    pub fn with_id(
        id: DocumentId,
        table: TableName,
        fields: serde_json::Map<String, Value>,
    ) -> Self {
        let now = Timestamp::now();
        Self {
            id,
            table,
            creation_time: now,
            update_time: now,
            fields,
            typed_fields: BTreeMap::new(),
        }
    }

    /// Returns a reference to a field if present.
    pub fn get_field(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    /// Returns the shared typed scalar metadata for one field if present.
    pub fn typed_field(&self, name: &str) -> Option<&TypedScalarValue> {
        self.typed_fields.get(name)
    }

    /// Sets one plain JSON field projection and clears any typed scalar
    /// metadata for that field.
    pub fn set_field(&mut self, name: impl Into<String>, value: Value) {
        let name = name.into();
        self.fields.insert(name.clone(), value);
        self.typed_fields.remove(&name);
    }

    /// Sets one typed scalar field and stores its projected JSON value beside
    /// the shared typed metadata.
    pub fn set_typed_field(&mut self, name: impl Into<String>, value: TypedScalarValue) {
        let name = name.into();
        self.fields.insert(name.clone(), value.projected_json());
        self.typed_fields.insert(name, value);
    }

    /// Removes one field and any typed scalar metadata bound to it.
    pub fn remove_field(&mut self, name: &str) {
        self.fields.remove(name);
        self.typed_fields.remove(name);
    }

    /// Converts the document into the external JSON representation by moving its fields.
    pub fn into_json(self) -> Value {
        let mut map = serde_json::Map::with_capacity(self.fields.len() + 3);
        map.insert("_id".to_string(), Value::String(self.id.to_string()));
        map.insert(
            "_creationTime".to_string(),
            Value::Number(serde_json::Number::from(self.creation_time.0)),
        );
        map.insert(
            "_updateTime".to_string(),
            Value::Number(serde_json::Number::from(self.update_time.0)),
        );
        map.extend(self.fields);
        Value::Object(map)
    }

    /// Converts the document into the external JSON representation.
    pub fn to_json(&self) -> Value {
        let mut map = serde_json::Map::with_capacity(self.fields.len() + 3);
        map.insert("_id".to_string(), Value::String(self.id.to_string()));
        map.insert(
            "_creationTime".to_string(),
            Value::Number(serde_json::Number::from(self.creation_time.0)),
        );
        map.insert(
            "_updateTime".to_string(),
            Value::Number(serde_json::Number::from(self.update_time.0)),
        );
        map.extend(
            self.fields
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn typed_scalar_fields_store_projection_outside_user_metadata() {
        let mut document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::new(),
        );
        document.set_typed_field(
            "updatedAt",
            TypedScalarValue::Timestamp {
                value: Timestamp(42),
            },
        );

        assert_eq!(document.get_field("updatedAt"), Some(&json!(42_u64)));
        assert!(matches!(
            document.typed_field("updatedAt"),
            Some(TypedScalarValue::Timestamp {
                value: Timestamp(42)
            })
        ));
    }

    #[test]
    fn plain_field_updates_clear_typed_scalar_metadata() {
        let mut document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::new(),
        );
        document.set_typed_field(
            "updatedAt",
            TypedScalarValue::Timestamp {
                value: Timestamp(42),
            },
        );
        document.set_field("updatedAt", json!("now"));

        assert_eq!(document.get_field("updatedAt"), Some(&json!("now")));
        assert_eq!(document.typed_field("updatedAt"), None);
    }
}

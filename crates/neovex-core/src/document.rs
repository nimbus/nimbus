use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{DocumentId, TableName, Timestamp};

/// A schemaless document stored in a logical table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub table: TableName,
    pub creation_time: Timestamp,
    pub fields: serde_json::Map<String, Value>,
}

impl Document {
    /// Creates a new document with generated system fields.
    pub fn new(table: TableName, fields: serde_json::Map<String, Value>) -> Self {
        Self {
            id: DocumentId::new(),
            table,
            creation_time: Timestamp::now(),
            fields,
        }
    }

    /// Returns a reference to a field if present.
    pub fn get_field(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    /// Converts the document into the external JSON representation by moving its fields.
    pub fn into_json(self) -> Value {
        let mut map = serde_json::Map::with_capacity(self.fields.len() + 2);
        map.insert("_id".to_string(), Value::String(self.id.to_string()));
        map.insert(
            "_creationTime".to_string(),
            Value::Number(serde_json::Number::from(self.creation_time.0)),
        );
        map.extend(self.fields);
        Value::Object(map)
    }

    /// Converts the document into the external JSON representation.
    pub fn to_json(&self) -> Value {
        let mut map = serde_json::Map::with_capacity(self.fields.len() + 2);
        map.insert("_id".to_string(), Value::String(self.id.to_string()));
        map.insert(
            "_creationTime".to_string(),
            Value::Number(serde_json::Number::from(self.creation_time.0)),
        );
        map.extend(
            self.fields
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        Value::Object(map)
    }
}

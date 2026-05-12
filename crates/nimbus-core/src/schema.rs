use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::{TableAccessPolicy, policy_revision_id};
use crate::types::validate_logical_name;
use crate::{Error, Result, TableName};

/// Schema for a single table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSchema {
    pub table: TableName,
    pub fields: Vec<FieldSchema>,
    pub indexes: Vec<IndexDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_policy: Option<TableAccessPolicy>,
}

/// Schema for a single field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
}

/// Supported field types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Number,
    Boolean,
    Array,
    Object,
    Any,
}

/// Definition of a secondary index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDefinition {
    pub name: String,
    pub fields: Vec<String>,
}

/// Tenant-level schema containing all table schemas.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub tables: HashMap<TableName, TableSchema>,
}

impl Schema {
    /// Returns the schema for a specific table.
    pub fn get_table(&self, table: &TableName) -> Option<&TableSchema> {
        self.tables.get(table)
    }
}

impl TableSchema {
    /// Validate a document's fields against this table schema.
    pub fn validate(&self, fields: &serde_json::Map<String, Value>) -> Result<()> {
        for field_schema in &self.fields {
            if field_schema.required && !fields.contains_key(&field_schema.name) {
                return Err(Error::SchemaValidation(format!(
                    "missing required field: {}",
                    field_schema.name
                )));
            }
        }

        for (name, value) in fields {
            if let Some(field_schema) = self.fields.iter().find(|field| field.name == *name)
                && !field_schema.field_type.matches(value)
            {
                return Err(Error::SchemaValidation(format!(
                    "field '{}' expected type {:?}, got {}",
                    name,
                    field_schema.field_type,
                    value_type_name(value)
                )));
            }
        }

        Ok(())
    }

    /// Validate index definitions for this table schema.
    pub fn validate_indexes(&self) -> Result<()> {
        use std::collections::HashSet;

        let mut seen_names = HashSet::new();
        for index in &self.indexes {
            validate_logical_name(&index.name, "index name")?;
            if !seen_names.insert(index.name.clone()) {
                return Err(Error::SchemaValidation(format!(
                    "duplicate index name: {}",
                    index.name
                )));
            }

            if index.fields.is_empty() {
                return Err(Error::SchemaValidation(format!(
                    "index '{}' must include at least one field",
                    index.name
                )));
            }

            let mut seen_fields = HashSet::new();
            for field_name in &index.fields {
                if !seen_fields.insert(field_name.clone()) {
                    return Err(Error::SchemaValidation(format!(
                        "index '{}' includes duplicate field '{}'",
                        index.name, field_name
                    )));
                }

                let field = self
                    .fields
                    .iter()
                    .find(|field| field.name == *field_name)
                    .ok_or_else(|| {
                        Error::SchemaValidation(format!(
                            "index '{}' refers to unknown field '{}'",
                            index.name, field_name
                        ))
                    })?;

                match field.field_type {
                    FieldType::String | FieldType::Number | FieldType::Boolean => {}
                    _ => {
                        return Err(Error::SchemaValidation(format!(
                            "index '{}' requires a scalar field type, got {:?}",
                            index.name, field.field_type
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Validate the declarative access policy definitions for this table.
    pub fn validate_access_policy(&self) -> Result<()> {
        if let Some(policy) = &self.access_policy {
            policy.validate()?;
        }
        Ok(())
    }

    /// Returns a stable fingerprint for the table's access policy state.
    pub fn access_policy_revision(&self) -> Result<String> {
        policy_revision_id(self.access_policy.as_ref())
    }
}

impl FieldType {
    /// Returns whether the JSON value matches this field type.
    pub fn matches(&self, value: &Value) -> bool {
        match self {
            Self::String => value.is_string(),
            Self::Number => value.is_number(),
            Self::Boolean => value.is_boolean(),
            Self::Array => value.is_array(),
            Self::Object => value.is_object(),
            Self::Any => true,
        }
    }
}

impl IndexDefinition {
    /// Returns the indexed field when this is still a single-field index.
    pub fn single_field(&self) -> Option<&str> {
        (self.fields.len() == 1).then(|| self.fields[0].as_str())
    }
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{FieldSchema, FieldType, IndexDefinition, TableSchema};
    use crate::TableName;

    fn users_schema() -> TableSchema {
        TableSchema {
            table: TableName::new("users").expect("table name should be valid"),
            fields: vec![
                FieldSchema {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    required: true,
                },
                FieldSchema {
                    name: "age".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                },
                FieldSchema {
                    name: "anything".to_string(),
                    field_type: FieldType::Any,
                    required: false,
                },
            ],
            indexes: Vec::new(),
            access_policy: None,
        }
    }

    #[test]
    fn schema_rejects_missing_required_field() {
        let schema = users_schema();
        let error = schema
            .validate(&serde_json::Map::from_iter([(
                "age".to_string(),
                json!(30),
            )]))
            .expect_err("validation should fail");

        assert!(error.to_string().contains("missing required field: name"));
    }

    #[test]
    fn schema_rejects_wrong_field_type() {
        let schema = users_schema();
        let error = schema
            .validate(&serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("age".to_string(), json!("thirty")),
            ]))
            .expect_err("validation should fail");

        assert!(
            error
                .to_string()
                .contains("field 'age' expected type Number, got string")
        );
    }

    #[test]
    fn schema_allows_extra_unknown_fields() {
        let schema = users_schema();
        schema
            .validate(&serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("nickname".to_string(), json!("ally")),
            ]))
            .expect("validation should succeed");
    }

    #[test]
    fn schema_allows_any_type_field() {
        let schema = users_schema();
        schema
            .validate(&serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("anything".to_string(), json!({ "nested": true })),
            ]))
            .expect("validation should succeed");
    }

    #[test]
    fn schema_rejects_index_on_unknown_or_non_scalar_field() {
        let mut schema = users_schema();
        schema.indexes = vec![IndexDefinition {
            name: "by_missing".to_string(),
            fields: vec!["missing".to_string()],
        }];

        let unknown_error = schema
            .validate_indexes()
            .expect_err("index validation should fail");
        assert!(unknown_error.to_string().contains("unknown field"));

        schema.indexes = vec![IndexDefinition {
            name: "by_anything".to_string(),
            fields: vec!["anything".to_string()],
        }];

        let non_scalar_error = schema
            .validate_indexes()
            .expect_err("index validation should fail");
        assert!(non_scalar_error.to_string().contains("scalar field type"));
    }

    #[test]
    fn schema_rejects_invalid_index_name() {
        let mut schema = users_schema();
        schema.indexes = vec![IndexDefinition {
            name: "bad\0name".to_string(),
            fields: vec!["name".to_string()],
        }];

        let error = schema
            .validate_indexes()
            .expect_err("index validation should fail");
        assert!(error.to_string().contains("index name"));
    }

    #[test]
    fn schema_rejects_index_without_fields() {
        let mut schema = users_schema();
        schema.indexes = vec![IndexDefinition {
            name: "empty".to_string(),
            fields: Vec::new(),
        }];

        let error = schema
            .validate_indexes()
            .expect_err("index validation should fail");
        assert!(
            error
                .to_string()
                .contains("must include at least one field")
        );
    }

    #[test]
    fn schema_rejects_duplicate_fields_within_one_index() {
        let mut schema = users_schema();
        schema.indexes = vec![IndexDefinition {
            name: "by_age_twice".to_string(),
            fields: vec!["age".to_string(), "age".to_string()],
        }];

        let error = schema
            .validate_indexes()
            .expect_err("index validation should fail");
        assert!(error.to_string().contains("includes duplicate field 'age'"));
    }
}

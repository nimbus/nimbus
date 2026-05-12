use super::*;
use std::collections::HashMap;

use nimbus_core::{FieldSchema, FieldType, IndexDefinition, Schema, TableName, TableSchema};
use nimbus_runtime::RuntimeCompatibilityTarget;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexManifest {
    pub(super) functions: Vec<ConvexFunctionDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexHttpRouteManifest {
    pub(super) routes: Vec<ConvexHttpRouteDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConvexNodeExternalPackagesManifest {
    pub(super) version: u32,
    pub(super) mode: ConvexNodeExternalPackageMode,
    #[serde(default)]
    pub(super) configured_external_packages: Vec<String>,
    pub(super) staging_root: String,
    #[serde(default)]
    pub(super) packages: Vec<ConvexNodeExternalPackageDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConvexNodeExternalPackageMode {
    None,
    Explicit,
    All,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConvexNodeExternalPackageDefinition {
    pub(super) package_name: String,
    pub(super) package_root: Option<String>,
    pub(super) staged_package_root: Option<String>,
    pub(super) size_bytes: u64,
    #[serde(default)]
    pub(super) resolved_specifiers: Vec<String>,
    #[serde(default)]
    pub(super) importers: Vec<ConvexNodeExternalPackageImporter>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexNodeExternalPackageImporter {
    pub(super) file: String,
    pub(super) kind: String,
    pub(super) specifier: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexFunctionDefinition {
    pub(super) name: String,
    pub(super) kind: ConvexFunctionKind,
    #[serde(default)]
    pub(super) visibility: ConvexFunctionVisibility,
    #[serde(default)]
    pub(super) schedulable: bool,
    #[serde(default)]
    pub(super) runtime_environment: ConvexRuntimeEnvironment,
    #[serde(default)]
    pub(super) node_runtime_target: Option<RuntimeCompatibilityTarget>,
    #[serde(default)]
    pub(super) runtime_handler: Option<String>,
    pub(super) plan: Value,
}

impl ConvexFunctionDefinition {
    pub(super) fn runtime_compatibility_target(&self) -> Option<RuntimeCompatibilityTarget> {
        match self.runtime_environment {
            ConvexRuntimeEnvironment::Default => None,
            ConvexRuntimeEnvironment::Node => Some(
                self.node_runtime_target
                    .unwrap_or(RuntimeCompatibilityTarget::Node22),
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexHttpRouteDefinition {
    #[serde(default)]
    pub(super) name: Option<String>,
    pub(super) method: ConvexHttpMethod,
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) path_prefix: Option<String>,
    pub(super) plan: ConvexHttpActionPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConvexHttpActionPlan {
    #[serde(default)]
    pub(super) operation: Option<Value>,
    pub(super) response: ConvexHttpResponseTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConvexHttpResponseTemplate {
    pub(super) kind: ConvexHttpResponseKind,
    pub(super) body: Value,
    #[serde(default)]
    pub(super) status: Option<Value>,
    #[serde(default)]
    pub(super) headers: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConvexHttpResponseKind {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub(super) enum ConvexHttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConvexFunctionKind {
    Query,
    PaginatedQuery,
    Mutation,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConvexFunctionVisibility {
    #[default]
    Public,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConvexRuntimeEnvironment {
    #[default]
    #[serde(rename = "default")]
    Default,
    Node,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexSchemaManifest {
    #[serde(default)]
    pub(super) tables: HashMap<String, ConvexSchemaTableDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexSchemaTableDefinition {
    #[serde(default)]
    pub(super) fields: HashMap<String, ConvexSchemaValidator>,
    #[serde(default)]
    pub(super) indexes: Vec<ConvexSchemaIndexDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConvexSchemaIndexDefinition {
    pub(super) name: String,
    #[serde(default)]
    pub(super) fields: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum ConvexSchemaValidator {
    Any,
    Null,
    String,
    Number,
    Boolean,
    Id {
        #[serde(rename = "tableName")]
        _table_name: Option<String>,
    },
    Literal {
        value: Value,
    },
    Array {
        _element: Box<ConvexSchemaValidator>,
    },
    Object {
        _fields: HashMap<String, ConvexSchemaValidator>,
    },
    Optional {
        inner: Box<ConvexSchemaValidator>,
    },
    Union {
        _members: Vec<ConvexSchemaValidator>,
    },
}

impl ConvexSchemaManifest {
    pub(super) fn into_schema(self) -> Result<Option<Schema>, Error> {
        if self.tables.is_empty() {
            return Ok(None);
        }

        let mut tables = HashMap::new();
        let mut table_names = self.tables.into_iter().collect::<Vec<_>>();
        table_names.sort_by(|(left, _), (right, _)| left.cmp(right));

        for (table_name, table_definition) in table_names {
            let table = TableName::new(table_name)?;
            let schema = table_definition.into_table_schema(table.clone())?;
            tables.insert(table, schema);
        }

        Ok(Some(Schema { tables }))
    }
}

impl ConvexSchemaTableDefinition {
    fn into_table_schema(self, table: TableName) -> Result<TableSchema, Error> {
        let mut fields = self.fields.into_iter().collect::<Vec<_>>();
        fields.sort_by(|(left, _), (right, _)| left.cmp(right));

        let fields = fields
            .into_iter()
            .map(|(field_name, validator)| {
                let (field_type, required) = validator.into_field_type_and_required();
                Ok(FieldSchema {
                    name: field_name,
                    field_type,
                    required,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let indexes = self
            .indexes
            .into_iter()
            .map(ConvexSchemaIndexDefinition::into_index_definition)
            .collect::<Result<Vec<_>, Error>>()?;

        Ok(TableSchema {
            table,
            fields,
            indexes,
            access_policy: None,
        })
    }
}

impl ConvexSchemaIndexDefinition {
    fn into_index_definition(self) -> Result<IndexDefinition, Error> {
        let [field] = self.fields.as_slice() else {
            return Err(Error::InvalidInput(format!(
                "convex schema index '{}' requires exactly one field in the current Nimbus schema bridge",
                self.name
            )));
        };

        Ok(IndexDefinition {
            name: self.name,
            fields: vec![field.clone()],
        })
    }
}

impl ConvexSchemaValidator {
    fn into_field_type_and_required(self) -> (FieldType, bool) {
        match self {
            Self::Any => (FieldType::Any, true),
            Self::Null => (FieldType::Any, true),
            Self::String => (FieldType::String, true),
            Self::Number => (FieldType::Number, true),
            Self::Boolean => (FieldType::Boolean, true),
            Self::Id { .. } => (FieldType::String, true),
            Self::Literal { value } => match value {
                Value::Null => (FieldType::Any, true),
                Value::Bool(_) => (FieldType::Boolean, true),
                Value::Number(_) => (FieldType::Number, true),
                Value::String(_) => (FieldType::String, true),
                Value::Array(_) => (FieldType::Array, true),
                Value::Object(_) => (FieldType::Object, true),
            },
            Self::Array { .. } => (FieldType::Array, true),
            Self::Object { .. } => (FieldType::Object, true),
            Self::Optional { inner } => {
                let (field_type, _) = inner.into_field_type_and_required();
                (field_type, false)
            }
            Self::Union { .. } => (FieldType::Any, true),
        }
    }
}

use super::*;
use std::collections::HashMap;

use nimbus_core::{FieldSchema, FieldType, IndexDefinition, Schema, TableName, TableSchema};
use nimbus_runtime::{
    RuntimeBackendKind, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeJavaScriptEvaluationFormat,
};

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexManifest {
    pub functions: Vec<ConvexFunctionDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexHttpRouteManifest {
    pub routes: Vec<ConvexHttpRouteDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvexNodeExternalPackagesManifest {
    pub version: u32,
    pub mode: ConvexNodeExternalPackageMode,
    #[serde(default)]
    pub configured_external_packages: Vec<String>,
    pub staging_root: String,
    #[serde(default)]
    pub packages: Vec<ConvexNodeExternalPackageDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvexNodeExternalPackageMode {
    None,
    Explicit,
    All,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvexNodeExternalPackageDefinition {
    pub package_name: String,
    pub package_root: Option<String>,
    pub staged_package_root: Option<String>,
    pub size_bytes: u64,
    #[serde(default)]
    pub resolved_specifiers: Vec<String>,
    #[serde(default)]
    pub importers: Vec<ConvexNodeExternalPackageImporter>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexNodeExternalPackageImporter {
    pub file: String,
    pub kind: String,
    pub specifier: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexFunctionDefinition {
    pub name: String,
    pub kind: ConvexFunctionKind,
    #[serde(default)]
    pub visibility: ConvexFunctionVisibility,
    #[serde(default)]
    pub schedulable: bool,
    #[serde(default)]
    pub runtime_environment: ConvexRuntimeEnvironment,
    #[serde(default)]
    pub runtime_engine: RuntimeBackendKind,
    #[serde(default)]
    pub runtime_bundle_content_kind: RuntimeBundleContentKind,
    #[serde(default)]
    pub runtime_javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat,
    #[serde(default)]
    pub runtime_compatibility_target: Option<RuntimeCompatibilityTarget>,
    #[serde(default)]
    pub runtime_package_resolution: Option<ConvexRuntimePackageResolution>,
    #[serde(default)]
    pub node_runtime_target: Option<RuntimeCompatibilityTarget>,
    #[serde(default)]
    pub runtime_handler: Option<String>,
    pub plan: Value,
}

impl ConvexFunctionDefinition {
    pub fn runtime_selection(&self) -> ConvexRuntimeSelection {
        let compatibility_target = self
            .runtime_compatibility_target
            .or(self.node_runtime_target)
            .unwrap_or(match self.runtime_environment {
                ConvexRuntimeEnvironment::Default => RuntimeCompatibilityTarget::WebStandardIsolate,
                ConvexRuntimeEnvironment::Node => {
                    RuntimeCompatibilityTarget::product_default_node_lts_target()
                }
                ConvexRuntimeEnvironment::Bun => RuntimeCompatibilityTarget::BunJsc,
            });
        let package_resolution =
            self.runtime_package_resolution
                .unwrap_or(match self.runtime_environment {
                    ConvexRuntimeEnvironment::Default => ConvexRuntimePackageResolution::Bundled,
                    ConvexRuntimeEnvironment::Node => {
                        ConvexRuntimePackageResolution::NodeExternalPackages
                    }
                    ConvexRuntimeEnvironment::Bun => {
                        ConvexRuntimePackageResolution::BunSelfContained
                    }
                });
        ConvexRuntimeSelection {
            engine: self.runtime_engine,
            bundle_content_kind: self.runtime_bundle_content_kind,
            javascript_evaluation_format: self.runtime_javascript_evaluation_format,
            compatibility_target,
            package_resolution,
        }
    }

    pub fn validate_runtime_selection(&self) -> Result<(), String> {
        if let (Some(runtime_target), Some(node_target)) =
            (self.runtime_compatibility_target, self.node_runtime_target)
            && runtime_target != node_target
        {
            return Err(format!(
                "function {} has conflicting runtime_compatibility_target {:?} and node_runtime_target {:?}",
                self.name, runtime_target, node_target
            ));
        }

        let selection = self.runtime_selection();
        match selection.engine {
            RuntimeBackendKind::V8 => {
                if !matches!(
                    selection.bundle_content_kind,
                    RuntimeBundleContentKind::JavaScript
                ) {
                    return Err(format!(
                        "function {} selects V8 with {:?} bundle content; V8 supports only JavaScript bundles",
                        self.name, selection.bundle_content_kind
                    ));
                }
                if !matches!(
                    selection.javascript_evaluation_format,
                    RuntimeJavaScriptEvaluationFormat::EsModule
                ) {
                    return Err(format!(
                        "function {} selects V8 with {:?} JavaScript evaluation format; V8 supports only ES module evaluation",
                        self.name, selection.javascript_evaluation_format
                    ));
                }
            }
            RuntimeBackendKind::BunJsc => {
                if !matches!(
                    selection.bundle_content_kind,
                    RuntimeBundleContentKind::JavaScript
                ) {
                    return Err(format!(
                        "function {} selects Bun/JSC with {:?} bundle content; Bun/JSC supports only JavaScript program-wrapper bundles",
                        self.name, selection.bundle_content_kind
                    ));
                }
                if !matches!(
                    selection.javascript_evaluation_format,
                    RuntimeJavaScriptEvaluationFormat::ProgramWrapper
                ) {
                    return Err(format!(
                        "function {} selects Bun/JSC with {:?} JavaScript evaluation format; Bun/JSC supports only program-wrapper evaluation",
                        self.name, selection.javascript_evaluation_format
                    ));
                }
                if !matches!(
                    selection.compatibility_target,
                    RuntimeCompatibilityTarget::BunJsc
                ) {
                    return Err(format!(
                        "function {} selects Bun/JSC with {:?} compatibility target; Bun/JSC must not be labeled as a Node runtime target",
                        self.name, selection.compatibility_target
                    ));
                }
                if !matches!(
                    selection.package_resolution,
                    ConvexRuntimePackageResolution::BunSelfContained
                ) {
                    return Err(format!(
                        "function {} selects Bun/JSC with {:?} package resolution; Bun/JSC supports only bun_self_contained package resolution",
                        self.name, selection.package_resolution
                    ));
                }
            }
            RuntimeBackendKind::Wasmtime => {
                if !matches!(
                    selection.bundle_content_kind,
                    RuntimeBundleContentKind::WasmComponent
                ) {
                    return Err(format!(
                        "function {} selects Wasmtime with {:?} bundle content; Wasmtime supports only WASM component bundles",
                        self.name, selection.bundle_content_kind
                    ));
                }
                if !matches!(
                    selection.javascript_evaluation_format,
                    RuntimeJavaScriptEvaluationFormat::EsModule
                ) {
                    return Err(format!(
                        "function {} selects Wasmtime with {:?} JavaScript evaluation format; Wasmtime does not use JavaScript program-wrapper evaluation",
                        self.name, selection.javascript_evaluation_format
                    ));
                }
                if !matches!(
                    selection.compatibility_target,
                    RuntimeCompatibilityTarget::WasmComponent
                ) {
                    return Err(format!(
                        "function {} selects Wasmtime with {:?} compatibility target; Wasmtime functions must use WasmComponent",
                        self.name, selection.compatibility_target
                    ));
                }
                if !matches!(
                    selection.package_resolution,
                    ConvexRuntimePackageResolution::Bundled
                ) {
                    return Err(format!(
                        "function {} selects Wasmtime with {:?} package resolution; Wasmtime component bundles must use bundled package resolution",
                        self.name, selection.package_resolution
                    ));
                }
            }
        }

        match self.runtime_environment {
            ConvexRuntimeEnvironment::Default => {
                if !matches!(selection.engine, RuntimeBackendKind::V8) {
                    return Err(format!(
                        "function {} uses the default runtime but selects {:?}; default runtime functions must use the V8 engine",
                        self.name, selection.engine
                    ));
                }
                if !matches!(
                    selection.compatibility_target,
                    RuntimeCompatibilityTarget::WebStandardIsolate
                ) {
                    return Err(format!(
                        "function {} uses the default runtime but selects {:?}; default runtime functions must use WebStandardIsolate",
                        self.name, selection.compatibility_target
                    ));
                }
                if !matches!(
                    selection.package_resolution,
                    ConvexRuntimePackageResolution::Bundled
                ) {
                    return Err(format!(
                        "function {} uses the default runtime but selects {:?} package resolution; default runtime functions must use bundled package resolution",
                        self.name, selection.package_resolution
                    ));
                }
            }
            ConvexRuntimeEnvironment::Node => {
                if !matches!(selection.engine, RuntimeBackendKind::V8) {
                    return Err(format!(
                        "function {} uses the Node runtime but selects {:?}; Node runtime functions must use the V8 engine",
                        self.name, selection.engine
                    ));
                }
                if !selection.compatibility_target.is_node() {
                    return Err(format!(
                        "function {} uses the Node runtime but selects {:?}; Node runtime functions must use a Node compatibility target",
                        self.name, selection.compatibility_target
                    ));
                }
                if !matches!(
                    selection.package_resolution,
                    ConvexRuntimePackageResolution::NodeExternalPackages
                ) {
                    return Err(format!(
                        "function {} uses the Node runtime but selects {:?} package resolution; Node runtime functions must use node_external_packages",
                        self.name, selection.package_resolution
                    ));
                }
            }
            ConvexRuntimeEnvironment::Bun => {
                if !matches!(selection.engine, RuntimeBackendKind::BunJsc) {
                    return Err(format!(
                        "function {} uses the Bun runtime but selects {:?}; Bun runtime functions must use the Bun/JSC engine",
                        self.name, selection.engine
                    ));
                }
                if !matches!(
                    selection.compatibility_target,
                    RuntimeCompatibilityTarget::BunJsc
                ) {
                    return Err(format!(
                        "function {} uses the Bun runtime but selects {:?}; Bun runtime functions must use BunJsc compatibility target",
                        self.name, selection.compatibility_target
                    ));
                }
                if !matches!(
                    selection.package_resolution,
                    ConvexRuntimePackageResolution::BunSelfContained
                ) {
                    return Err(format!(
                        "function {} uses the Bun runtime but selects {:?} package resolution; Bun runtime functions must use bun_self_contained",
                        self.name, selection.package_resolution
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvexRuntimeSelection {
    pub engine: RuntimeBackendKind,
    pub bundle_content_kind: RuntimeBundleContentKind,
    pub javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat,
    pub compatibility_target: RuntimeCompatibilityTarget,
    pub package_resolution: ConvexRuntimePackageResolution,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexHttpRouteDefinition {
    #[serde(default)]
    pub name: Option<String>,
    pub method: ConvexHttpMethod,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub path_prefix: Option<String>,
    pub plan: ConvexHttpActionPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvexHttpActionPlan {
    #[serde(default)]
    pub operation: Option<Value>,
    pub response: ConvexHttpResponseTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvexHttpResponseTemplate {
    pub kind: ConvexHttpResponseKind,
    pub body: Value,
    #[serde(default)]
    pub status: Option<Value>,
    #[serde(default)]
    pub headers: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvexHttpResponseKind {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ConvexHttpMethod {
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
pub enum ConvexFunctionKind {
    Query,
    PaginatedQuery,
    Mutation,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConvexFunctionVisibility {
    #[default]
    Public,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConvexRuntimeEnvironment {
    #[default]
    #[serde(rename = "default")]
    Default,
    Node,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConvexRuntimePackageResolution {
    Bundled,
    NodeExternalPackages,
    BunSelfContained,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexSchemaManifest {
    #[serde(default)]
    pub tables: HashMap<String, ConvexSchemaTableDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexSchemaTableDefinition {
    #[serde(default)]
    pub fields: HashMap<String, ConvexSchemaValidator>,
    #[serde(default)]
    pub indexes: Vec<ConvexSchemaIndexDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConvexSchemaIndexDefinition {
    pub name: String,
    #[serde(default)]
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConvexSchemaValidator {
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
        #[serde(rename = "element")]
        _element: Box<ConvexSchemaValidator>,
    },
    Object {
        #[serde(rename = "fields")]
        _fields: HashMap<String, ConvexSchemaValidator>,
    },
    Optional {
        inner: Box<ConvexSchemaValidator>,
    },
    Union {
        #[serde(rename = "members")]
        _members: Vec<ConvexSchemaValidator>,
    },
}

impl ConvexSchemaManifest {
    pub fn into_schema(self) -> Result<Option<Schema>, Error> {
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
        Ok(IndexDefinition::new(self.name, self.fields))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convex_schema_manifest_parses_union_validator_members() {
        let manifest = serde_json::from_value::<ConvexSchemaManifest>(serde_json::json!({
            "tables": {
                "runs": {
                    "fields": {
                        "runtime": {
                            "kind": "union",
                            "members": [
                                { "kind": "literal", "value": "default" },
                                { "kind": "literal", "value": "node" }
                            ]
                        }
                    },
                    "indexes": []
                }
            }
        }))
        .expect("union validator should deserialize from its generated 'members' key");

        manifest
            .into_schema()
            .expect("schema with a union field should convert");
    }

    #[test]
    fn convex_schema_manifest_preserves_composite_indexes() {
        let manifest = serde_json::from_value::<ConvexSchemaManifest>(serde_json::json!({
            "tables": {
                "messages": {
                    "fields": {
                        "tenantId": { "kind": "string" },
                        "channelId": { "kind": "string" },
                        "body": { "kind": "string" }
                    },
                    "indexes": [
                        {
                            "name": "by_tenantId_and_channelId",
                            "fields": ["tenantId", "channelId"]
                        }
                    ]
                }
            }
        }))
        .expect("manifest should deserialize");

        let schema = manifest
            .into_schema()
            .expect("schema should convert")
            .expect("schema should exist");
        let table = schema
            .tables
            .get(&TableName::new("messages").expect("table name should parse"))
            .expect("messages schema should exist");

        assert_eq!(table.indexes[0].name, "by_tenantId_and_channelId");
        assert_eq!(table.indexes[0].fields, vec!["tenantId", "channelId"]);
        table
            .validate_indexes()
            .expect("composite index should validate");
    }
}

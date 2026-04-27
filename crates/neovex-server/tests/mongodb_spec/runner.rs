use std::collections::HashMap;
use std::path::Path;

use serde_yaml::Value;

pub struct SpecTestFile {
    pub description: String,
    pub schema_version: String,
    pub create_entities: Vec<EntityDef>,
    pub initial_data: Vec<InitialData>,
    pub tests: Vec<SpecTest>,
}

pub struct EntityDef {
    pub kind: EntityKind,
    pub id: String,
    pub properties: HashMap<String, String>,
}

#[derive(Debug)]
pub enum EntityKind {
    Client,
    Database,
    Collection,
    Session,
    Other(String),
}

pub struct InitialData {
    pub database_name: String,
    pub collection_name: String,
    pub documents: Vec<bson::Document>,
}

pub struct SpecTest {
    pub description: String,
    pub operations: Vec<Operation>,
    pub skip_reason: Option<String>,
    pub run_on_requirements: Option<Vec<RunOnRequirement>>,
}

pub struct Operation {
    pub name: String,
    pub object: String,
    pub arguments: bson::Document,
    pub expect_result: Option<Value>,
    pub expect_error: bool,
}

pub struct RunOnRequirement {
    pub min_server_version: Option<String>,
    pub max_server_version: Option<String>,
    pub topologies: Vec<String>,
}

#[derive(Debug)]
pub enum TestResult {
    Pass,
    Skip(String),
    Fail(String),
}

pub fn parse_spec_file(path: &Path) -> Result<SpecTestFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    let yaml: Value =
        serde_yaml::from_str(&content).map_err(|e| format!("failed to parse YAML: {e}"))?;

    let description = yaml_str(&yaml, "description").unwrap_or_default();
    let schema_version = yaml_str(&yaml, "schemaVersion").unwrap_or_default();

    let create_entities = parse_create_entities(&yaml);
    let initial_data = parse_initial_data(&yaml);
    let tests = parse_tests(&yaml);

    Ok(SpecTestFile {
        description,
        schema_version,
        create_entities,
        initial_data,
        tests,
    })
}

fn yaml_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn parse_create_entities(yaml: &Value) -> Vec<EntityDef> {
    let Some(entities) = yaml.get("createEntities").and_then(|v| v.as_sequence()) else {
        return vec![];
    };

    let mut result = Vec::new();
    for entity in entities {
        let mapping = match entity.as_mapping() {
            Some(m) => m,
            None => continue,
        };

        for (kind_val, props_val) in mapping {
            let kind_str = match kind_val.as_str() {
                Some(s) => s,
                None => continue,
            };

            let kind = match kind_str {
                "client" => EntityKind::Client,
                "database" => EntityKind::Database,
                "collection" => EntityKind::Collection,
                "session" => EntityKind::Session,
                other => EntityKind::Other(other.to_string()),
            };

            let id = props_val
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let mut properties = HashMap::new();
            if let Some(m) = props_val.as_mapping() {
                for (k, v) in m {
                    if let (Some(ks), Some(vs)) = (k.as_str(), v.as_str()) {
                        properties.insert(ks.to_string(), vs.to_string());
                    }
                }
            }

            result.push(EntityDef {
                kind,
                id,
                properties,
            });
        }
    }
    result
}

fn parse_initial_data(yaml: &Value) -> Vec<InitialData> {
    let Some(data) = yaml.get("initialData").and_then(|v| v.as_sequence()) else {
        return vec![];
    };

    let mut result = Vec::new();
    for item in data {
        let db = item
            .get("databaseName")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();
        let coll = item
            .get("collectionName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let documents = item
            .get("documents")
            .and_then(|v| v.as_sequence())
            .map(|docs| {
                docs.iter()
                    .filter_map(|d| yaml_value_to_bson(d).and_then(|b| b.as_document().cloned()))
                    .collect()
            })
            .unwrap_or_default();

        result.push(InitialData {
            database_name: db,
            collection_name: coll,
            documents,
        });
    }
    result
}

fn parse_tests(yaml: &Value) -> Vec<SpecTest> {
    let Some(tests) = yaml.get("tests").and_then(|v| v.as_sequence()) else {
        return vec![];
    };

    let mut result = Vec::new();
    for test in tests {
        let description = yaml_str(test, "description").unwrap_or_default();

        let skip_reason = test
            .get("skipReason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let run_on_requirements = test
            .get("runOnRequirements")
            .and_then(|v| v.as_sequence())
            .map(|reqs| reqs.iter().filter_map(parse_run_on_requirement).collect());

        let operations = test
            .get("operations")
            .and_then(|v| v.as_sequence())
            .map(|ops| ops.iter().filter_map(parse_operation).collect())
            .unwrap_or_default();

        result.push(SpecTest {
            description,
            operations,
            skip_reason,
            run_on_requirements,
        });
    }
    result
}

fn parse_run_on_requirement(req: &Value) -> Option<RunOnRequirement> {
    let min_server = req
        .get("minServerVersion")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let max_server = req
        .get("maxServerVersion")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let topologies = req
        .get("topologies")
        .and_then(|v| v.as_sequence())
        .map(|ts| {
            ts.iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Some(RunOnRequirement {
        min_server_version: min_server,
        max_server_version: max_server,
        topologies,
    })
}

fn parse_operation(op: &Value) -> Option<Operation> {
    let name = op.get("name")?.as_str()?.to_string();
    let object = op
        .get("object")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let arguments = op
        .get("arguments")
        .and_then(yaml_value_to_bson)
        .and_then(|b| b.as_document().cloned())
        .unwrap_or_default();

    let expect_result = op.get("expectResult").cloned();
    let expect_error = op.get("expectError").map(|v| !v.is_null()).unwrap_or(false);

    Some(Operation {
        name,
        object,
        arguments,
        expect_result,
        expect_error,
    })
}

pub fn yaml_value_to_bson(value: &Value) -> Option<bson::Bson> {
    match value {
        Value::Null => Some(bson::Bson::Null),
        Value::Bool(b) => Some(bson::Bson::Boolean(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    Some(bson::Bson::Int32(i as i32))
                } else {
                    Some(bson::Bson::Int64(i))
                }
            } else {
                n.as_f64().map(bson::Bson::Double)
            }
        }
        Value::String(s) => Some(bson::Bson::String(s.clone())),
        Value::Sequence(seq) => {
            let arr: Vec<bson::Bson> = seq.iter().filter_map(yaml_value_to_bson).collect();
            Some(bson::Bson::Array(arr))
        }
        Value::Mapping(m) => {
            let mut doc = bson::Document::new();
            for (k, v) in m {
                let key = k.as_str()?.to_string();
                let val = yaml_value_to_bson(v)?;
                doc.insert(key, val);
            }
            Some(bson::Bson::Document(doc))
        }
        Value::Tagged(_) => None,
    }
}

pub fn classify_operations(spec: &SpecTestFile) -> OperationClassification {
    let mut supported = Vec::new();
    let mut unsupported = Vec::new();

    let supported_ops = [
        "find",
        "insertOne",
        "insertMany",
        "updateOne",
        "updateMany",
        "deleteOne",
        "deleteMany",
        "findOneAndUpdate",
        "findOneAndReplace",
        "findOneAndDelete",
        "aggregate",
        "countDocuments",
        "distinct",
        "createIndex",
        "dropIndex",
        "listIndexes",
    ];

    for test in &spec.tests {
        let all_ops_supported = test
            .operations
            .iter()
            .all(|op| supported_ops.contains(&op.name.as_str()));

        if all_ops_supported {
            supported.push(test.description.clone());
        } else {
            let unsupported_ops: Vec<_> = test
                .operations
                .iter()
                .filter(|op| !supported_ops.contains(&op.name.as_str()))
                .map(|op| op.name.clone())
                .collect();
            unsupported.push((test.description.clone(), unsupported_ops));
        }
    }

    OperationClassification {
        supported,
        unsupported,
    }
}

pub struct OperationClassification {
    pub supported: Vec<String>,
    pub unsupported: Vec<(String, Vec<String>)>,
}

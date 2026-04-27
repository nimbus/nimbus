use std::collections::HashMap;
use std::net::SocketAddr;

use neovex_engine::Service;
use neovex_server::adapters_mongodb::listener::run_listener;
use neovex_testing::ServiceFixture;
use tokio::net::TcpListener;

use super::runner::{self, SpecTest, SpecTestFile, TestResult};
use super::wire_client::WireClient;

pub struct SpecTestFixture {
    _fixture: ServiceFixture<Service>,
    pub addr: SocketAddr,
}

impl SpecTestFixture {
    pub async fn new() -> Self {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let service = fixture.service();
        tokio::spawn(run_listener(listener, service));

        Self {
            _fixture: fixture,
            addr,
        }
    }
}

pub async fn execute_spec_file(
    fixture: &SpecTestFixture,
    spec: &SpecTestFile,
) -> Vec<(String, TestResult)> {
    let mut results = Vec::new();

    let mut entity_map: HashMap<String, EntityRef> = HashMap::new();
    for entity in &spec.create_entities {
        match entity.kind {
            runner::EntityKind::Client => {}
            runner::EntityKind::Database => {
                let db_name = entity
                    .properties
                    .get("databaseName")
                    .cloned()
                    .unwrap_or_else(|| "default".to_string());
                entity_map.insert(entity.id.clone(), EntityRef::Database(db_name));
            }
            runner::EntityKind::Collection => {
                let db_id = entity
                    .properties
                    .get("database")
                    .cloned()
                    .unwrap_or_default();
                let coll_name = entity
                    .properties
                    .get("collectionName")
                    .cloned()
                    .unwrap_or_default();
                let db_name = match entity_map.get(&db_id) {
                    Some(EntityRef::Database(name)) => name.clone(),
                    _ => "default".to_string(),
                };
                entity_map.insert(entity.id.clone(), EntityRef::Collection(db_name, coll_name));
            }
            _ => {}
        }
    }

    for test in &spec.tests {
        if let Some(ref reason) = test.skip_reason {
            results.push((test.description.clone(), TestResult::Skip(reason.clone())));
            continue;
        }

        let result = execute_single_test(fixture, spec, &entity_map, test).await;
        results.push((test.description.clone(), result));
    }

    results
}

enum EntityRef {
    Database(String),
    Collection(String, String),
}

fn resolve_collection<'a>(
    entity_map: &'a HashMap<String, EntityRef>,
    object: &str,
) -> Option<(&'a str, &'a str)> {
    match entity_map.get(object) {
        Some(EntityRef::Collection(db, coll)) => Some((db.as_str(), coll.as_str())),
        _ => None,
    }
}

async fn execute_single_test(
    fixture: &SpecTestFixture,
    spec: &SpecTestFile,
    entity_map: &HashMap<String, EntityRef>,
    test: &SpecTest,
) -> TestResult {
    let mut client = WireClient::connect(fixture.addr).await;

    for data in &spec.initial_data {
        let _ = client
            .drop_collection(&data.database_name, &data.collection_name)
            .await;

        if !data.documents.is_empty() {
            let resp = client
                .insert(&data.database_name, &data.collection_name, &data.documents)
                .await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return TestResult::Fail(format!(
                    "seed insert failed: {}",
                    resp.get_str("errmsg").unwrap_or("unknown")
                ));
            }
        }
    }

    for op in &test.operations {
        let result = execute_operation(&mut client, entity_map, op).await;
        match result {
            Ok(result_docs) => {
                if op.expect_error {
                    return TestResult::Fail(format!(
                        "expected error for '{}' but got success",
                        op.name
                    ));
                }

                if let Some(ref expected) = op.expect_result
                    && let Some(expected_seq) = expected.as_sequence()
                {
                    let expected_bson: Vec<bson::Document> = expected_seq
                        .iter()
                        .filter_map(|d| {
                            runner::yaml_value_to_bson(d).and_then(|b| b.as_document().cloned())
                        })
                        .collect();

                    if !docs_match(&result_docs, &expected_bson) {
                        return TestResult::Fail(format!(
                            "'{}': result mismatch (got {} docs, expected {})",
                            op.name,
                            result_docs.len(),
                            expected_bson.len()
                        ));
                    }
                }
            }
            Err(e) => {
                if !op.expect_error {
                    return TestResult::Fail(format!("'{}' failed: {}", op.name, e));
                }
            }
        }
    }

    TestResult::Pass
}

async fn execute_operation(
    client: &mut WireClient,
    entity_map: &HashMap<String, EntityRef>,
    op: &runner::Operation,
) -> Result<Vec<bson::Document>, String> {
    let (db, coll) = resolve_collection(entity_map, &op.object)
        .ok_or_else(|| format!("unsupported entity: {}", op.object))?;

    match op.name.as_str() {
        "find" => {
            let filter = op
                .arguments
                .get_document("filter")
                .ok()
                .cloned()
                .unwrap_or_default();

            let mut options = bson::Document::new();
            if let Ok(sort) = op.arguments.get_document("sort") {
                options.insert("sort", sort.clone());
            }
            if let Ok(n) = op.arguments.get_i64("limit") {
                options.insert("limit", n);
            }
            if let Ok(n) = op.arguments.get_i32("limit") {
                options.insert("limit", n as i64);
            }
            if let Ok(n) = op.arguments.get_i64("skip") {
                options.insert("skip", n);
            }
            if let Ok(n) = op.arguments.get_i32("skip") {
                options.insert("skip", n as i64);
            }
            if let Ok(n) = op.arguments.get_i64("batchSize") {
                options.insert("batchSize", n);
            }
            if let Ok(n) = op.arguments.get_i32("batchSize") {
                options.insert("batchSize", n as i64);
            }
            if let Ok(proj) = op.arguments.get_document("projection") {
                options.insert("projection", proj.clone());
            }

            client.find(db, coll, filter, options).await
        }
        "insertOne" => {
            let document = op
                .arguments
                .get_document("document")
                .map_err(|_| "missing document".to_string())?;
            let resp = client
                .insert(db, coll, std::slice::from_ref(document))
                .await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return Err(resp
                    .get_str("errmsg")
                    .unwrap_or("insert failed")
                    .to_string());
            }
            Ok(vec![])
        }
        "insertMany" => {
            let documents = op
                .arguments
                .get_array("documents")
                .map_err(|_| "missing documents".to_string())?;
            let docs: Vec<bson::Document> = documents
                .iter()
                .filter_map(|d| d.as_document().cloned())
                .collect();
            let resp = client.insert(db, coll, &docs).await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return Err(resp
                    .get_str("errmsg")
                    .unwrap_or("insert failed")
                    .to_string());
            }
            Ok(vec![])
        }
        "updateOne" => {
            let filter = op
                .arguments
                .get_document("filter")
                .map_err(|_| "missing filter".to_string())?
                .clone();
            let update = op
                .arguments
                .get_document("update")
                .map_err(|_| "missing update".to_string())?
                .clone();
            let resp = client.update(db, coll, filter, update, false).await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return Err(resp
                    .get_str("errmsg")
                    .unwrap_or("update failed")
                    .to_string());
            }
            Ok(vec![])
        }
        "updateMany" => {
            let filter = op
                .arguments
                .get_document("filter")
                .map_err(|_| "missing filter".to_string())?
                .clone();
            let update = op
                .arguments
                .get_document("update")
                .map_err(|_| "missing update".to_string())?
                .clone();
            let resp = client.update(db, coll, filter, update, true).await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return Err(resp
                    .get_str("errmsg")
                    .unwrap_or("update failed")
                    .to_string());
            }
            Ok(vec![])
        }
        "deleteOne" => {
            let filter = op
                .arguments
                .get_document("filter")
                .map_err(|_| "missing filter".to_string())?
                .clone();
            let resp = client.delete(db, coll, filter, 1).await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return Err(resp
                    .get_str("errmsg")
                    .unwrap_or("delete failed")
                    .to_string());
            }
            Ok(vec![])
        }
        "deleteMany" => {
            let filter = op
                .arguments
                .get_document("filter")
                .map_err(|_| "missing filter".to_string())?
                .clone();
            let resp = client.delete(db, coll, filter, 0).await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return Err(resp
                    .get_str("errmsg")
                    .unwrap_or("delete failed")
                    .to_string());
            }
            Ok(vec![])
        }
        "aggregate" => {
            let pipeline = op
                .arguments
                .get_array("pipeline")
                .map_err(|_| "missing pipeline".to_string())?;
            let stages: Vec<bson::Document> = pipeline
                .iter()
                .filter_map(|d| d.as_document().cloned())
                .collect();
            client.aggregate(db, coll, stages).await
        }
        "countDocuments" => {
            let filter = op
                .arguments
                .get_document("filter")
                .ok()
                .cloned()
                .unwrap_or_default();
            let cmd = bson::doc! {
                "count": coll,
                "query": filter,
                "$db": db,
            };
            let resp = client.command(&cmd).await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return Err(resp.get_str("errmsg").unwrap_or("count failed").to_string());
            }
            Ok(vec![])
        }
        "distinct" => {
            let key = op
                .arguments
                .get_str("fieldName")
                .map_err(|_| "missing fieldName".to_string())?;
            let filter = op
                .arguments
                .get_document("filter")
                .ok()
                .cloned()
                .unwrap_or_default();
            let cmd = bson::doc! {
                "distinct": coll,
                "key": key,
                "query": filter,
                "$db": db,
            };
            let resp = client.command(&cmd).await;
            if resp.get_f64("ok").unwrap_or(0.0) != 1.0 {
                return Err(resp
                    .get_str("errmsg")
                    .unwrap_or("distinct failed")
                    .to_string());
            }
            Ok(vec![])
        }
        other => Err(format!("unsupported operation: {other}")),
    }
}

fn docs_match(actual: &[bson::Document], expected: &[bson::Document]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    for (a, e) in actual.iter().zip(expected.iter()) {
        for (key, expected_val) in e {
            if is_special_matcher(expected_val) {
                continue;
            }
            match a.get(key) {
                Some(actual_val) => {
                    if !bson_values_match(actual_val, expected_val) {
                        return false;
                    }
                }
                None => return false,
            }
        }
    }
    true
}

fn bson_values_match(actual: &bson::Bson, expected: &bson::Bson) -> bool {
    if is_special_matcher(expected) {
        return true;
    }
    match (actual, expected) {
        (bson::Bson::Int32(a), bson::Bson::Int32(e)) => a == e,
        (bson::Bson::Int64(a), bson::Bson::Int64(e)) => a == e,
        (bson::Bson::Int32(a), bson::Bson::Int64(e)) => *a as i64 == *e,
        (bson::Bson::Int64(a), bson::Bson::Int32(e)) => *a == *e as i64,
        (bson::Bson::Double(a), bson::Bson::Double(e)) => (a - e).abs() < f64::EPSILON,
        (bson::Bson::String(a), bson::Bson::String(e)) => a == e,
        (bson::Bson::Boolean(a), bson::Bson::Boolean(e)) => a == e,
        (bson::Bson::Null, bson::Bson::Null) => true,
        (bson::Bson::Document(a), bson::Bson::Document(e)) => {
            for (k, ev) in e {
                match a.get(k) {
                    Some(av) => {
                        if !bson_values_match(av, ev) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        }
        (bson::Bson::Array(a), bson::Bson::Array(e)) => {
            if a.len() != e.len() {
                return false;
            }
            a.iter()
                .zip(e.iter())
                .all(|(av, ev)| bson_values_match(av, ev))
        }
        _ => actual == expected,
    }
}

fn is_special_matcher(value: &bson::Bson) -> bool {
    if let bson::Bson::Document(doc) = value {
        doc.keys().any(|k| k.starts_with("$$"))
    } else {
        false
    }
}

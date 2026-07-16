//! Seeded Elle list-append history generation through real execution units.
//!
//! Run the external serializability check locally with:
//! `NIMBUS_ELLE_CLI_JAR=/path/to/elle-cli-standalone.jar cargo nextest run -p nimbus-engine elle_serializable_check_passes`.
//! The test invokes `java -jar ... --model list-append --consistency-models
//! serializable target/elle/<history>.edn`. The jar is optional and is not a CI
//! dependency; `elle_history_recorder_emits_wellformed_edn` always exercises
//! generation, file export, and the built-in structural self-check.

use super::*;
use nimbus_core::{Result, TableName};
use nimbus_testing::{ElleHistoryRecorder, ElleListAppendOp, validate_elle_edn_history};
use std::path::PathBuf;
use std::sync::{Barrier, Mutex};

const ELLE_SEED: u64 = 0x4e49_4d42_5553_0002;
const ELLE_WORKERS: usize = 4;
const ELLE_TRANSACTIONS_PER_WORKER: usize = 24;
const ELLE_KEY_COUNT: usize = 4;

struct SeededChoices {
    state: u64,
}

impl SeededChoices {
    fn derived(seed: u64, process: usize) -> Self {
        Self {
            state: mix64(seed ^ (process as u64).rotate_left(23)),
        }
    }

    fn index(&mut self, upper_bound: usize) -> usize {
        self.state = mix64(self.state);
        (self.state as usize) % upper_bound
    }
}

fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn elle_target_path(label: &str) -> PathBuf {
    // Runtime lookup keeps the test tree free of compile-time Cargo env
    // macros (taxonomy rule F2); CARGO_MANIFEST_DIR is always set when the
    // test runs under cargo/nextest.
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR should be set by the cargo test runner");
    PathBuf::from(manifest_dir)
        .join("../..")
        .join("target/elle")
        .join(format!("nimbus-list-append-{label}-{ELLE_SEED:016x}.edn"))
}

fn key_name(index: usize) -> String {
    format!("k{index}")
}

fn list_fields(values: &[i64]) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("values".to_string(), json!(values))])
}

fn read_list(
    unit: &crate::engine::execution_units::MutationExecutionUnit,
    table: &TableName,
    document_id: &DocumentId,
) -> Result<Vec<i64>> {
    unit.get_document(table, document_id.clone())?
        .ok_or_else(|| Error::DocumentNotFound(document_id.clone()))?
        .get_field("values")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::Internal("Elle list value must be an array".to_string()))?
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| Error::Internal("Elle list element must be an integer".to_string()))
        })
        .collect()
}

async fn generate_elle_history(label: &str) -> (PathBuf, String) {
    let data_dir = tempdir().expect("Elle engine tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(90_000))),
            Arc::new(NoopFaultInjector),
            Arc::new(SeededIdSource::new(ELLE_SEED)),
        )
        .expect("deterministic memory engine should create"),
    );
    let tenant_id = TenantId::new(format!("elle-{label}")).expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("Elle tenant should create");
    let table = messages_table("elle_list_append");
    let seed_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("seed execution unit should begin");
    let mut document_ids = Vec::with_capacity(ELLE_KEY_COUNT);
    for _ in 0..ELLE_KEY_COUNT {
        document_ids.push(
            seed_unit
                .insert_document(table.clone(), list_fields(&[]))
                .expect("Elle key should stage"),
        );
    }
    seed_unit
        .commit()
        .expect("Elle keys should commit")
        .expect("Elle seed transaction should contain writes");

    let history = Arc::new(Mutex::new(ElleHistoryRecorder::new()));
    let start_round = Arc::new(Barrier::new(ELLE_WORKERS));
    let mut workers = Vec::with_capacity(ELLE_WORKERS);
    for process in 0..ELLE_WORKERS {
        workers.push(tokio::task::spawn_blocking({
            let engine = Arc::clone(&engine);
            let tenant_id = tenant_id.clone();
            let table = table.clone();
            let document_ids = document_ids.clone();
            let history = Arc::clone(&history);
            let start_round = Arc::clone(&start_round);
            move || {
                let mut choices = SeededChoices::derived(ELLE_SEED, process);
                for transaction in 0..ELLE_TRANSACTIONS_PER_WORKER {
                    start_round.wait();
                    let read_key = choices.index(ELLE_KEY_COUNT);
                    let append_key = choices.index(ELLE_KEY_COUNT);
                    let appended_value = (process as i64 + 1) * 10_000 + transaction as i64 + 1;
                    let invoke_ops = vec![
                        ElleListAppendOp::Read {
                            key: key_name(read_key),
                            value: None,
                        },
                        ElleListAppendOp::Append {
                            key: key_name(append_key),
                            value: appended_value,
                        },
                    ];
                    history
                        .lock()
                        .expect("Elle history lock should not be poisoned")
                        .record_invoke(process, invoke_ops.clone());

                    let unit = engine
                        .begin_mutation_execution_unit(
                            tenant_id.clone(),
                            PrincipalContext::anonymous(),
                        )
                        .expect("worker execution unit should begin");
                    let observed = read_list(&unit, &table, &document_ids[read_key])
                        .expect("list read should succeed");
                    let mut appended = read_list(&unit, &table, &document_ids[append_key])
                        .expect("append target read should succeed");
                    appended.push(appended_value);
                    unit.update_document(
                        table.clone(),
                        document_ids[append_key].clone(),
                        list_fields(&appended),
                    )
                    .expect("list append should stage");
                    let completion_ops = vec![
                        ElleListAppendOp::Read {
                            key: key_name(read_key),
                            value: Some(observed),
                        },
                        ElleListAppendOp::Append {
                            key: key_name(append_key),
                            value: appended_value,
                        },
                    ];
                    match unit.commit() {
                        Ok(Some(_)) => history
                            .lock()
                            .expect("Elle history lock should not be poisoned")
                            .record_ok(process, completion_ops),
                        Err(Error::Conflict { .. }) => history
                            .lock()
                            .expect("Elle history lock should not be poisoned")
                            .record_fail(process, invoke_ops),
                        Ok(None) => panic!("list append transaction must contain a write"),
                        Err(error) => panic!("unexpected list append failure: {error}"),
                    }
                }
            }
        }));
    }
    for worker in workers {
        timeout(Duration::from_secs(15), worker)
            .await
            .expect("Elle worker should finish")
            .expect("Elle worker should not panic");
    }

    let path = elle_target_path(label);
    let recorder = history
        .lock()
        .expect("Elle history lock should not be poisoned");
    assert_eq!(
        recorder.event_count(),
        ELLE_WORKERS * ELLE_TRANSACTIONS_PER_WORKER * 2
    );
    recorder
        .write_edn(&path)
        .expect("Elle history should write under target/elle");
    (path, recorder.to_edn())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn elle_history_recorder_emits_wellformed_edn() {
    let (path, edn) = generate_elle_history("self-check").await;
    let event_count =
        validate_elle_edn_history(&edn).expect("generated history should be valid EDN");
    assert_eq!(event_count, ELLE_WORKERS * ELLE_TRANSACTIONS_PER_WORKER * 2);
    assert_eq!(
        std::fs::read_to_string(path).expect("written Elle history should read"),
        edn
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn elle_serializable_check_passes() {
    let Some(jar) = std::env::var_os("NIMBUS_ELLE_CLI_JAR") else {
        eprintln!("NIMBUS_ELLE_CLI_JAR is unset; skipping external Elle serializability check");
        return;
    };
    let (path, edn) = generate_elle_history("serializable").await;
    validate_elle_edn_history(&edn).expect("history passed to elle-cli should be well formed");
    let output = std::process::Command::new("java")
        .arg("-jar")
        .arg(jar)
        .args([
            "--model",
            "list-append",
            "--consistency-models",
            "serializable",
        ])
        .arg(&path)
        .output()
        .expect("java should execute the configured elle-cli jar");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "elle-cli failed with {}:\n{combined}",
        output.status
    );
    assert!(
        combined.contains(":valid? true") || combined.contains("valid? true"),
        "elle-cli did not report a valid serializable history:\n{combined}"
    );
    assert!(
        !combined.contains(":valid? false") && !combined.contains("valid? false"),
        "elle-cli reported a serializability anomaly:\n{combined}"
    );
}

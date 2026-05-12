#![allow(dead_code)]

use super::*;

pub(super) fn benchmark_tenant_id(label: &str) -> BenchResult<TenantId> {
    Ok(TenantId::new(format!("bench-{label}"))?)
}

pub(super) fn tasks_table() -> TableName {
    TableName::new("tasks").expect("static table name should be valid")
}

pub(super) fn query_for_all() -> Query {
    Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}

pub(super) fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

pub(super) fn single_field_schema() -> TableSchema {
    TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![IndexDefinition {
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    }
}

pub(super) fn composite_schema() -> TableSchema {
    TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![IndexDefinition {
            name: "by_team_status_rank".to_string(),
            fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    }
}

pub(super) fn copy_dir_all(source: &Path, destination: &Path) -> BenchResult<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

pub(super) fn write_benchmark_master_key(_root: &Path) -> BenchResult<PathBuf> {
    let path = env::temp_dir().join(format!("nimbus-bench-master-{}.key", std::process::id()));
    if !path.exists() {
        fs::write(&path, [0x42_u8; 32])?;
    }
    Ok(path)
}

pub(super) fn read_round_override(env_key: &str, default: usize) -> usize {
    env::var(env_key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

pub(super) fn read_u64_override(env_key: &str, default: u64) -> u64 {
    env::var(env_key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

pub(super) fn duration_from_nanos_f64(nanos: f64) -> Duration {
    Duration::from_secs_f64((nanos.max(0.0)) / 1_000_000_000.0)
}

pub(super) fn median_f64(sorted: &[f64]) -> f64 {
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

pub(super) fn student_t_critical_95(sample_count: usize) -> f64 {
    match sample_count.saturating_sub(1) {
        0 => 0.0,
        1 => 12.706,
        2 => 4.303,
        3 => 3.182,
        4 => 2.776,
        5 => 2.571,
        6 => 2.447,
        7 => 2.365,
        8 => 2.306,
        9 => 2.262,
        10 => 2.228,
        11 => 2.201,
        12 => 2.179,
        13 => 2.160,
        14 => 2.145,
        15 => 2.131,
        16 => 2.120,
        17 => 2.110,
        18 => 2.101,
        19 => 2.093,
        20 => 2.086,
        21 => 2.080,
        22 => 2.074,
        23 => 2.069,
        24 => 2.064,
        25 => 2.060,
        26 => 2.056,
        27 => 2.052,
        28 => 2.048,
        29 => 2.045,
        30 => 2.042,
        _ => 1.960,
    }
}

pub(super) fn duration_ratio(baseline: Duration, candidate: Duration) -> f64 {
    candidate.as_secs_f64().max(f64::MIN_POSITIVE).recip() * baseline.as_secs_f64()
}

pub(super) fn format_duration(duration: Duration) -> String {
    if duration.as_secs_f64() >= 1.0 {
        format!("{:.2} s", duration.as_secs_f64())
    } else if duration.as_millis() > 0 {
        format!("{:.2} ms", duration.as_secs_f64() * 1_000.0)
    } else if duration.as_micros() > 0 {
        format!("{:.2} us", duration.as_secs_f64() * 1_000_000.0)
    } else {
        format!("{:.2} ns", duration.as_secs_f64() * 1_000_000_000.0)
    }
}

pub(super) fn format_confidence_interval(lower: Duration, upper: Duration) -> String {
    format!("{} - {}", format_duration(lower), format_duration(upper))
}

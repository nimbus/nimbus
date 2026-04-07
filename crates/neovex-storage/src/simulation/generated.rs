use std::collections::BTreeMap;

use neovex_core::{
    Cursor, Filter, FilterOp, OrderBy, OrderDirection, PaginatedQuery, Query, TableName,
};
use serde_json::{Map, Value, json};

use super::coordination::ScenarioMetadata;
use super::seeding::splitmix64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTaskRecord {
    pub title: String,
    pub status: String,
    pub rank: i64,
}

impl GeneratedTaskRecord {
    pub(crate) fn generated(seed: u64, slot: u32, step: usize, draw: u64) -> Self {
        let status = match draw % 3 {
            0 => "todo",
            1 => "done",
            _ => "in_progress",
        };
        Self {
            title: format!("seed-{seed}-slot-{slot}-step-{step}"),
            status: status.to_string(),
            rank: ((step as i64) * 32) + i64::from(slot),
        }
    }

    pub fn fields(&self) -> Map<String, Value> {
        Map::from_iter([
            ("title".to_string(), json!(self.title)),
            ("status".to_string(), json!(self.status)),
            ("rank".to_string(), json!(self.rank)),
        ])
    }

    pub fn from_json(value: &Value) -> Self {
        let object = value
            .as_object()
            .expect("generated task json should be an object");
        Self {
            title: object
                .get("title")
                .and_then(Value::as_str)
                .expect("generated task title should be present")
                .to_string(),
            status: object
                .get("status")
                .and_then(Value::as_str)
                .expect("generated task status should be present")
                .to_string(),
            rank: object
                .get("rank")
                .and_then(Value::as_i64)
                .expect("generated task rank should be present"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedTaskHistoryStep {
    Insert {
        slot: u32,
        record: GeneratedTaskRecord,
    },
    Update {
        slot: u32,
        record: GeneratedTaskRecord,
    },
    Delete {
        slot: u32,
    },
}

impl GeneratedTaskHistoryStep {
    pub fn slot(&self) -> u32 {
        match self {
            Self::Insert { slot, .. } | Self::Update { slot, .. } | Self::Delete { slot } => *slot,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Insert { slot, record } => format!(
                "insert(slot={slot}, title={}, status={}, rank={})",
                record.title, record.status, record.rank
            ),
            Self::Update { slot, record } => format!(
                "update(slot={slot}, title={}, status={}, rank={})",
                record.title, record.status, record.rank
            ),
            Self::Delete { slot } => format!("delete(slot={slot})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTaskPageExpectation {
    pub data: Vec<GeneratedTaskRecord>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTaskHistoryModel {
    records_by_slot: BTreeMap<u32, GeneratedTaskRecord>,
    query_status: String,
    page_size: usize,
}

impl GeneratedTaskHistoryModel {
    pub fn final_documents(&self) -> Vec<GeneratedTaskRecord> {
        let mut records = self.records_by_slot.values().cloned().collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.title
                .cmp(&right.title)
                .then_with(|| left.rank.cmp(&right.rank))
                .then_with(|| left.status.cmp(&right.status))
        });
        records
    }

    pub fn query_result(&self) -> Vec<GeneratedTaskRecord> {
        let mut records = self
            .records_by_slot
            .values()
            .filter(|record| record.status == self.query_status)
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.title
                .cmp(&right.title)
                .then_with(|| left.rank.cmp(&right.rank))
                .then_with(|| left.status.cmp(&right.status))
        });
        records
    }

    pub fn first_page(&self) -> GeneratedTaskPageExpectation {
        self.page_from_offset(0)
    }

    pub fn second_page(&self) -> GeneratedTaskPageExpectation {
        self.page_from_offset(self.page_size)
    }

    fn page_from_offset(&self, offset: usize) -> GeneratedTaskPageExpectation {
        let query = self.query_result();
        let remaining = query.len().saturating_sub(offset);
        let data = query
            .into_iter()
            .skip(offset)
            .take(self.page_size)
            .collect::<Vec<_>>();
        GeneratedTaskPageExpectation {
            data,
            has_more: remaining > self.page_size,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTaskHistory {
    metadata: ScenarioMetadata,
    steps: Vec<GeneratedTaskHistoryStep>,
    table: String,
    query_status: String,
    page_size: usize,
}

impl GeneratedTaskHistory {
    pub fn seeded(name: impl Into<String>, seed: u64, step_count: usize) -> Self {
        let mut live_slots = Vec::new();
        let mut next_slot = 0_u32;
        let mut steps = Vec::with_capacity(step_count);

        for step in 0..step_count {
            let draw = splitmix64(seed ^ ((step as u64) << 32) ^ 0xa5a5_a5a5_a5a5_a5a5);
            let should_insert = live_slots.is_empty() || draw % 100 < 45;
            if should_insert {
                let slot = next_slot;
                next_slot = next_slot.saturating_add(1);
                live_slots.push(slot);
                steps.push(GeneratedTaskHistoryStep::Insert {
                    slot,
                    record: GeneratedTaskRecord::generated(seed, slot, step, draw),
                });
                continue;
            }

            let slot_index = (draw as usize) % live_slots.len();
            let slot = live_slots[slot_index];
            if draw % 100 < 80 {
                steps.push(GeneratedTaskHistoryStep::Update {
                    slot,
                    record: GeneratedTaskRecord::generated(seed, slot, step, draw ^ 0x5a5a_5a5a),
                });
            } else {
                live_slots.swap_remove(slot_index);
                steps.push(GeneratedTaskHistoryStep::Delete { slot });
            }
        }

        let query_status = dominant_generated_task_status(&steps);

        Self {
            metadata: ScenarioMetadata::new(name, seed),
            steps,
            table: "tasks".to_string(),
            query_status,
            page_size: 2,
        }
    }

    pub fn metadata(&self) -> &ScenarioMetadata {
        &self.metadata
    }

    pub fn describe(&self) -> String {
        format!(
            "{} with {} generated steps",
            self.metadata.describe(),
            self.steps.len()
        )
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn query_status(&self) -> &str {
        &self.query_status
    }

    pub fn page_size(&self) -> usize {
        self.page_size
    }

    pub fn steps(&self) -> &[GeneratedTaskHistoryStep] {
        &self.steps
    }

    pub fn ordered_query(&self) -> Query {
        Query {
            table: TableName::new(self.table()).expect("generated task table should be valid"),
            filters: vec![Filter {
                field: "status".to_string(),
                op: FilterOp::Eq,
                value: json!(self.query_status()),
            }],
            order: Some(OrderBy {
                field: "title".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        }
    }

    pub fn paginated_query(&self, after: Option<Cursor>) -> PaginatedQuery {
        PaginatedQuery {
            query: self.ordered_query(),
            page_size: self.page_size(),
            after,
        }
    }

    pub fn step_description(&self, step_index: usize) -> String {
        self.steps
            .get(step_index)
            .map(GeneratedTaskHistoryStep::describe)
            .unwrap_or_else(|| format!("unknown-step-{step_index}"))
    }

    pub fn failure_context(&self, invariant: &str, step_index: Option<usize>) -> String {
        match step_index {
            Some(step_index) => format!(
                "{invariant}; {}; step {step_index}: {}",
                self.describe(),
                self.step_description(step_index)
            ),
            None => format!("{invariant}; {}", self.describe()),
        }
    }

    pub fn model(&self) -> GeneratedTaskHistoryModel {
        self.model_through(self.steps.len())
    }

    pub fn model_through(&self, step_count: usize) -> GeneratedTaskHistoryModel {
        let mut records_by_slot = BTreeMap::new();
        for step in self.steps.iter().take(step_count) {
            match step {
                GeneratedTaskHistoryStep::Insert { slot, record }
                | GeneratedTaskHistoryStep::Update { slot, record } => {
                    records_by_slot.insert(*slot, record.clone());
                }
                GeneratedTaskHistoryStep::Delete { slot } => {
                    records_by_slot.remove(slot);
                }
            }
        }
        GeneratedTaskHistoryModel {
            records_by_slot,
            query_status: self.query_status.clone(),
            page_size: self.page_size,
        }
    }

    pub fn model_after_step(&self, step_index: usize) -> GeneratedTaskHistoryModel {
        self.model_through(step_index.saturating_add(1))
    }
}

fn dominant_generated_task_status(steps: &[GeneratedTaskHistoryStep]) -> String {
    let mut records_by_slot = BTreeMap::new();
    for step in steps {
        match step {
            GeneratedTaskHistoryStep::Insert { slot, record }
            | GeneratedTaskHistoryStep::Update { slot, record } => {
                records_by_slot.insert(*slot, record);
            }
            GeneratedTaskHistoryStep::Delete { slot } => {
                records_by_slot.remove(slot);
            }
        }
    }

    let mut counts = BTreeMap::from([
        ("done".to_string(), 0_usize),
        ("in_progress".to_string(), 0_usize),
        ("todo".to_string(), 0_usize),
    ]);
    for record in records_by_slot.values() {
        *counts.entry(record.status.clone()).or_insert(0) += 1;
    }

    counts
        .into_iter()
        .max_by(|(left_status, left_count), (right_status, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_status.cmp(left_status))
        })
        .map(|(status, _)| status)
        .unwrap_or_else(|| "todo".to_string())
}

use super::support::CreatedMessage;
use super::*;

#[derive(Debug, Clone)]
pub(super) enum SeededDemoOperation {
    SendViaAction {
        author: String,
        body: String,
    },
    SendViaHttpAction {
        author: String,
        body: String,
    },
    ScheduleSend {
        author: String,
        body: String,
    },
    RuntimeSendAndSchedule {
        author: String,
        body: String,
    },
    QueryByAuthor {
        author: Option<String>,
    },
    LoadViaHttpAction {
        author: String,
    },
    LoadById {
        message_index: usize,
    },
    CheckUnique {
        author: String,
    },
    CheckExact {
        author: String,
        body: String,
        expect_match: bool,
    },
}

fn scenario_message_budget() -> usize {
    12
}

pub(super) fn seeded_convex_demo_request_timeout() -> Duration {
    Duration::from_secs(3)
}

pub(super) fn seeded_convex_demo_operation_count(step_count: usize) -> usize {
    (6 + step_count / 12).min(14)
}

pub(super) fn seeded_convex_demo_faulted_overlap_step(operation_count: usize) -> usize {
    operation_count.saturating_sub(1).min(2)
}

fn seeded_convex_demo_draw(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut draw = *state;
    draw = (draw ^ (draw >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    draw = (draw ^ (draw >> 27)).wrapping_mul(0x94d049bb133111eb);
    draw ^ (draw >> 31)
}

fn seeded_convex_demo_author(state: &mut u64) -> String {
    const AUTHORS: [&str; 4] = ["Ada", "Byron", "Curie", "Dijkstra"];
    AUTHORS[(seeded_convex_demo_draw(state) as usize) % AUTHORS.len()].to_string()
}

fn seeded_convex_demo_body(seed: u64, step_index: usize, state: &mut u64) -> String {
    format!(
        "seed-{seed}-step-{step_index}-{:04x}",
        seeded_convex_demo_draw(state) & 0xffff
    )
}

pub(super) fn seeded_convex_demo_context(
    seed: u64,
    operation_count: usize,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
    invariant: &str,
    step_index: Option<usize>,
) -> String {
    match case {
        Some(case) => {
            let step_suffix = step_index
                .map(|step| format!(" at convex demo step {step}"))
                .unwrap_or_default();
            format!(
                "{invariant}{step_suffix}; convex demo seed {}, operations {}. {}",
                seed,
                operation_count,
                case.failure_context("nimbus-server", test_name, invariant)
            )
        }
        None => history_context(seed, operation_count, invariant, step_index),
    }
}

fn history_context(
    seed: u64,
    operation_count: usize,
    invariant: &str,
    step_index: Option<usize>,
) -> String {
    match step_index {
        Some(step_index) => format!(
            "{invariant}; convex demo seed {seed}, operations {operation_count}, step {step_index}"
        ),
        None => format!("{invariant}; convex demo seed {seed}, operations {operation_count}"),
    }
}

pub(super) fn choose_seeded_convex_demo_operation(
    seed: u64,
    step_index: usize,
    created: &[CreatedMessage],
    state: &mut u64,
) -> SeededDemoOperation {
    if created.is_empty() {
        return SeededDemoOperation::SendViaAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        };
    }

    let max_messages = scenario_message_budget();
    let can_write = created.len() < max_messages;
    let can_runtime_write = created.len() + 2 <= max_messages;
    let draw = seeded_convex_demo_draw(state) % 10;

    match draw {
        0 if can_runtime_write => SeededDemoOperation::RuntimeSendAndSchedule {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        1 | 2 if can_write => SeededDemoOperation::SendViaAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        3 if can_write => SeededDemoOperation::SendViaHttpAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        4 if can_write => SeededDemoOperation::ScheduleSend {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        5 => {
            let author = if seeded_convex_demo_draw(state).is_multiple_of(4) {
                None
            } else {
                Some(
                    created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                        .author
                        .clone(),
                )
            };
            SeededDemoOperation::QueryByAuthor { author }
        }
        6 => SeededDemoOperation::LoadViaHttpAction {
            author: created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                .author
                .clone(),
        },
        7 => SeededDemoOperation::LoadById {
            message_index: (seeded_convex_demo_draw(state) as usize) % created.len(),
        },
        8 => {
            let author = if seeded_convex_demo_draw(state).is_multiple_of(5) {
                format!("missing-author-{}", step_index)
            } else {
                created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                    .author
                    .clone()
            };
            SeededDemoOperation::CheckUnique { author }
        }
        _ => {
            if seeded_convex_demo_draw(state).is_multiple_of(2) {
                let message = &created[(seeded_convex_demo_draw(state) as usize) % created.len()];
                SeededDemoOperation::CheckExact {
                    author: message.author.clone(),
                    body: message.body.clone(),
                    expect_match: true,
                }
            } else {
                SeededDemoOperation::CheckExact {
                    author: seeded_convex_demo_author(state),
                    body: format!("missing-body-{}", step_index),
                    expect_match: false,
                }
            }
        }
    }
}

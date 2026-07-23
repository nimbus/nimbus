//! Elle list-append history recording for deterministic concurrency tests.
//!
//! The recorder emits one EDN map per line in Elle/Jepsen history format. Each
//! transaction has paired `:invoke` and `:ok`/`:fail` events with `:process`,
//! globally monotonic `:index`, and logical `:time`. Logical time is deliberate:
//! histories remain replayable and never consult ambient wall time.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElleEventType {
    Invoke,
    Ok,
    Fail,
}

impl fmt::Display for ElleEventType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Invoke => ":invoke",
            Self::Ok => ":ok",
            Self::Fail => ":fail",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElleListAppendOp {
    Read {
        key: String,
        value: Option<Vec<i64>>,
    },
    Append {
        key: String,
        value: i64,
    },
}

impl ElleListAppendOp {
    fn to_edn(&self) -> String {
        match self {
            Self::Read { key, value } => {
                let value = match value {
                    None => "nil".to_string(),
                    Some(values) => format!(
                        "[{}]",
                        values
                            .iter()
                            .map(i64::to_string)
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                };
                format!("[:r \"{}\" {value}]", escape_edn_string(key))
            }
            Self::Append { key, value } => {
                format!("[:append \"{}\" {value}]", escape_edn_string(key))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ElleEvent {
    event_type: ElleEventType,
    operations: Vec<ElleListAppendOp>,
    process: usize,
    index: u64,
    time: u64,
}

impl ElleEvent {
    fn to_edn(&self) -> String {
        let operations = self
            .operations
            .iter()
            .map(ElleListAppendOp::to_edn)
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "{{:type {}, :f :txn, :value [{}], :process {}, :index {}, :time {}}}",
            self.event_type, operations, self.process, self.index, self.time
        )
    }
}

#[derive(Debug, Default)]
pub struct ElleHistoryRecorder {
    events: Vec<ElleEvent>,
    next_index: u64,
    next_time: u64,
}

impl ElleHistoryRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn record_invoke(&mut self, process: usize, operations: Vec<ElleListAppendOp>) {
        self.record(ElleEventType::Invoke, process, operations);
    }

    pub fn record_ok(&mut self, process: usize, operations: Vec<ElleListAppendOp>) {
        self.record(ElleEventType::Ok, process, operations);
    }

    pub fn record_fail(&mut self, process: usize, operations: Vec<ElleListAppendOp>) {
        self.record(ElleEventType::Fail, process, operations);
    }

    fn record(
        &mut self,
        event_type: ElleEventType,
        process: usize,
        operations: Vec<ElleListAppendOp>,
    ) {
        let event = ElleEvent {
            event_type,
            operations,
            process,
            index: self.next_index,
            time: self.next_time,
        };
        self.next_index = self.next_index.saturating_add(1);
        self.next_time = self.next_time.saturating_add(1);
        self.events.push(event);
    }

    pub fn to_edn(&self) -> String {
        let mut output = self
            .events
            .iter()
            .map(ElleEvent::to_edn)
            .collect::<Vec<_>>()
            .join("\n");
        if !output.is_empty() {
            output.push('\n');
        }
        output
    }

    pub fn write_edn(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_edn())
    }
}

fn escape_edn_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Validates the emitted line-oriented EDN plus the event-pairing invariants
/// Elle relies on. This intentionally avoids making the external checker a CI
/// dependency; semantic serializability remains the env-gated elle-cli lane.
pub fn validate_elle_edn_history(history: &str) -> Result<usize, String> {
    let mut expected_index = 0u64;
    let mut previous_time = None::<u64>;
    let mut pending_processes = HashSet::new();
    let mut event_count = 0usize;

    for (line_number, line) in history.lines().enumerate() {
        let line_number = line_number + 1;
        if line.trim().is_empty() {
            continue;
        }
        validate_balanced_edn(line).map_err(|error| format!("line {line_number}: {error}"))?;
        if !line.starts_with("{:type ") || !line.ends_with('}') {
            return Err(format!("line {line_number}: event must be one EDN map"));
        }
        for field in [
            ":type ",
            ":f :txn",
            ":value [",
            ":process ",
            ":index ",
            ":time ",
        ] {
            if line.matches(field).count() != 1 {
                return Err(format!(
                    "line {line_number}: required field {field:?} must occur exactly once"
                ));
            }
        }
        if !line.contains("[:r \"") && !line.contains("[:append \"") {
            return Err(format!(
                "line {line_number}: transaction must contain a list-append operation"
            ));
        }

        let process = parse_unsigned(line, ":process ", line_number)?;
        let index = parse_unsigned(line, ":index ", line_number)?;
        let time = parse_unsigned(line, ":time ", line_number)?;
        if index != expected_index {
            return Err(format!(
                "line {line_number}: expected event index {expected_index}, got {index}"
            ));
        }
        expected_index = expected_index.saturating_add(1);
        if previous_time.is_some_and(|previous| time <= previous) {
            return Err(format!(
                "line {line_number}: logical event time must increase strictly"
            ));
        }
        previous_time = Some(time);

        if line.starts_with("{:type :invoke,") {
            if !pending_processes.insert(process) {
                return Err(format!(
                    "line {line_number}: process {process} invoked twice without completion"
                ));
            }
        } else if line.starts_with("{:type :ok,") || line.starts_with("{:type :fail,") {
            if !pending_processes.remove(&process) {
                return Err(format!(
                    "line {line_number}: process {process} completed without an invoke"
                ));
            }
        } else {
            return Err(format!("line {line_number}: unsupported event type"));
        }
        event_count = event_count.saturating_add(1);
    }

    if event_count == 0 {
        return Err("history must contain at least one event".to_string());
    }
    if !pending_processes.is_empty() {
        return Err(format!(
            "history ended with pending processes: {pending_processes:?}"
        ));
    }
    Ok(event_count)
}

/// Validates the terminal verdict emitted by the pinned standalone Elle CLI.
///
/// Elle CLI 0.1.9 writes one terminal line in the form
/// `<history-path> true`. Diagnostics may precede that line, but the checker
/// result is accepted only when the process exits successfully and exactly one
/// terminal verdict names the history supplied by Nimbus. Multiple verdicts,
/// negative or unknown outcomes, and path mismatches fail closed.
pub fn validate_elle_cli_result(
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    history_path: &Path,
) -> Result<(), String> {
    if exit_code != Some(0) {
        return Err(format!(
            "elle-cli exited with {exit_code:?}; stdout:\n{stdout}\nstderr:\n{stderr}"
        ));
    }

    let expected_path = history_path.to_string_lossy();
    let mut verdicts = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let trimmed = line.trim();
        let Some(remainder) = trimmed.strip_prefix(expected_path.as_ref()) else {
            continue;
        };
        if remainder.is_empty() || !remainder.chars().next().is_some_and(char::is_whitespace) {
            continue;
        }
        verdicts.push(remainder.trim().to_string());
    }

    match verdicts.as_slice() {
        [verdict] if verdict == "true" => Ok(()),
        [] => Err(format!(
            "elle-cli did not emit the terminal verdict `{expected_path} true`; stdout:\n{stdout}\nstderr:\n{stderr}"
        )),
        [verdict] => Err(format!(
            "elle-cli reported non-passing verdict `{verdict}` for `{expected_path}`"
        )),
        _ => Err(format!(
            "elle-cli emitted contradictory or duplicate verdicts for `{expected_path}`: {verdicts:?}"
        )),
    }
}

fn parse_unsigned(line: &str, field: &str, line_number: usize) -> Result<u64, String> {
    let start = line
        .find(field)
        .map(|offset| offset + field.len())
        .ok_or_else(|| format!("line {line_number}: missing {field:?}"))?;
    let digits = line[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        return Err(format!(
            "line {line_number}: {field:?} must contain an unsigned integer"
        ));
    }
    digits
        .parse::<u64>()
        .map_err(|error| format!("line {line_number}: invalid integer: {error}"))
}

fn validate_balanced_edn(line: &str) -> Result<(), String> {
    let mut delimiters = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for character in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' | '(' => delimiters.push(character),
            '}' => match delimiters.pop() {
                Some('{') => {}
                _ => return Err("unbalanced `}`".to_string()),
            },
            ']' => match delimiters.pop() {
                Some('[') => {}
                _ => return Err("unbalanced `]`".to_string()),
            },
            ')' => match delimiters.pop() {
                Some('(') => {}
                _ => return Err("unbalanced `)`".to_string()),
            },
            _ => {}
        }
    }
    if in_string {
        return Err("unterminated EDN string".to_string());
    }
    if !delimiters.is_empty() {
        return Err("unclosed EDN delimiter".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_escapes_strings_and_self_validates() {
        let mut history = ElleHistoryRecorder::new();
        history.record_invoke(
            7,
            vec![
                ElleListAppendOp::Read {
                    key: "k\\\"0".to_string(),
                    value: None,
                },
                ElleListAppendOp::Append {
                    key: "k1".to_string(),
                    value: 42,
                },
            ],
        );
        history.record_ok(
            7,
            vec![
                ElleListAppendOp::Read {
                    key: "k\\\"0".to_string(),
                    value: Some(vec![1, 2]),
                },
                ElleListAppendOp::Append {
                    key: "k1".to_string(),
                    value: 42,
                },
            ],
        );
        let edn = history.to_edn();
        assert!(edn.contains("k\\\\\\\"0"));
        assert_eq!(validate_elle_edn_history(&edn), Ok(2));
    }

    #[test]
    fn self_check_rejects_unpaired_events() {
        let history =
            "{:type :invoke, :f :txn, :value [[:r \"k0\" nil]], :process 0, :index 0, :time 0}\n";
        assert!(validate_elle_edn_history(history).is_err());
    }

    #[test]
    fn cli_result_accepts_the_pinned_terminal_true_grammar() {
        let path = Path::new("/tmp/nimbus elle/history.edn");
        assert_eq!(
            validate_elle_cli_result(
                Some(0),
                "checker startup\n/tmp/nimbus elle/history.edn  true\n",
                "",
                path,
            ),
            Ok(())
        );
    }

    #[test]
    fn cli_result_rejects_false_and_unknown_verdicts() {
        let path = Path::new("/tmp/history.edn");
        for verdict in ["false", ":unknown"] {
            let output = format!("{}  {verdict}\n", path.display());
            assert!(
                validate_elle_cli_result(Some(0), &output, "", path)
                    .expect_err("non-passing verdict must fail")
                    .contains(verdict)
            );
        }
    }

    #[test]
    fn cli_result_rejects_malformed_or_wrong_path_output() {
        let path = Path::new("/tmp/history.edn");
        for output in [
            "/tmp/history.edn\n",
            "/tmp/another-history.edn true\n",
            "/tmp/history.edn true trailing\n",
        ] {
            assert!(
                validate_elle_cli_result(Some(0), output, "", path).is_err(),
                "malformed output must fail: {output:?}"
            );
        }
    }

    #[test]
    fn cli_result_rejects_contradictory_verdicts() {
        let path = Path::new("/tmp/history.edn");
        let output = "/tmp/history.edn true\n/tmp/history.edn false\n";
        assert!(
            validate_elle_cli_result(Some(0), output, "", path)
                .expect_err("contradictory verdicts must fail")
                .contains("contradictory")
        );
    }

    #[test]
    fn cli_result_rejects_nonzero_checker_exit_even_with_true_output() {
        let path = Path::new("/tmp/history.edn");
        let error = validate_elle_cli_result(Some(7), "/tmp/history.edn true\n", "boom", path)
            .expect_err("nonzero checker exit must fail");
        assert!(error.contains("Some(7)"));
        assert!(error.contains("boom"));
    }
}

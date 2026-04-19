use super::*;

pub(super) fn read_pid_file_if_exists(path: &Path) -> Result<Option<u32>, String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed.parse::<u32>().map(Some).map_err(|error| {
        format!(
            "failed to parse pidfile {} containing {:?}: {error}",
            path.display(),
            trimmed
        )
    })
}

#[derive(Debug)]
pub(super) struct MachineApiProcessRow {
    pub(super) pid: u32,
    pub(super) ppid: u32,
    pub(super) command: String,
}

pub(super) fn snapshot_process_rows(
    runtime_pid: Option<u32>,
    conmon_pid: Option<u32>,
) -> Result<Vec<MachineApiProcessRow>, String> {
    let pid_set = [runtime_pid, conmon_pid]
        .into_iter()
        .flatten()
        .collect::<std::collections::BTreeSet<_>>();
    if pid_set.is_empty() {
        return Ok(Vec::new());
    }

    let output = std::process::Command::new("ps")
        .args(["-ax", "-o", "pid=,ppid=,command="])
        .output()
        .map_err(|error| format!("failed to run ps for service snapshot: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ps exited with status {} while collecting service snapshot",
            output.status
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("ps output was not valid utf-8: {error}"))?;
    Ok(parse_process_rows(&stdout, &pid_set))
}

fn parse_process_rows(
    stdout: &str,
    pid_set: &std::collections::BTreeSet<u32>,
) -> Vec<MachineApiProcessRow> {
    stdout
        .lines()
        .filter_map(parse_process_row)
        .filter(|row| pid_set.contains(&row.pid))
        .collect()
}

fn parse_process_row(line: &str) -> Option<MachineApiProcessRow> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    let mut fields = trimmed.split_whitespace();
    let pid = fields.next()?.parse::<u32>().ok()?;
    let ppid = fields.next()?.parse::<u32>().ok()?;
    let command = fields.collect::<Vec<_>>().join(" ");
    if command.is_empty() {
        return None;
    }

    Some(MachineApiProcessRow { pid, ppid, command })
}

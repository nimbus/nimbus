use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use neovex::Error;

use super::{MachineConfigRecord, MachineLifecycle, MachineStateRecord};

pub(super) fn build_ssh_command(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<Command, Error> {
    let ssh_target = resolve_localhost_ssh_target(config, state)?;
    let mut command = Command::new("ssh");
    append_localhost_ssh_options(
        &mut command,
        ssh_target.identity_path,
        ssh_target.ssh_port,
        ssh_target.ssh_user,
    );
    Ok(command)
}

pub(super) fn build_scp_command(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
    guest_is_src: bool,
    guest_path: &str,
    host_path: &str,
) -> Result<Command, Error> {
    let ssh_target = resolve_localhost_ssh_target(config, state)?;
    let guest_path = format!("{}@127.0.0.1:{guest_path}", ssh_target.ssh_user);

    let mut command = Command::new("scp");
    append_localhost_scp_options(&mut command, ssh_target.identity_path, ssh_target.ssh_port);
    command.arg("-r");
    if guest_is_src {
        command.arg(guest_path).arg(host_path);
    } else {
        command.arg(host_path).arg(guest_path);
    }
    Ok(command)
}

struct LocalhostSshTarget<'a> {
    identity_path: &'a Path,
    ssh_port: u16,
    ssh_user: &'a str,
}

fn resolve_localhost_ssh_target<'a>(
    config: &'a MachineConfigRecord,
    state: &'a MachineStateRecord,
) -> Result<LocalhostSshTarget<'a>, Error> {
    if state.lifecycle != MachineLifecycle::Running {
        return Err(Error::Conflict(format!(
            "machine '{}' is {} and cannot accept SSH",
            config.name,
            state.lifecycle.as_str()
        )));
    }

    let runtime = state.runtime.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no recorded runtime; start it first",
            config.name
        ))
    })?;
    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no SSH identity configured; start the machine to auto-generate one or re-run `neovex machine init --identity <path>`",
            config.name
        ))
    })?;
    if !identity_path.is_file() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH identity does not exist at {}",
            config.name,
            identity_path.display()
        )));
    }

    Ok(LocalhostSshTarget {
        identity_path,
        ssh_port: runtime.ssh_port,
        ssh_user: &config.guest.ssh_user,
    })
}

fn append_localhost_ssh_options(
    command: &mut Command,
    identity_path: &Path,
    ssh_port: u16,
    ssh_user: &str,
) {
    command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("CheckHostIP=no")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg("SetEnv=LC_ALL=")
        .arg("-i")
        .arg(identity_path)
        .arg("-p")
        .arg(ssh_port.to_string())
        .arg(format!("{ssh_user}@127.0.0.1"));
}

fn append_localhost_scp_options(command: &mut Command, identity_path: &Path, ssh_port: u16) {
    command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("CheckHostIP=no")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg("SetEnv=LC_ALL=")
        .arg("-i")
        .arg(identity_path)
        .arg("-P")
        .arg(ssh_port.to_string());
}

fn build_localhost_ssh_command(
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Result<Command, Error> {
    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no SSH identity configured",
            config.name
        ))
    })?;
    if !identity_path.is_file() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH identity does not exist at {}",
            config.name,
            identity_path.display()
        )));
    }

    let mut command = Command::new("ssh");
    append_localhost_ssh_options(
        &mut command,
        identity_path,
        ssh_port,
        &config.guest.ssh_user,
    );
    Ok(command)
}

pub(super) fn run_guest_ssh_shell_capture(
    config: &MachineConfigRecord,
    ssh_port: u16,
    remote_shell_script: &str,
) -> Result<String, Error> {
    let output = build_localhost_ssh_command(config, ssh_port)?
        .arg(remote_shell_command(remote_shell_script))
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to run guest SSH command on localhost:{ssh_port}: {error}"
            ))
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    Err(Error::Internal(format!(
        "guest SSH command failed on localhost:{ssh_port} with status {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

pub(super) fn stream_guest_file_over_ssh(
    config: &MachineConfigRecord,
    ssh_port: u16,
    source_path: &Path,
    remote_shell_script: &str,
) -> Result<(), Error> {
    let input = fs::File::open(source_path).map_err(|error| {
        Error::Internal(format!(
            "failed to open guest neovex binary {}: {error}",
            source_path.display()
        ))
    })?;
    let status = build_localhost_ssh_command(config, ssh_port)?
        .arg(remote_shell_command(remote_shell_script))
        .stdin(Stdio::from(input))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to stream guest neovex binary over SSH to localhost:{ssh_port}: {error}"
            ))
        })?;
    if status.success() {
        return Ok(());
    }

    Err(Error::Internal(format!(
        "guest neovex binary sync failed on localhost:{ssh_port} with status {status}"
    )))
}

pub(super) fn run_silent_ssh_probe(
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Result<(), Error> {
    let status = build_localhost_ssh_command(config, ssh_port)?
        .arg("true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to run guest SSH readiness probe on localhost:{ssh_port}: {error}"
            ))
        })?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "guest SSH readiness probe failed on localhost:{ssh_port} with status {status}"
    )))
}

pub(super) fn remote_shell_command(script: &str) -> String {
    format!("sh -lc {}", shell_single_quote(script))
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

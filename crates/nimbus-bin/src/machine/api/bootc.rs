use std::process::Command;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::Value;

use super::binaries::resolve_binary;
use super::*;

pub(super) async fn machine_api_bootc_status(
    State(state): State<MachineApiState>,
) -> Result<Json<MachineApiBootcStatusResponse>, MachineApiHttpError> {
    spawn_bootc_task(move || read_bootc_status(&state))
        .await
        .map(Json)
}

pub(super) async fn machine_api_bootc_switch(
    State(state): State<MachineApiState>,
    Json(request): Json<MachineApiBootcSwitchRequest>,
) -> Result<Json<MachineApiBootcOperationResponse>, MachineApiHttpError> {
    spawn_bootc_task(move || {
        let before = read_bootc_status(&state)?;
        let transport = request.transport.as_deref().unwrap_or("registry");
        let output = run_bootc_command(
            &state,
            &[
                "switch",
                "--quiet",
                "--transport",
                transport,
                &request.image,
            ],
        )?;
        let after = read_bootc_status(&state)?;
        Ok(MachineApiBootcOperationResponse {
            before,
            after,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    })
    .await
    .map(Json)
}

pub(super) async fn machine_api_bootc_upgrade(
    State(state): State<MachineApiState>,
    Json(request): Json<MachineApiBootcUpgradeRequest>,
) -> Result<Json<MachineApiBootcOperationResponse>, MachineApiHttpError> {
    spawn_bootc_task(move || {
        let before = read_bootc_status(&state)?;
        let mut args = vec!["upgrade", "--quiet"];
        if request.check {
            args.push("--check");
        }
        if let Some(tag) = request.tag.as_deref() {
            args.push("--tag");
            args.push(tag);
        }
        let output = run_bootc_command(&state, &args)?;
        let after = read_bootc_status(&state)?;
        Ok(MachineApiBootcOperationResponse {
            before,
            after,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    })
    .await
    .map(Json)
}

pub(super) async fn machine_api_bootc_rollback(
    State(state): State<MachineApiState>,
    Json(_request): Json<MachineApiBootcRollbackRequest>,
) -> Result<Json<MachineApiBootcOperationResponse>, MachineApiHttpError> {
    spawn_bootc_task(move || {
        let before = read_bootc_status(&state)?;
        let output = run_bootc_command(&state, &["rollback"])?;
        let after = read_bootc_status(&state)?;
        Ok(MachineApiBootcOperationResponse {
            before,
            after,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    })
    .await
    .map(Json)
}

async fn spawn_bootc_task<T>(
    task: impl FnOnce() -> Result<T, MachineApiHttpError> + Send + 'static,
) -> Result<T, MachineApiHttpError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|error| MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("bootc operation task failed to join: {error}"),
        })?
}

fn read_bootc_status(
    state: &MachineApiState,
) -> Result<MachineApiBootcStatusResponse, MachineApiHttpError> {
    let output = run_bootc_command(state, &["status", "--json"])?;
    let status =
        serde_json::from_str::<Value>(&output.stdout).map_err(|error| MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("failed to decode `bootc status --json` output: {error}"),
        })?;
    Ok(MachineApiBootcStatusResponse {
        booted_image: deployment_image(&status, "booted"),
        booted_digest: deployment_digest(&status, "booted"),
        staged_image: deployment_image(&status, "staged"),
        staged_digest: deployment_digest(&status, "staged"),
        rollback_image: deployment_image(&status, "rollback"),
        rollback_digest: deployment_digest(&status, "rollback"),
        status,
    })
}

fn deployment_image(status: &Value, deployment: &str) -> Option<String> {
    status
        .get("status")?
        .get(deployment)?
        .get("image")?
        .get("image")?
        .get("image")?
        .as_str()
        .map(ToOwned::to_owned)
}

fn deployment_digest(status: &Value, deployment: &str) -> Option<String> {
    status
        .get("status")?
        .get(deployment)?
        .get("image")?
        .get("imageDigest")?
        .as_str()
        .map(ToOwned::to_owned)
}

struct BootcCommandOutput {
    stdout: String,
    stderr: String,
}

fn run_bootc_command(
    state: &MachineApiState,
    args: &[&str],
) -> Result<BootcCommandOutput, MachineApiHttpError> {
    let bootc = resolve_binary(
        "bootc",
        state.binary_lookup_path.as_deref(),
        &state.helper_binary_dirs,
    )
    .ok_or_else(|| MachineApiHttpError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: "missing guest binary required for bootc lifecycle operations: bootc".to_owned(),
    })?;
    let output = Command::new(&bootc)
        .args(args)
        .output()
        .map_err(|error| MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "failed to run bootc command {}: {error}",
                render_bootc_command(args)
            ),
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "bootc command {} failed with status {}{}{}",
                render_bootc_command(args),
                output.status,
                render_command_stream("stdout", &stdout),
                render_command_stream("stderr", &stderr),
            ),
        });
    }
    Ok(BootcCommandOutput { stdout, stderr })
}

fn render_bootc_command(args: &[&str]) -> String {
    std::iter::once("bootc")
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_command_stream(label: &str, contents: &str) -> String {
    let contents = contents.trim();
    if contents.is_empty() {
        String::new()
    } else {
        format!("; {label}: {contents}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_bootc_status_digest_fields() {
        let status = serde_json::json!({
            "status": {
                "booted": {
                    "image": {
                        "image": {"image": "ghcr.io/nimbus/nimbus-machine-os:v1"},
                        "imageDigest": "sha256:booted"
                    }
                },
                "staged": {
                    "image": {
                        "image": {"image": "ghcr.io/nimbus/nimbus-machine-os:v2"},
                        "imageDigest": "sha256:staged"
                    }
                },
                "rollback": null
            }
        });

        assert_eq!(
            deployment_image(&status, "booted").as_deref(),
            Some("ghcr.io/nimbus/nimbus-machine-os:v1")
        );
        assert_eq!(
            deployment_digest(&status, "staged").as_deref(),
            Some("sha256:staged")
        );
        assert_eq!(deployment_image(&status, "rollback"), None);
    }
}

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::cli_ux;
use crate::codegen::{CodegenOptions, run_codegen_for_app_dir_with_options};
use crate::deploy::{DeployRequest, post_deploy_request};

use super::DevTailLogsMode;
use super::banner::format_watch_roots;
use super::plan::DevWatchPlan;

const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WATCH_DEBOUNCE_DELAY: Duration = Duration::from_millis(300);

pub(super) async fn run_dev_watch_loop(
    plan: DevWatchPlan,
) -> Result<(), Box<dyn std::error::Error>> {
    if plan.source_roots.is_empty() {
        std::future::pending::<()>().await;
        return Ok(());
    }

    emit_dev_info(format!(
        "watching {} for codegen changes",
        format_watch_roots(&plan.source_roots)
    ));
    emit_log_tail_note(plan.tail_logs);

    let mut snapshot = match collect_source_snapshot(&plan.app_dir, &plan.source_roots) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            emit_dev_warning(format!(
                "could not snapshot watched sources under {}: {error}",
                plan.app_dir.display()
            ));
            SourceSnapshot::default()
        }
    };

    loop {
        tokio::time::sleep(WATCH_POLL_INTERVAL).await;
        let changed = match collect_source_snapshot(&plan.app_dir, &plan.source_roots) {
            Ok(next) if next != snapshot => true,
            Ok(_) => false,
            Err(error) => {
                emit_dev_warning(format!(
                    "could not rescan watched sources under {}: {error}",
                    plan.app_dir.display()
                ));
                false
            }
        };

        if !changed {
            continue;
        }

        tokio::time::sleep(WATCH_DEBOUNCE_DELAY).await;
        let next_snapshot = match collect_source_snapshot(&plan.app_dir, &plan.source_roots) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                emit_dev_warning(format!(
                    "could not rescan watched sources under {} after debounce: {error}",
                    plan.app_dir.display()
                ));
                continue;
            }
        };

        if next_snapshot == snapshot {
            continue;
        }
        snapshot = next_snapshot;

        emit_dev_info("source change detected; running codegen");
        match run_codegen_for_app_dir_with_options(
            &plan.app_dir,
            CodegenOptions {
                debug_node_apis: plan.debug_node_apis,
            },
        )
        .await
        {
            Ok(()) => match activate_dev_generation(&plan).await {
                Ok(response) => {
                    let change_lines = response.diff.human_lines();
                    if response.activated {
                        emit_dev_info(format!(
                            "activated app generation {} after codegen (previous {}, {} changes)",
                            response.generation,
                            response.previous_generation,
                            change_lines.len()
                        ));
                    } else {
                        emit_dev_info(format!(
                            "validated app artifacts against generation {} without activation (dry_run={})",
                            response.generation, response.dry_run
                        ));
                    }
                    for line in change_lines.into_iter().take(8) {
                        emit_dev_info(format!("deploy diff: {line}"));
                    }
                }
                Err(error) => emit_dev_warning(format!(
                    "generated app artifacts, but local activation failed: {error}"
                )),
            },
            Err(error) => emit_dev_warning(format!("codegen failed: {error}")),
        }
    }
}

async fn activate_dev_generation(
    plan: &DevWatchPlan,
) -> Result<crate::deploy::DeployResponse, Box<dyn std::error::Error>> {
    let request = DeployRequest::from_app_dir(&plan.app_dir, false)?;
    let admin_token = crate::deploy::load_local_admin_token_for_loopback(&plan.local_url);
    post_deploy_request(
        &plan.local_url,
        &plan.deploy_admin_token,
        admin_token.as_deref(),
        &request,
    )
    .await
}

fn emit_dev_info(message: impl AsRef<str>) {
    let _ = cli_ux::write_stderr_prefixed_line("info:", message.as_ref());
}

fn emit_dev_warning(message: impl AsRef<str>) {
    let _ = cli_ux::write_stderr_prefixed_line("warning:", message.as_ref());
}

fn emit_log_tail_note(mode: DevTailLogsMode) {
    match mode {
        DevTailLogsMode::Always | DevTailLogsMode::PauseOnSync => emit_dev_info(format!(
            "runtime log tail mode is {}; live multiplexing is pending runtime log plumbing",
            mode.as_str()
        )),
        DevTailLogsMode::Disable => emit_dev_info("runtime log tailing disabled"),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SourceSnapshot {
    files: std::collections::BTreeMap<PathBuf, FileFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

pub(super) fn collect_source_snapshot(
    app_dir: &Path,
    source_roots: &[PathBuf],
) -> io::Result<SourceSnapshot> {
    let mut files = std::collections::BTreeMap::new();
    for source_root in source_roots {
        collect_source_snapshot_recursive(app_dir, source_root, &mut files)?;
    }
    Ok(SourceSnapshot { files })
}

fn collect_source_snapshot_recursive(
    base: &Path,
    dir: &Path,
    files: &mut std::collections::BTreeMap<PathBuf, FileFingerprint>,
) -> io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            if should_skip_watch_dir(&path) {
                continue;
            }
            collect_source_snapshot_recursive(base, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative_path = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
        files.insert(
            relative_path,
            FileFingerprint {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            },
        );
    }
    Ok(())
}

fn should_skip_watch_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        name,
        "_generated" | "node_modules" | ".git" | ".nimbus" | ".next" | "dist" | "build"
    )
}

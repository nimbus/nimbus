//! Signal-correlated systemd job completion.
//!
//! The naive flow — call `StartTransientUnit`, log the returned job path,
//! return success — masks asynchronous unit failures: the Manager returns the
//! job object path long before the unit actually starts, so a missing
//! `ExecStart` binary would look like success. Instead we call
//! `Manager.Subscribe`, establish the `JobRemoved` stream **before** issuing
//! the method call (closing the race where the job finishes before we are
//! listening), and complete only when the `JobRemoved` signal whose `job`
//! object path matches ours arrives — classifying its `result`.

use std::future::Future;
use std::time::Duration;

use futures::StreamExt;
use nimbus_core::{Error, Result};
use tokio::time;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus_systemd::systemd1::ManagerProxy;

use super::map_zbus;

pub(crate) const DEFAULT_SYSTEMD_JOB_COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);

/// Classified outcome of a systemd job, from the `JobRemoved` `result` string.
#[derive(Debug)]
pub(crate) enum JobOutcome {
    /// `"done"` — the job completed and the unit reached its target state.
    Done,
    /// `"skipped"` — the unit was already in the requested state.
    Skipped,
    /// `"failed"`/`"canceled"`/`"timeout"`/`"dependency"`, or any unrecognized
    /// result string. Carries the raw result for diagnostics.
    Failed(String),
}

impl JobOutcome {
    /// True when the job reached its target state (`done` or `skipped`).
    pub(crate) fn succeeded(&self) -> bool {
        matches!(self, JobOutcome::Done | JobOutcome::Skipped)
    }
}

/// Ensure the connection is subscribed to systemd signals.
///
/// `Manager.Subscribe` is per-connection and **errors** with
/// `org.freedesktop.systemd1.AlreadySubscribed` if the same connection
/// subscribes twice — which happens when a single client runs a start then a
/// stop. Being already subscribed is exactly the state we need for signal
/// delivery, so that error is tolerated; any other error propagates.
async fn ensure_subscribed(manager: &ManagerProxy<'_>) -> Result<()> {
    match manager.subscribe().await {
        Ok(()) => Ok(()),
        Err(err) if is_already_subscribed(&err) => Ok(()),
        Err(err) => Err(map_zbus(err)),
    }
}

fn is_already_subscribed(err: &zbus::Error) -> bool {
    matches!(
        err,
        zbus::Error::MethodError(name, _, _) if name.as_str().ends_with(".AlreadySubscribed")
    )
}

/// `StartTransientUnit` correlated with its `JobRemoved` completion signal.
pub(crate) async fn start_transient_unit_and_wait(
    manager: &ManagerProxy<'_>,
    name: String,
    mode: String,
    properties: Vec<(String, OwnedValue)>,
    job_completion_timeout: Duration,
) -> Result<(OwnedObjectPath, JobOutcome)> {
    ensure_subscribed(manager).await?;
    let mut job_removed = manager.receive_job_removed().await.map_err(map_zbus)?;
    let job_path = manager
        .start_transient_unit(name, mode, properties, Vec::new())
        .await
        .map_err(map_zbus)?;
    let outcome =
        wait_for_job(&mut job_removed, &job_path, "start", job_completion_timeout).await?;
    Ok((job_path, outcome))
}

/// `StopUnit` correlated with its `JobRemoved` completion signal.
pub(crate) async fn stop_unit_and_wait(
    manager: &ManagerProxy<'_>,
    name: String,
    mode: String,
    job_completion_timeout: Duration,
) -> Result<(OwnedObjectPath, JobOutcome)> {
    ensure_subscribed(manager).await?;
    let mut job_removed = manager.receive_job_removed().await.map_err(map_zbus)?;
    let job_path = manager.stop_unit(name, mode).await.map_err(map_zbus)?;
    let outcome = wait_for_job(&mut job_removed, &job_path, "stop", job_completion_timeout).await?;
    Ok((job_path, outcome))
}

/// Drain `JobRemoved` signals until the one for `job_path` arrives.
async fn wait_for_job(
    job_removed: &mut zbus_systemd::systemd1::JobRemovedStream,
    job_path: &OwnedObjectPath,
    phase: &str,
    job_completion_timeout: Duration,
) -> Result<JobOutcome> {
    wait_for_job_result_with_timeout(
        job_path,
        phase,
        job_completion_timeout,
        drain_job_removed(job_removed, job_path, phase),
    )
    .await
}

async fn wait_for_job_result_with_timeout(
    job_path: &OwnedObjectPath,
    phase: &str,
    job_completion_timeout: Duration,
    wait_for_result: impl Future<Output = Result<JobOutcome>>,
) -> Result<JobOutcome> {
    time::timeout(job_completion_timeout, wait_for_result)
        .await
        .map_err(|_| job_wait_timeout_error(job_path, phase, job_completion_timeout))?
}

async fn drain_job_removed(
    job_removed: &mut zbus_systemd::systemd1::JobRemovedStream,
    job_path: &OwnedObjectPath,
    phase: &str,
) -> Result<JobOutcome> {
    while let Some(signal) = job_removed.next().await {
        let args = signal.args().map_err(map_zbus)?;
        if args.job() == job_path {
            return Ok(classify_result(args.result()));
        }
    }
    Err(Error::Internal(format!(
        "systemd JobRemoved stream ended before the {phase} job {} completed",
        job_path.as_str()
    )))
}

fn job_wait_timeout_error(
    job_path: &OwnedObjectPath,
    phase: &str,
    job_completion_timeout: Duration,
) -> Error {
    Error::Internal(format!(
        "systemd JobRemoved stream did not report the {phase} job {} within {}ms",
        job_path.as_str(),
        job_completion_timeout.as_millis()
    ))
}

fn classify_result(result: &str) -> JobOutcome {
    match result {
        "done" => JobOutcome::Done,
        "skipped" => JobOutcome::Skipped,
        // `failed`/`canceled`/`timeout`/`dependency` and any unrecognized
        // result (`once`/`merged`/`assert`/`unsupported`/`collected`, …) are
        // errors — an unknown result is never silently treated as success.
        other => JobOutcome::Failed(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::future;

    use super::*;

    #[tokio::test]
    async fn wait_for_job_result_times_out_when_matching_signal_never_arrives() {
        let job_path = OwnedObjectPath::try_from("/org/freedesktop/systemd1/job/42".to_string())
            .expect("test job path should be valid");

        let error = wait_for_job_result_with_timeout(
            &job_path,
            "start",
            Duration::from_millis(5),
            future::pending::<Result<JobOutcome>>(),
        )
        .await
        .expect_err("missing JobRemoved signal should time out");

        assert!(
            error.to_string().contains(
                "did not report the start job /org/freedesktop/systemd1/job/42 within 5ms"
            ),
            "timeout error should identify the missing job path and deadline: {error}"
        );
    }
}

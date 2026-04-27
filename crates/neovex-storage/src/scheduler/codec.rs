use neovex_core::{Error, JobId, Result, ScheduledJob, Timestamp};

pub(super) fn scheduled_job_key(run_at: Timestamp, id: &JobId) -> Vec<u8> {
    let mut key = Vec::with_capacity(9 + id.as_str().len());
    key.extend_from_slice(&run_at.0.to_be_bytes());
    key.extend_from_slice(id.as_str().as_bytes());
    key
}

pub(super) fn running_job_key(id: &JobId) -> Vec<u8> {
    id.as_str().as_bytes().to_vec()
}

pub(super) fn due_jobs_upper_bound(now: Timestamp) -> Vec<u8> {
    let mut key = Vec::with_capacity(9);
    key.extend_from_slice(&now.0.to_be_bytes());
    key.push(0xff);
    key
}

pub(super) fn scheduled_job_result_key(id: &JobId) -> Vec<u8> {
    id.as_str().as_bytes().to_vec()
}

pub(super) fn scheduled_key_matches_job_id(key: &[u8], job_id: &JobId) -> bool {
    key.len() >= 8 && key[8..] == job_id.as_str().as_bytes()[..]
}

pub(super) fn serialize_job(job: &ScheduledJob) -> Result<Vec<u8>> {
    rmp_serde::to_vec(job).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn deserialize_job(bytes: &[u8]) -> Result<ScheduledJob> {
    rmp_serde::from_slice(bytes).map_err(|error| Error::Serialization(error.to_string()))
}

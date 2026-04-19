use super::*;

pub(super) fn read_log_chunk(path: &Path, offset: u64) -> Result<(String, u64), String> {
    let Ok(mut file) = File::open(path) else {
        return Ok((String::new(), offset));
    };

    let metadata = file.metadata().map_err(|error| {
        format!(
            "failed to inspect persisted log file {}: {error}",
            path.display()
        )
    })?;
    let file_len = metadata.len();
    let start = offset.min(file_len);
    file.seek(SeekFrom::Start(start)).map_err(|error| {
        format!(
            "failed to seek persisted log file {}: {error}",
            path.display()
        )
    })?;

    let mut buffer = String::new();
    file.read_to_string(&mut buffer).map_err(|error| {
        format!(
            "failed to read persisted log file {}: {error}",
            path.display()
        )
    })?;

    Ok((buffer, file_len))
}

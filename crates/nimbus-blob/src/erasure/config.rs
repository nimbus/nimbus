use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use nimbus_core::{Error, Result};

const DEFAULT_DATA_SHARDS: usize = 4;
const DEFAULT_PARITY_SHARDS: usize = 2;
const DEFAULT_STRIPE_WIDTH: usize = 1024 * 1024;

/// Configuration for the multi-drive erasure-coded blob leg.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErasureConfig {
    pub drives: Vec<PathBuf>,
    pub data_shards: usize,
    pub parity_shards: usize,
    pub stripe_width: usize,
}

impl ErasureConfig {
    pub fn new(
        drives: Vec<PathBuf>,
        data_shards: usize,
        parity_shards: usize,
        stripe_width: usize,
    ) -> Result<Self> {
        if !(2..=16).contains(&data_shards) {
            return Err(Error::InvalidInput(format!(
                "erasure data shard count must be 2..=16, got {data_shards}"
            )));
        }
        if !(1..=4).contains(&parity_shards) {
            return Err(Error::InvalidInput(format!(
                "erasure parity shard count must be 1..=4, got {parity_shards}"
            )));
        }
        let total = data_shards + parity_shards;
        if drives.len() != total {
            return Err(Error::InvalidInput(format!(
                "erasure drive count must equal data+parity shards ({total}), got {}",
                drives.len()
            )));
        }
        if stripe_width == 0 || stripe_width % (data_shards * 2) != 0 {
            return Err(Error::InvalidInput(format!(
                "erasure stripe width must be non-zero and a multiple of data_shards*2 ({}), got {stripe_width}",
                data_shards * 2
            )));
        }

        let mut normalized = Vec::with_capacity(drives.len());
        for drive in &drives {
            let identity = path_identity(drive)?;
            if normalized.contains(&identity) {
                return Err(Error::InvalidInput(format!(
                    "erasure drive roots must be distinct after canonicalization: {}",
                    drive.display()
                )));
            }
            normalized.push(identity);
        }

        Ok(Self {
            drives,
            data_shards,
            parity_shards,
            stripe_width,
        })
    }

    pub fn default_for_drives(drives: Vec<PathBuf>) -> Result<Self> {
        Self::new(
            drives,
            DEFAULT_DATA_SHARDS,
            DEFAULT_PARITY_SHARDS,
            DEFAULT_STRIPE_WIDTH,
        )
    }

    pub(crate) fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }
}

fn path_identity(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| Error::InvalidInput(format!("resolve current dir: {err}")))?
            .join(path)
    };

    if let Ok(canonical) = absolute.canonicalize() {
        return Ok(canonical);
    }

    let mut probe = absolute.as_path();
    let mut suffix: Vec<OsString> = Vec::new();
    loop {
        if probe.exists() {
            let mut base = probe.canonicalize().map_err(|err| {
                Error::InvalidInput(format!(
                    "canonicalize existing drive path {}: {err}",
                    probe.display()
                ))
            })?;
            for component in suffix.iter().rev() {
                base.push(component);
            }
            return Ok(normalize_lexically(base));
        }
        let Some(file_name) = probe.file_name() else {
            return Ok(normalize_lexically(absolute));
        };
        suffix.push(file_name.to_os_string());
        let Some(parent) = probe.parent() else {
            return Ok(normalize_lexically(absolute));
        };
        probe = parent;
    }
}

fn normalize_lexically(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

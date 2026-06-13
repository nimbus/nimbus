use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use deno_permissions::PathResolveError;
use serde::Serialize;

use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeLimits;
use crate::runtime::RuntimeBundle;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuntimeContractPathsDescriptor {
    pub(crate) cwd: String,
    pub(crate) app_root: String,
    pub(crate) generated_root: String,
    pub(crate) temp_root: String,
    pub(crate) cache_root: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimePathPolicy {
    cwd: PathBuf,
    app_root: PathBuf,
    generated_root: PathBuf,
    temp_root: PathBuf,
    cache_root: PathBuf,
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    resolution_roots: Vec<PathBuf>,
    run_targets: Vec<PathBuf>,
}

fn runtime_self_exec_target(generated_root: &Path) -> Result<PathBuf> {
    let current_exec = std::env::current_exe().map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "failed to resolve runtime self exec target: {error}"
        ))
    })?;
    let exec_name = current_exec.file_name().ok_or_else(|| {
        NimbusRuntimeError::Contract("runtime self exec target should have a file name".to_string())
    })?;
    canonicalize_preserving_missing_suffix(&generated_root.join("bin").join(exec_name))
        .map_err(NimbusRuntimeError::Io)
}

fn runtime_host_exec_target() -> Result<PathBuf> {
    let current_exec = std::env::current_exe().map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "failed to resolve runtime host exec target: {error}"
        ))
    })?;
    canonicalize_preserving_missing_suffix(&current_exec).map_err(NimbusRuntimeError::Io)
}

impl RuntimePathPolicy {
    pub(crate) fn for_bundle(bundle: &RuntimeBundle, limits: &RuntimeLimits) -> Result<Self> {
        let generated_root = bundle.module_root()?;
        let (app_root, nimbus_root) = infer_app_and_nimbus_roots(&generated_root);
        let temp_root = nimbus_root.join("tmp");
        let cache_root = nimbus_root.join("cache");

        let read_roots = resolve_path_grants(
            &limits.grants.read,
            &app_root,
            &generated_root,
            &temp_root,
            &cache_root,
            "read",
        )?;
        let write_roots = resolve_path_grants(
            &limits.grants.write,
            &app_root,
            &generated_root,
            &temp_root,
            &cache_root,
            "write",
        )?;
        let cwd = if read_roots.iter().any(|root| root == &app_root) {
            app_root.clone()
        } else {
            generated_root.clone()
        };
        let mut resolution_roots = vec![generated_root.clone()];
        for root in [&app_root, &cache_root] {
            if read_roots.iter().any(|read_root| read_root == root)
                && resolution_roots.iter().all(|existing| existing != root)
            {
                resolution_roots.push(root.clone());
            }
        }

        let run_targets =
            resolve_run_grants(&limits.grants.run, &app_root, &generated_root, &cache_root)?;

        Ok(Self {
            cwd: canonicalize_preserving_missing_suffix(&cwd)?,
            app_root: canonicalize_preserving_missing_suffix(&app_root)?,
            generated_root: canonicalize_preserving_missing_suffix(&generated_root)?,
            temp_root: canonicalize_preserving_missing_suffix(&temp_root)?,
            cache_root: canonicalize_preserving_missing_suffix(&cache_root)?,
            read_roots: canonicalize_roots(read_roots)?,
            write_roots: canonicalize_roots(write_roots)?,
            resolution_roots: canonicalize_roots(resolution_roots)?,
            run_targets,
        })
    }

    pub(crate) fn descriptor(&self) -> RuntimeContractPathsDescriptor {
        RuntimeContractPathsDescriptor {
            cwd: self.cwd.display().to_string(),
            app_root: self.app_root.display().to_string(),
            generated_root: self.generated_root.display().to_string(),
            temp_root: self.temp_root.display().to_string(),
            cache_root: self.cache_root.display().to_string(),
        }
    }

    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub(crate) fn resolution_roots(&self) -> &[PathBuf] {
        &self.resolution_roots
    }

    pub(crate) fn read_roots(&self) -> &[PathBuf] {
        &self.read_roots
    }

    pub(crate) fn write_roots(&self) -> &[PathBuf] {
        &self.write_roots
    }

    pub(crate) fn run_targets(&self) -> &[PathBuf] {
        &self.run_targets
    }

    pub(crate) fn runtime_self_exec_target(&self) -> Result<PathBuf> {
        runtime_self_exec_target(&self.generated_root)
    }

    pub(crate) fn ensure_module_read_path(&self, path: &Path) -> Result<PathBuf> {
        let canonical = canonicalize_preserving_missing_suffix(path)?;
        self.ensure_within_roots(&canonical, &self.read_roots, "read")?;
        Ok(canonical)
    }

    pub(crate) fn ensure_read_path_lexical(&self, path: &Path) -> Result<PathBuf> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        let normalized = normalize_absolute_path_lexically(&absolute);
        self.ensure_within_roots(&normalized, &self.read_roots, "read")?;
        Ok(normalized)
    }

    pub(crate) fn ensure_read_metadata_path(&self, path: &Path) -> Result<PathBuf> {
        self.ensure_read_path_lexical(path)
    }

    pub(crate) fn ensure_read_metadata_target_path(&self, path: &Path) -> Result<PathBuf> {
        let canonical = canonicalize_preserving_missing_suffix_from_base(path, &self.cwd)?;
        self.ensure_within_roots(&canonical, &self.read_roots, "read")?;
        Ok(canonical)
    }

    pub(crate) fn ensure_read_link_path(&self, path: &Path) -> Result<PathBuf> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        };
        let normalized = normalize_absolute_path_lexically(&absolute);
        let Some(name) = normalized.file_name() else {
            self.ensure_within_roots(&normalized, &self.read_roots, "read")?;
            return Ok(normalized);
        };
        let parent = normalized.parent().unwrap_or(self.cwd.as_path());
        let canonical_parent = canonicalize_preserving_missing_suffix_from_base(parent, &self.cwd)?;
        let canonical_link = canonical_parent.join(name);
        self.ensure_within_roots(&canonical_link, &self.read_roots, "read")?;
        Ok(canonical_link)
    }

    pub(crate) fn ensure_read_link_target_path(
        &self,
        target: &Path,
        link_path: &Path,
    ) -> Result<()> {
        let link_parent = link_path.parent().unwrap_or(self.cwd.as_path());
        let resolved_target =
            canonicalize_preserving_missing_suffix_from_base(target, link_parent)?;
        self.ensure_within_roots(&resolved_target, &self.read_roots, "read")
    }

    pub(crate) fn ensure_write_path(&self, path: &Path) -> Result<PathBuf> {
        let canonical = canonicalize_preserving_missing_suffix_from_base(path, &self.cwd)?;
        self.ensure_within_roots(&canonical, &self.write_roots, "write")?;
        Ok(canonical)
    }

    pub(crate) fn ensure_symlink_target_path(
        &self,
        target: &Path,
        link_path: &Path,
    ) -> Result<PathBuf> {
        let link_canonical =
            canonicalize_preserving_missing_suffix_from_base(link_path, &self.cwd)?;
        let link_parent = link_canonical.parent().unwrap_or(self.cwd.as_path());
        let resolved_target =
            canonicalize_preserving_missing_suffix_from_base(target, link_parent)?;
        self.ensure_within_roots(&resolved_target, &self.read_roots, "read")?;
        Ok(target.to_path_buf())
    }

    fn ensure_within_roots(&self, candidate: &Path, roots: &[PathBuf], access: &str) -> Result<()> {
        if roots.iter().any(|root| candidate.starts_with(root)) {
            return Ok(());
        }

        let allowed = if roots.is_empty() {
            "none".to_string()
        } else {
            roots
                .iter()
                .map(|root| root.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        Err(NimbusRuntimeError::CapabilityDenied(format!(
            "runtime {access} capability denied for {} (allowed roots: {allowed})",
            candidate.display()
        )))
    }
}

fn resolve_path_grants(
    grants: &[String],
    app_root: &Path,
    generated_root: &Path,
    temp_root: &Path,
    cache_root: &Path,
    access: &str,
) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    for grant in grants {
        let root = match grant.as_str() {
            "$app_root" => app_root.to_path_buf(),
            "$generated_root" => generated_root.to_path_buf(),
            "$temp_root" => temp_root.to_path_buf(),
            "$cache_root" => cache_root.to_path_buf(),
            "" => {
                return Err(NimbusRuntimeError::Contract(format!(
                    "runtime {access} grant must not be empty"
                )));
            }
            literal => PathBuf::from(literal),
        };
        if roots.iter().all(|existing| existing != &root) {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn resolve_run_grants(
    grants: &[String],
    app_root: &Path,
    generated_root: &Path,
    cache_root: &Path,
) -> Result<Vec<PathBuf>> {
    let mut run_targets = Vec::new();
    for grant in grants {
        match grant.as_str() {
            "$discovered_tooling" => {
                run_targets.extend(discover_tooling_run_targets(
                    app_root,
                    generated_root,
                    cache_root,
                )?);
            }
            "$runtime_self_exec" => run_targets.push(runtime_self_exec_target(generated_root)?),
            "$runtime_host_exec" => run_targets.push(runtime_host_exec_target()?),
            "" => {
                return Err(NimbusRuntimeError::Contract(
                    "runtime run grant must not be empty".to_string(),
                ));
            }
            raw if raw.starts_with('$') => {
                return Err(NimbusRuntimeError::Contract(format!(
                    "unknown runtime run grant symbol `{raw}`"
                )));
            }
            raw => run_targets.push(
                canonicalize_preserving_missing_suffix(&PathBuf::from(raw))
                    .map_err(NimbusRuntimeError::Io)?,
            ),
        }
    }
    run_targets.sort();
    run_targets.dedup();
    Ok(run_targets)
}

fn infer_app_and_nimbus_roots(generated_root: &Path) -> (PathBuf, PathBuf) {
    let Some(nimbus_root) = generated_root.parent() else {
        return (generated_root.to_path_buf(), generated_root.join(".nimbus"));
    };
    if nimbus_root
        .file_name()
        .is_some_and(|name| name == ".nimbus")
    {
        let app_root = nimbus_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| generated_root.to_path_buf());
        return (app_root, nimbus_root.to_path_buf());
    }
    (generated_root.to_path_buf(), generated_root.join(".nimbus"))
}

fn discover_tooling_run_targets(
    app_root: &Path,
    generated_root: &Path,
    cache_root: &Path,
) -> Result<Vec<PathBuf>> {
    let mut run_targets = Vec::new();
    for search_root in [
        app_root.join("node_modules"),
        generated_root.join("node_modules"),
        cache_root.to_path_buf(),
    ] {
        collect_executable_files(&search_root, &mut run_targets)?;
    }
    Ok(run_targets)
}

fn collect_executable_files(root: &Path, run_targets: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    let mut pending = VecDeque::from([root.to_path_buf()]);
    while let Some(path) = pending.pop_front() {
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(NimbusRuntimeError::Contract(format!(
                    "failed to inspect runtime tooling run target {}: {error}",
                    path.display()
                )));
            }
        };
        if metadata.is_dir() {
            let entries = std::fs::read_dir(&path).map_err(|error| {
                NimbusRuntimeError::Contract(format!(
                    "failed to scan runtime tooling run roots under {}: {error}",
                    path.display()
                ))
            })?;
            for entry in entries {
                let entry = entry.map_err(|error| {
                    NimbusRuntimeError::Contract(format!(
                        "failed to enumerate runtime tooling run roots under {}: {error}",
                        path.display()
                    ))
                })?;
                pending.push_back(entry.path());
            }
            continue;
        }

        if !metadata.is_file() || !is_executable_candidate(&path, &metadata) {
            continue;
        }

        let canonical = canonicalize_preserving_missing_suffix(&path)?;
        if run_targets.iter().all(|existing| existing != &canonical) {
            run_targets.push(canonical);
        }
    }

    run_targets.sort();
    Ok(())
}

fn is_executable_candidate(path: &Path, metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let _ = path;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        let _ = metadata;
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "exe" | "cmd" | "bat" | "com" | "ps1"
                )
            })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, metadata);
        false
    }
}

fn canonicalize_roots(roots: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut canonical = Vec::new();
    for root in roots {
        let root = canonicalize_preserving_missing_suffix(&root)?;
        if canonical.iter().all(|existing| existing != &root) {
            canonical.push(root);
        }
    }
    Ok(canonical)
}

pub(super) fn path_resolve_error_from_io(error: std::io::Error) -> PathResolveError {
    match error.kind() {
        std::io::ErrorKind::NotFound => PathResolveError::NotFound(error),
        _ => PathResolveError::Canonicalize(error),
    }
}

pub(super) fn canonicalize_preserving_missing_suffix(path: &Path) -> std::io::Result<PathBuf> {
    canonicalize_preserving_missing_suffix_from_base(path, &std::env::current_dir()?)
}

fn normalize_absolute_path_lexically(path: &Path) -> PathBuf {
    let mut prefix = None::<OsString>;
    let mut has_root = false;
    let mut parts = Vec::<OsString>::new();

    for component in path.components() {
        match component {
            Component::Prefix(value) => {
                prefix = Some(value.as_os_str().to_os_string());
            }
            Component::RootDir => {
                has_root = true;
                parts.clear();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !parts.is_empty() {
                    parts.pop();
                }
            }
            Component::Normal(part) => parts.push(part.to_os_string()),
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if has_root {
        normalized.push(std::path::MAIN_SEPARATOR.to_string());
    }
    for part in parts {
        normalized.push(part);
    }
    normalized
}

pub(super) fn canonicalize_preserving_missing_suffix_from_base(
    path: &Path,
    base: &Path,
) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let absolute = normalize_absolute_path_lexically(&absolute);

    let mut current = absolute.as_path();
    let mut missing = VecDeque::<OsString>::new();
    loop {
        match current.canonicalize() {
            Ok(mut canonical) => {
                for segment in &missing {
                    canonical.push(segment);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let file_name = current.file_name().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!(
                            "path does not have an existing ancestor: {}",
                            path.display()
                        ),
                    )
                })?;
                missing.push_front(file_name.to_os_string());
                current = current.parent().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!(
                            "path does not have an existing ancestor: {}",
                            path.display()
                        ),
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

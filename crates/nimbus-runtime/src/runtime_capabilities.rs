use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use deno_permissions::{
    AllowRunDescriptor, AllowRunDescriptorParseResult, DenyRunDescriptor, EnvDescriptor,
    FfiDescriptor, ImportDescriptor, NetDescriptor, PathDescriptor, PathQueryDescriptor,
    PathResolveError, PermissionDescriptorParser, Permissions, PermissionsContainer,
    PermissionsOptions, ReadDescriptor, RunDescriptorParseError, RunQueryDescriptor,
    SpecialFilePathQueryDescriptor, SysDescriptor, SysDescriptorParseError, WriteDescriptor,
};
use serde::Serialize;
use sys_traits::impls::RealSys;

use crate::error::{NimbusRuntimeError, Result};
use crate::limits::{RuntimeGrants, RuntimeLimits};
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

#[derive(Debug, Clone)]
pub(crate) struct RuntimeEnvPolicy {
    allowed_names: BTreeSet<String>,
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

#[derive(Debug)]
struct RuntimePermissionDescriptorParser {
    cwd: PathBuf,
    sys: RealSys,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub(crate) enum RuntimeEnvLookupDescriptor {
    Allowed { value: String },
    Missing,
    Denied { message: String },
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

impl RuntimeEnvPolicy {
    pub(crate) fn for_grants(grants: &RuntimeGrants) -> Self {
        let allowed_names = grants.env_read.iter().cloned().collect();
        Self { allowed_names }
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<String, String> {
        self.allowed_names
            .iter()
            .filter_map(|name| std::env::var(name).ok().map(|value| (name.clone(), value)))
            .collect()
    }

    pub(crate) fn lookup(&self, name: &str) -> RuntimeEnvLookupDescriptor {
        if !is_valid_env_name(name) {
            return RuntimeEnvLookupDescriptor::Denied {
                message: format!(
                    "runtime env capability denied for invalid variable name `{name}`"
                ),
            };
        }
        if !self.allowed_names.contains(name) {
            return RuntimeEnvLookupDescriptor::Denied {
                message: format!(
                    "runtime env capability denied for `{name}`; env access is allowlist-only"
                ),
            };
        }
        match std::env::var(name) {
            Ok(value) => RuntimeEnvLookupDescriptor::Allowed { value },
            Err(std::env::VarError::NotPresent) => RuntimeEnvLookupDescriptor::Missing,
            Err(std::env::VarError::NotUnicode(_)) => RuntimeEnvLookupDescriptor::Denied {
                message: format!(
                    "runtime env capability denied for `{name}`; value is not valid UTF-8"
                ),
            },
        }
    }

    pub(crate) fn allowed_names(&self) -> Vec<String> {
        self.allowed_names.iter().cloned().collect()
    }
}

impl RuntimePermissionDescriptorParser {
    fn new(cwd: PathBuf) -> Self {
        Self { cwd, sys: RealSys }
    }

    fn resolve_canonical_path(
        &self,
        path: &Path,
    ) -> std::result::Result<PathBuf, PathResolveError> {
        canonicalize_preserving_missing_suffix_from_base(path, &self.cwd)
            .map_err(path_resolve_error_from_io)
    }

    fn parse_scoped_path_descriptor(
        &self,
        path: Cow<'_, Path>,
    ) -> std::result::Result<PathDescriptor, PathResolveError> {
        if path.as_os_str().as_encoded_bytes().is_empty() {
            return Err(PathResolveError::EmptyPath);
        }
        Ok(PathDescriptor::new_known_cwd(path, &self.cwd))
    }
}

impl PermissionDescriptorParser for RuntimePermissionDescriptorParser {
    fn parse_read_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<ReadDescriptor, PathResolveError> {
        Ok(self
            .parse_scoped_path_descriptor(Cow::Borrowed(Path::new(text)))?
            .into_read())
    }

    fn parse_write_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<WriteDescriptor, PathResolveError> {
        Ok(self
            .parse_scoped_path_descriptor(Cow::Borrowed(Path::new(text)))?
            .into_write())
    }

    fn parse_net_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<NetDescriptor, deno_permissions::NetDescriptorParseError> {
        NetDescriptor::parse_for_list(text)
    }

    fn parse_import_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<ImportDescriptor, deno_permissions::NetDescriptorParseError> {
        ImportDescriptor::parse_for_list(text)
    }

    fn parse_env_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<EnvDescriptor, deno_permissions::EnvDescriptorParseError> {
        if text.is_empty() {
            Err(deno_permissions::EnvDescriptorParseError)
        } else {
            Ok(EnvDescriptor::new(Cow::Borrowed(text)))
        }
    }

    fn parse_sys_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<SysDescriptor, SysDescriptorParseError> {
        if text.is_empty() {
            Err(SysDescriptorParseError::Empty)
        } else {
            SysDescriptor::parse(text.to_string())
        }
    }

    fn parse_allow_run_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<AllowRunDescriptorParseResult, RunDescriptorParseError> {
        if text.is_empty() {
            return Err(RunDescriptorParseError::EmptyRunQuery);
        }
        if AllowRunDescriptor::is_path(text) {
            let canonical = self.resolve_canonical_path(Path::new(text))?;
            return Ok(AllowRunDescriptorParseResult::Descriptor(
                AllowRunDescriptor(PathDescriptor::new_known_absolute(Cow::Owned(canonical))),
            ));
        }
        AllowRunDescriptor::parse(text, &self.cwd, &self.sys).map_err(Into::into)
    }

    fn parse_deny_run_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<DenyRunDescriptor, PathResolveError> {
        Ok(DenyRunDescriptor::parse(text, &self.cwd))
    }

    fn parse_ffi_descriptor(
        &self,
        text: &str,
    ) -> std::result::Result<FfiDescriptor, PathResolveError> {
        Ok(self
            .parse_scoped_path_descriptor(Cow::Borrowed(Path::new(text)))?
            .into_ffi())
    }

    fn parse_path_query<'a>(
        &self,
        path: Cow<'a, Path>,
    ) -> std::result::Result<PathQueryDescriptor<'a>, PathResolveError> {
        if path.as_os_str().as_encoded_bytes().is_empty() {
            return Err(PathResolveError::EmptyPath);
        }
        let requested = (!path.is_absolute()).then(|| path.to_string_lossy().into_owned());
        let resolved = if path.is_absolute() {
            path.into_owned()
        } else {
            self.cwd.join(path.as_ref())
        };
        let query = PathQueryDescriptor::new_known_absolute(Cow::Owned(resolved));
        Ok(match requested {
            Some(requested) => query.with_requested(requested),
            None => query,
        })
    }

    fn parse_special_file_descriptor<'a>(
        &self,
        path: PathQueryDescriptor<'a>,
    ) -> std::result::Result<SpecialFilePathQueryDescriptor<'a>, PathResolveError> {
        SpecialFilePathQueryDescriptor::parse(&self.sys, path)
    }

    fn parse_net_query(
        &self,
        text: &str,
    ) -> std::result::Result<NetDescriptor, deno_permissions::NetDescriptorParseError> {
        NetDescriptor::parse_for_query(text)
    }

    fn parse_run_query<'a>(
        &self,
        requested: &'a str,
    ) -> std::result::Result<RunQueryDescriptor<'a>, RunDescriptorParseError> {
        if requested.is_empty() {
            return Err(RunDescriptorParseError::EmptyRunQuery);
        }
        if AllowRunDescriptor::is_path(requested) {
            let canonical = self.resolve_canonical_path(Path::new(requested))?;
            return Ok(RunQueryDescriptor::Path(
                PathQueryDescriptor::new_known_absolute(Cow::Owned(canonical))
                    .with_requested(requested.to_string()),
            ));
        }
        RunQueryDescriptor::parse(requested, &self.sys).map_err(Into::into)
    }
}

pub(crate) fn build_permissions_container(
    paths: &RuntimePathPolicy,
    env: &RuntimeEnvPolicy,
    limits: &RuntimeLimits,
) -> Result<PermissionsContainer> {
    let parser = Arc::new(RuntimePermissionDescriptorParser::new(paths.cwd.clone()));
    let options = PermissionsOptions {
        allow_env: (!env.allowed_names.is_empty()).then(|| env.allowed_names()),
        deny_env: None,
        ignore_env: None,
        allow_net: allowed_net_descriptors(&limits.grants),
        deny_net: None,
        allow_ffi: (!limits.grants.ffi.is_empty()).then(|| limits.grants.ffi.clone()),
        deny_ffi: None,
        allow_read: (!paths.read_roots().is_empty()).then(|| {
            paths
                .read_roots()
                .iter()
                .map(|root| root.display().to_string())
                .collect()
        }),
        deny_read: None,
        ignore_read: None,
        allow_sys: (!limits.grants.sys.is_empty()).then(|| limits.grants.sys.clone()),
        deny_sys: None,
        allow_write: (!paths.write_roots().is_empty()).then(|| {
            paths
                .write_roots()
                .iter()
                .map(|root| root.display().to_string())
                .collect()
        }),
        deny_write: None,
        allow_run: (!paths.run_targets().is_empty()).then(|| {
            paths
                .run_targets()
                .iter()
                .map(|path| path.display().to_string())
                .collect()
        }),
        deny_run: None,
        allow_import: None,
        deny_import: None,
        prompt: false,
    };
    let permissions = Permissions::from_options(parser.as_ref(), &options).map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "failed to build runtime permission contract: {error}"
        ))
    })?;
    Ok(PermissionsContainer::new(parser, permissions))
}

fn allowed_net_descriptors(grants: &RuntimeGrants) -> Option<Vec<String>> {
    let mut descriptors = Vec::new();
    for grant in grants.net_connect.iter().chain(grants.net_listen.iter()) {
        if descriptors.iter().all(|existing| existing != grant) {
            descriptors.push(grant.clone());
        }
    }
    (!descriptors.is_empty()).then_some(descriptors)
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

fn path_resolve_error_from_io(error: std::io::Error) -> PathResolveError {
    match error.kind() {
        std::io::ErrorKind::NotFound => PathResolveError::NotFound(error),
        _ => PathResolveError::Canonicalize(error),
    }
}

fn canonicalize_preserving_missing_suffix(path: &Path) -> std::io::Result<PathBuf> {
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

fn canonicalize_preserving_missing_suffix_from_base(
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

fn is_valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeLimits;
    use deno_permissions::OpenAccessKind;
    use std::path::PathBuf;

    #[test]
    fn application_preset_roots_stay_within_generated_bundle_root() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_root = tempdir.path().join("app/.nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let limits = RuntimeLimits::application_node22();
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");

        let expected_cwd = bundle_root
            .canonicalize()
            .expect("bundle root should canonicalize");
        assert_eq!(policy.cwd(), expected_cwd.as_path());
        let expected_package_json = expected_cwd.join("package.json");
        let checked = permissions
            .check_open(
                Cow::Borrowed(Path::new("./package.json")),
                OpenAccessKind::Read,
                Some("test"),
            )
            .expect("read path should resolve");
        assert_eq!(checked.into_owned_path(), expected_package_json);
        let denied = permissions
            .check_open(
                Cow::Borrowed(Path::new("../package.json")),
                OpenAccessKind::Read,
                Some("test"),
            )
            .expect_err("parent traversal should be denied");
        assert!(
            denied.to_string().contains("Requires read access"),
            "unexpected error: {denied}"
        );
    }

    #[test]
    fn path_roots_are_driven_by_grants_not_preset_name() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let mut limits = RuntimeLimits::tooling_node22();
        limits.grants = RuntimeGrants::application_node();
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");

        let expected_cwd = bundle_root
            .canonicalize()
            .expect("bundle root should canonicalize");
        assert_eq!(
            policy.cwd(),
            expected_cwd.as_path(),
            "a tooling preset must not widen cwd without matching read grants"
        );
        let denied = policy
            .ensure_read_path_lexical(&app_root.join("package.json"))
            .expect_err("app-root read should require an app-root read grant");
        assert!(
            denied
                .to_string()
                .contains("runtime read capability denied"),
            "unexpected denial: {denied}"
        );
    }

    #[test]
    fn tooling_preset_uses_app_root_as_cwd_and_allows_tmp_writes() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let limits = RuntimeLimits::tooling_node22();
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");

        let expected_cwd = app_root
            .canonicalize()
            .expect("app root should canonicalize");
        assert_eq!(policy.cwd(), expected_cwd.as_path());
        let expected_tmp_write = expected_cwd.join(".nimbus/tmp/cache.txt");
        let checked = permissions
            .check_open(
                Cow::Borrowed(Path::new(".nimbus/tmp/cache.txt")),
                OpenAccessKind::Write,
                Some("test"),
            )
            .expect("tmp write should resolve");
        assert_eq!(checked.into_owned_path(), expected_tmp_write);
        let denied = permissions
            .check_open(
                Cow::Borrowed(Path::new("../outside.txt")),
                OpenAccessKind::Write,
                Some("test"),
            )
            .expect_err("escape write should be denied");
        assert!(
            denied.to_string().contains("Requires write access"),
            "unexpected error: {denied}"
        );
    }

    #[test]
    fn permissions_container_resolves_paths_from_runtime_scoped_cwd() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let limits = RuntimeLimits::tooling_node22();
        let paths =
            RuntimePathPolicy::for_bundle(&bundle, &limits).expect("path policy should build");
        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let permissions =
            build_permissions_container(&paths, &env, &limits).expect("permissions should build");

        let checked = permissions
            .check_open(
                Cow::Borrowed(Path::new(".nimbus/tmp/cache.txt")),
                OpenAccessKind::Write,
                Some("test"),
            )
            .expect("tmp path should be allowed");
        let expected = app_root
            .join(".nimbus/tmp/cache.txt")
            .canonicalize()
            .unwrap_or_else(|_| {
                canonicalize_preserving_missing_suffix(&app_root.join(".nimbus/tmp/cache.txt"))
                    .expect("expected path should canonicalize")
            });
        assert_eq!(checked.into_owned_path(), expected);

        let denied = permissions
            .check_open(
                Cow::Borrowed(Path::new("../outside.txt")),
                OpenAccessKind::Write,
                Some("test"),
            )
            .expect_err("parent traversal should be denied");
        assert!(
            denied.to_string().contains("Requires write access"),
            "unexpected error: {denied}"
        );
    }

    #[test]
    fn ensure_write_path_allows_in_root_parent_traversal_after_missing_segment() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let paths = RuntimePathPolicy::for_bundle(&bundle, &RuntimeLimits::application_node22())
            .expect("path policy should build");

        let checked = paths
            .ensure_write_path(Path::new("test10/../test11/test12"))
            .expect("in-root mkdir path should normalize");
        let expected = bundle_root
            .canonicalize()
            .expect("bundle root should canonicalize")
            .join("test11/test12");
        assert_eq!(checked, expected);
    }

    #[test]
    fn ensure_symlink_target_path_allows_in_root_relative_parent_traversal() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let paths = RuntimePathPolicy::for_bundle(&bundle, &RuntimeLimits::application_node22())
            .expect("path policy should build");

        let link_path = paths
            .ensure_write_path(Path::new("fixtures/a/symlink/a/b/c"))
            .expect("symlink destination should be allowed");
        let checked = paths
            .ensure_symlink_target_path(Path::new("../.."), &link_path)
            .expect("relative symlink target should normalize against the link parent");
        assert_eq!(checked, PathBuf::from("../.."));
    }

    #[test]
    fn ensure_read_metadata_path_denies_ancestor_of_approved_root() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let paths = RuntimePathPolicy::for_bundle(&bundle, &RuntimeLimits::application_node22())
            .expect("path policy should build");

        let error = paths
            .ensure_read_metadata_path(Path::new("/"))
            .expect_err("ancestor metadata should be denied outside approved roots");
        assert!(
            error
                .to_string()
                .contains("runtime read capability denied for /"),
            "unexpected metadata denial: {error}"
        );
    }

    #[test]
    fn application_preset_has_no_run_targets_and_denies_subprocess_queries() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let limits = RuntimeLimits::application_node22();
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        assert!(
            policy.run_targets().is_empty(),
            "application preset should not expose runnable targets"
        );

        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");
        let parser = RuntimePermissionDescriptorParser::new(policy.cwd().to_path_buf());
        let current_exec = std::env::current_exe().expect("current exec should resolve");
        let current_exec_query = current_exec.to_string_lossy().into_owned();
        let run_query = parser
            .parse_run_query(current_exec_query.as_str())
            .expect("current exec query should parse");
        let error = permissions
            .check_run(&run_query, "test")
            .expect_err("application preset should deny subprocess execution");
        assert!(
            error.to_string().contains("Requires run access"),
            "unexpected run denial: {error}"
        );
    }

    #[test]
    fn application_self_exec_run_grant_only_allows_compat_exec_target() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let mut limits = RuntimeLimits::application_node22();
        limits.grants.run = vec!["$runtime_self_exec".to_string()];
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        assert_eq!(
            policy.run_targets().len(),
            1,
            "self-exec grant should expose exactly one compat exec target"
        );

        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");
        let parser = RuntimePermissionDescriptorParser::new(policy.cwd().to_path_buf());

        let allowed_path = policy.run_targets()[0].to_string_lossy().into_owned();
        let allowed = parser
            .parse_run_query(allowed_path.as_str())
            .expect("self exec query should parse");
        permissions
            .check_run(&allowed, "test")
            .expect("self-exec target should be runnable");

        let current_exec = std::env::current_exe().expect("current exec should resolve");
        let current_exec_query = current_exec.to_string_lossy().into_owned();
        let denied = parser
            .parse_run_query(current_exec_query.as_str())
            .expect("host exec query should parse");
        let error = permissions
            .check_run(&denied, "test")
            .expect_err("self-exec grant should still deny host exec");
        assert!(
            error.to_string().contains("Requires run access"),
            "unexpected run denial: {error}"
        );
    }

    #[test]
    fn tooling_preset_discovers_staged_run_targets_and_denies_escape_runs() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        let binary_root = app_root.join("node_modules/esbuild/bin");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        std::fs::create_dir_all(&binary_root).expect("binary root should build");
        let binary_path = binary_root.join(binary_name());
        write_test_executable(&binary_path);

        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let limits = RuntimeLimits::tooling_node22();
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        assert!(
            policy.run_targets().contains(
                &binary_path
                    .canonicalize()
                    .expect("binary path should canonicalize")
            ),
            "tooling run targets should include staged package binaries"
        );

        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");
        let parser = RuntimePermissionDescriptorParser::new(policy.cwd().to_path_buf());

        let allowed_path = binary_path.to_string_lossy().into_owned();
        let allowed = parser
            .parse_run_query(allowed_path.as_str())
            .expect("binary query should parse");
        permissions
            .check_run(&allowed, "test")
            .expect("staged package binary should be runnable");

        let outside_binary = tempdir.path().join("outside").join(binary_name());
        std::fs::create_dir_all(
            outside_binary
                .parent()
                .expect("outside parent should exist"),
        )
        .expect("outside parent should build");
        write_test_executable(&outside_binary);
        let denied_path = outside_binary.to_string_lossy().into_owned();
        let denied = parser
            .parse_run_query(denied_path.as_str())
            .expect("outside query should parse");
        let error = permissions
            .check_run(&denied, "test")
            .expect_err("outside binary should be denied");
        assert!(
            error.to_string().contains("Requires run access"),
            "unexpected run denial: {error}"
        );
    }

    #[test]
    fn run_targets_are_driven_by_grants_not_preset_name() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let app_root = tempdir.path().join("app");
        let bundle_root = app_root.join(".nimbus/convex");
        let binary_root = app_root.join("node_modules/.bin");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        std::fs::create_dir_all(&binary_root).expect("binary root should build");
        let binary_path = binary_root.join(binary_name());
        write_test_executable(&binary_path);

        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let mut invalid_application_limits = RuntimeLimits::application_node22();
        invalid_application_limits.grants.run = vec!["$discovered_tooling".to_string()];
        assert!(
            std::panic::catch_unwind(|| invalid_application_limits.normalized()).is_err(),
            "$discovered_tooling should still require the Tooling preset guardrail"
        );

        let mut limits = RuntimeLimits::tooling_node22();
        limits.grants.run = vec![binary_path.to_string_lossy().into_owned()];
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        assert_eq!(
            policy.run_targets(),
            &[binary_path
                .canonicalize()
                .expect("binary path should canonicalize")]
        );
    }

    #[test]
    fn application_node22_permissions_allow_local_network_hosts() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_root = tempdir.path().join("app/.nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let limits = RuntimeLimits::application_node22();
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let mut permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");

        permissions
            .check_net(&("localhost", Some(8080)), "test")
            .expect("loopback hostname should be allowed");
        permissions
            .check_net(&("127.0.0.1", Some(8080)), "test")
            .expect("loopback ipv4 should be allowed");
        permissions
            .check_net(&("127.0.0.1", Some(0)), "test")
            .expect("loopback ipv4 ephemeral listen port should be allowed");
        permissions
            .check_net(&("0.0.0.0", Some(0)), "test")
            .expect("wildcard listen host should be allowed");
        permissions
            .check_sys("hostname", "test")
            .expect("hostname sys capability should be allowed");
    }

    #[test]
    fn node_network_permissions_are_driven_by_grants() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_root = tempdir.path().join("app/.nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let mut limits = RuntimeLimits::application_node22();
        limits.grants.net_connect.clear();
        limits.grants.net_listen.clear();
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let mut permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");

        let error = permissions
            .check_net(&("127.0.0.1", Some(8080)), "test")
            .expect_err("Node target should still require explicit net grants");
        assert!(
            error.to_string().contains("Requires net access"),
            "unexpected net denial: {error}"
        );
    }

    #[test]
    fn web_standard_permissions_deny_local_network_hosts() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_root = tempdir.path().join("app/.nimbus/convex");
        std::fs::create_dir_all(&bundle_root).expect("bundle root should build");
        let bundle_path = bundle_root.join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);

        let limits = RuntimeLimits::application_web_standard();
        let policy = RuntimePathPolicy::for_bundle(&bundle, &limits).expect("policy should build");
        let env = RuntimeEnvPolicy::for_grants(&limits.grants);
        let mut permissions =
            build_permissions_container(&policy, &env, &limits).expect("permissions should build");

        let error = permissions
            .check_net(&("127.0.0.1", Some(8080)), "test")
            .expect_err("web-standard runtime should still deny raw net access");
        assert!(
            error.to_string().contains("Requires net access"),
            "unexpected net denial: {error}"
        );
    }

    #[cfg(unix)]
    fn binary_name() -> &'static str {
        "esbuild"
    }

    #[cfg(windows)]
    fn binary_name() -> &'static str {
        "esbuild.cmd"
    }

    fn write_test_executable(path: &PathBuf) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("test executable should write");
            let mut permissions = std::fs::metadata(path)
                .expect("test executable metadata should load")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions)
                .expect("test executable permissions should update");
        }
        #[cfg(windows)]
        {
            std::fs::write(path, "@echo off\r\nexit /b 0\r\n")
                .expect("test executable should write");
        }
    }
}

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deno_permissions::{
    AllowRunDescriptor, AllowRunDescriptorParseResult, DenyRunDescriptor, EnvDescriptor,
    FfiDescriptor, ImportDescriptor, NetDescriptor, PathDescriptor, PathQueryDescriptor,
    PathResolveError, PermissionDescriptorParser, Permissions, PermissionsContainer,
    PermissionsOptions, ReadDescriptor, RunDescriptorParseError, RunQueryDescriptor,
    SpecialFilePathQueryDescriptor, SysDescriptor, SysDescriptorParseError, WriteDescriptor,
};
use sys_traits::impls::RealSys;

use crate::error::{NimbusRuntimeError, Result};
use crate::limits::{RuntimeGrants, RuntimeLimits};

use super::env::RuntimeEnvPolicy;
use super::paths::{
    RuntimePathPolicy, canonicalize_preserving_missing_suffix_from_base, path_resolve_error_from_io,
};

#[derive(Debug)]
pub(super) struct RuntimePermissionDescriptorParser {
    cwd: PathBuf,
    sys: RealSys,
}

impl RuntimePermissionDescriptorParser {
    pub(super) fn new(cwd: PathBuf) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimePermissionProfile {
    grants: RuntimeGrants,
    ambient_authority_allowed: bool,
}

impl RuntimePermissionProfile {
    pub(crate) fn for_limits(limits: &RuntimeLimits) -> Self {
        Self {
            grants: limits.grants.clone(),
            ambient_authority_allowed: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn ambient_denied(limits: &RuntimeLimits) -> Self {
        Self {
            grants: limits.grants.clone(),
            ambient_authority_allowed: false,
        }
    }

    fn permissions_options(
        &self,
        paths: &RuntimePathPolicy,
        env: &RuntimeEnvPolicy,
    ) -> PermissionsOptions {
        PermissionsOptions {
            allow_env: env.has_allowed_read_names().then(|| env.allowed_names()),
            deny_env: None,
            ignore_env: None,
            allow_net: self
                .ambient_authority_allowed
                .then(|| allowed_net_descriptors(&self.grants))
                .flatten(),
            deny_net: None,
            allow_ffi: (self.ambient_authority_allowed && !self.grants.ffi.is_empty())
                .then(|| self.grants.ffi.clone()),
            deny_ffi: None,
            allow_read: (self.ambient_authority_allowed && !paths.read_roots().is_empty()).then(
                || {
                    paths
                        .read_roots()
                        .iter()
                        .map(|root| root.display().to_string())
                        .collect()
                },
            ),
            deny_read: None,
            ignore_read: None,
            allow_sys: (!self.grants.sys.is_empty()).then(|| self.grants.sys.clone()),
            deny_sys: None,
            allow_write: (self.ambient_authority_allowed && !paths.write_roots().is_empty()).then(
                || {
                    paths
                        .write_roots()
                        .iter()
                        .map(|root| root.display().to_string())
                        .collect()
                },
            ),
            deny_write: None,
            allow_run: (self.ambient_authority_allowed && !paths.run_targets().is_empty()).then(
                || {
                    paths
                        .run_targets()
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect()
                },
            ),
            deny_run: None,
            allow_import: None,
            deny_import: None,
            prompt: false,
        }
    }
}

/// Worker permission contract: the configured `RuntimePermissionProfile`
/// carries the effective grants into isolate construction. Query, mutation,
/// and action invocations intentionally share the same profile until a future
/// product contract introduces narrower per-kind grants; today net/fs/ffi/run
/// authority is denied unless the active `RuntimeLimits` grant it explicitly.
pub(crate) fn build_permissions_container(
    paths: &RuntimePathPolicy,
    env: &RuntimeEnvPolicy,
    limits: &RuntimeLimits,
) -> Result<PermissionsContainer> {
    build_permissions_container_for_profile(
        paths,
        env,
        RuntimePermissionProfile::for_limits(limits),
    )
}

/// Test fixture: a container whose configured ambient grants (read/write/
/// net/ffi/run) are withheld, used to prove denial propagation paths.
#[cfg(test)]
pub(crate) fn build_ambient_denied_permissions_container(
    paths: &RuntimePathPolicy,
    env: &RuntimeEnvPolicy,
    limits: &RuntimeLimits,
) -> Result<PermissionsContainer> {
    build_permissions_container_for_profile(
        paths,
        env,
        RuntimePermissionProfile::ambient_denied(limits),
    )
}

fn build_permissions_container_for_profile(
    paths: &RuntimePathPolicy,
    env: &RuntimeEnvPolicy,
    profile: RuntimePermissionProfile,
) -> Result<PermissionsContainer> {
    let parser = Arc::new(RuntimePermissionDescriptorParser::new(
        paths.cwd().to_path_buf(),
    ));
    let options = profile.permissions_options(paths, env);
    let permissions = Permissions::from_options(parser.as_ref(), &options).map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "failed to build runtime permission contract: {error}"
        ))
    })?;
    Ok(PermissionsContainer::new(parser, permissions))
}

pub(crate) fn build_module_read_permissions_container(
    paths: &RuntimePathPolicy,
) -> Result<PermissionsContainer> {
    let parser = Arc::new(RuntimePermissionDescriptorParser::new(
        paths.cwd().to_path_buf(),
    ));
    let options = PermissionsOptions {
        allow_env: None,
        deny_env: None,
        ignore_env: None,
        allow_net: None,
        deny_net: None,
        allow_ffi: None,
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
        allow_sys: None,
        deny_sys: None,
        allow_write: None,
        deny_write: None,
        allow_run: None,
        deny_run: None,
        allow_import: None,
        deny_import: None,
        prompt: false,
    };
    let permissions = Permissions::from_options(parser.as_ref(), &options).map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "failed to build runtime module-read permission contract: {error}"
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

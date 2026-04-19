use super::readiness::visible_published_endpoints;
use super::*;

impl KrunSandboxBackend {
    pub(super) fn plan_start(&self, spec: &SandboxSpec) -> Result<KrunLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        self.plan_start_with_id(spec, &sandbox_id, None, None)
    }

    pub(super) fn plan_start_from_image(
        &self,
        spec: &SandboxSpec,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<KrunLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        let prepared_launch = self.prepare_image_launch(&sandbox_id, image_reference, overrides)?;
        self.plan_start_with_materialized_launch(spec, &sandbox_id, prepared_launch)
    }

    pub(super) fn plan_start_from_build(
        &self,
        spec: &SandboxSpec,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<KrunLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        let prepared_launch = self.prepare_built_image_launch(
            &sandbox_id,
            image_name,
            dockerfile_path,
            context_path,
            overrides,
        )?;
        self.plan_start_with_materialized_launch(spec, &sandbox_id, prepared_launch)
    }

    #[cfg(test)]
    pub(super) fn plan_start_with_launch_defaults(
        &self,
        spec: &SandboxSpec,
        launch_defaults: Option<&OciImageLaunchDefaults>,
    ) -> Result<KrunLaunchPlan> {
        let sandbox_id = next_sandbox_id(&spec.name);
        self.plan_start_with_id(spec, &sandbox_id, launch_defaults, None)
    }

    fn plan_start_with_materialized_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedMaterializedImageLaunch,
    ) -> Result<KrunLaunchPlan> {
        self.plan_start_with_id(
            spec,
            sandbox_id,
            Some(&prepared_launch.launch_defaults),
            Some(KrunLaunchArtifact::Rootfs(prepared_launch.artifact)),
        )
    }

    pub(super) fn plan_start_with_id(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        launch_defaults: Option<&OciImageLaunchDefaults>,
        launch_artifact: Option<KrunLaunchArtifact>,
    ) -> Result<KrunLaunchPlan> {
        if spec.backend != SandboxBackendKind::Krun {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "krun backend cannot lower sandbox spec for backend {:?}",
                    spec.backend
                ),
            });
        }

        let mut resolved_launch = resolve_launch_spec(spec, launch_defaults);
        apply_guest_user_switch(&mut resolved_launch.spec, &resolved_launch.image_metadata)?;
        let bundle_layout =
            KrunBundleLayout::new(self.config.bundle_root.join(sandbox_id.as_str()));
        write_bundle_config(
            &bundle_layout,
            &hostname_for(&resolved_launch.spec),
            &resolved_launch.spec,
            &KrunBundleOptions {
                additional_mounts: guest_user_switch_mounts(
                    &self.config,
                    &resolved_launch.image_metadata,
                ),
            },
        )?;

        let conmon_layout = OciConmonLayout::new(&self.config.state_root, sandbox_id);
        conmon_layout
            .ensure_directories()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create krun state directories under {}: {error}",
                    self.config.state_root.display()
                ),
            })?;

        let conmon_launch = build_launch_plan(
            &OciConmonConfig {
                conmon_path: self.config.conmon_path.clone(),
                runtime_path: self.config.runtime_path.clone(),
                buildah_path: self.config.buildah_path.clone(),
                use_buildah_unshare: launch_artifact
                    .as_ref()
                    .is_some_and(KrunLaunchArtifact::uses_mount_session_unshare)
                    && self.config.use_buildah_unshare,
                log_level: self.config.log_level.clone(),
            },
            &conmon_layout,
            sandbox_id,
            &spec.name,
            &bundle_layout.bundle_dir,
            launch_artifact
                .as_ref()
                .and_then(KrunLaunchArtifact::mount_session_name),
            &krun_vm_config_prelude(
                &resolved_launch.spec,
                launch_artifact
                    .as_ref()
                    .is_some_and(KrunLaunchArtifact::uses_mount_session_unshare)
                    && self.config.use_buildah_unshare,
            )?,
        );

        let handle = SandboxHandle::new(
            sandbox_id.clone(),
            resolved_launch.spec.name.clone(),
            SandboxBackendKind::Krun,
            SandboxStatus::Starting,
            visible_published_endpoints(
                self.config.launch_mode,
                &resolved_launch.spec,
                SandboxStatus::Starting,
            ),
        );
        let manifest = KrunSandboxManifest {
            handle,
            spec: resolved_launch.spec,
            image_metadata: resolved_launch.image_metadata,
            launch_artifact,
            bundle_layout,
            conmon_layout,
            conmon_launch,
            last_exit_code: None,
            restart_count: 0,
            next_restart_at_millis: None,
            launch_mode: self.config.launch_mode,
            shutdown_requested: false,
            status: SandboxStatus::Starting,
        };

        Ok(KrunLaunchPlan { manifest })
    }

    fn prepare_image_launch(
        &self,
        sandbox_id: &SandboxId,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciImageMaterializer::under_state_root(&self.config.state_root).prepare_image_launch(
            sandbox_id,
            image_reference,
            overrides,
        )
    }

    fn prepare_built_image_launch(
        &self,
        sandbox_id: &SandboxId,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciDockerfileBuilder::under_state_root(&self.config.state_root).prepare_built_image_launch(
            sandbox_id,
            image_name,
            dockerfile_path,
            context_path,
            overrides,
        )
    }

    fn buildah_cli(&self) -> BuildahCli {
        let buildah = BuildahCli::new(self.config.buildah_path.clone());
        #[cfg(test)]
        let buildah = buildah.with_launcher_args(self.config.buildah_launcher_args.clone());
        buildah.with_unshare(self.config.use_buildah_unshare)
    }

    pub(super) fn cleanup_manifest_launch_artifacts(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        let Some(artifact) = manifest.launch_artifact.as_ref() else {
            return Ok(());
        };
        match artifact {
            KrunLaunchArtifact::MountedRootfs(session) => {
                self.buildah_cli()
                    .cleanup_rootfs_session(&session.session_name)?;
            }
            KrunLaunchArtifact::Rootfs(rootfs) => {
                if !rootfs.rootfs_path.exists() {
                    return Ok(());
                }
                std::fs::remove_dir_all(&rootfs.rootfs_path).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove materialized krun rootfs {}: {error}",
                            rootfs.rootfs_path.display()
                        ),
                    }
                })?;
            }
        }
        Ok(())
    }

    pub(super) fn materialize_auto_port_bindings(
        &self,
        manifest: &mut KrunSandboxManifest,
    ) -> Result<()> {
        let auto_bindings = self.port_manager().allocate_missing_bindings(
            &manifest.spec.port_bindings,
            &manifest.image_metadata.exposed_ports,
        )?;
        if auto_bindings.is_empty() {
            return Ok(());
        }

        manifest.spec.port_bindings.extend(auto_bindings);
        manifest.handle.published_endpoints =
            visible_published_endpoints(manifest.launch_mode, &manifest.spec, manifest.status);
        write_bundle_config(
            &manifest.bundle_layout,
            &hostname_for(&manifest.spec),
            &manifest.spec,
            &KrunBundleOptions {
                additional_mounts: guest_user_switch_mounts(&self.config, &manifest.image_metadata),
            },
        )
    }

    pub(super) fn materialize_krun_vm_config(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        if manifest
            .launch_artifact
            .as_ref()
            .is_some_and(KrunLaunchArtifact::uses_mount_session_unshare)
            && self.config.use_buildah_unshare
        {
            return Ok(());
        }

        let vm_config_path = krun_vm_config_path(&manifest.spec.filesystem.rootfs);
        match desired_krun_vm_config(&manifest.spec)? {
            Some(vm_config) => {
                let rendered = serde_json::to_vec_pretty(&vm_config).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!("failed to serialize krun vm config: {error}"),
                    }
                })?;
                std::fs::write(&vm_config_path, rendered).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to write krun vm config {}: {error}",
                            vm_config_path.display()
                        ),
                    }
                })
            }
            None => {
                if !vm_config_path.exists() {
                    return Ok(());
                }
                std::fs::remove_file(&vm_config_path).map_err(|error| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove stale krun vm config {}: {error}",
                            vm_config_path.display()
                        ),
                    }
                })
            }
        }
    }

    fn port_manager(&self) -> PortManager {
        PortManager::new(
            self.config.state_root.clone(),
            self.config.published_port_range.clone(),
        )
    }
}

fn next_sandbox_id(name: &str) -> SandboxId {
    SandboxId::new(format!(
        "{}-{}",
        slugify(name),
        Ulid::new().to_string().to_ascii_lowercase()
    ))
}

fn hostname_for(spec: &SandboxSpec) -> String {
    let slug = slugify(&spec.name);
    if slug.is_empty() {
        "neovex-sandbox".to_owned()
    } else {
        slug
    }
}

pub(super) fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

pub(super) fn desired_krun_vm_config(spec: &SandboxSpec) -> Result<Option<KrunVmConfig>> {
    let cpu_count = spec.resources.cpu_count;
    let memory_limit_bytes = spec.resources.memory_limit_bytes;

    match (cpu_count, memory_limit_bytes) {
        (None, _) => Ok(None),
        (Some(_), None) => Err(SandboxError::InvalidSpec {
            message:
                "krun sandbox cpu_count requires memory_limit_bytes so crun can configure /.krun_vm.json"
                    .to_owned(),
        }),
        (Some(0), _) => Err(SandboxError::InvalidSpec {
            message: "krun sandbox cpu_count must be greater than zero".to_owned(),
        }),
        (Some(_), Some(0)) => Err(SandboxError::InvalidSpec {
            message: "krun sandbox memory_limit_bytes must be greater than zero".to_owned(),
        }),
        (Some(cpus), Some(memory_limit_bytes)) => {
            let ram_mib = memory_limit_bytes.div_ceil(BYTES_PER_MIB);
            let ram_mib = u32::try_from(ram_mib).map_err(|_| SandboxError::InvalidSpec {
                message: format!(
                    "krun sandbox memory_limit_bytes {memory_limit_bytes} exceeds the maximum supported MiB range"
                ),
            })?;
            Ok(Some(KrunVmConfig { cpus, ram_mib }))
        }
    }
}

pub(super) fn krun_vm_config_path(rootfs: &Path) -> PathBuf {
    rootfs.join(KRUN_VM_CONFIG_FILENAME)
}

fn krun_vm_config_prelude(spec: &SandboxSpec, needs_unshare_mount: bool) -> Result<Vec<String>> {
    if !needs_unshare_mount {
        return Ok(Vec::new());
    }

    let vm_config_path = krun_vm_config_path(&spec.filesystem.rootfs);
    let escaped_path = shell_escape(vm_config_path.to_string_lossy().as_ref());
    match desired_krun_vm_config(spec)? {
        Some(vm_config) => {
            let rendered = json!({
                "cpus": vm_config.cpus,
                "ram_mib": vm_config.ram_mib,
            })
            .to_string();
            Ok(vec![format!(
                "printf '%s' {} > {}",
                shell_escape(&rendered),
                escaped_path,
            )])
        }
        None => Ok(vec![format!("rm -f {escaped_path}")]),
    }
}

fn resolve_launch_spec(
    spec: &SandboxSpec,
    launch_defaults: Option<&OciImageLaunchDefaults>,
) -> KrunResolvedLaunchSpec {
    let Some(launch_defaults) = launch_defaults else {
        return KrunResolvedLaunchSpec {
            spec: spec.clone(),
            image_metadata: KrunImageMetadata::default(),
        };
    };

    let mut resolved_spec = spec.clone();
    resolved_spec.filesystem =
        resolve_filesystem_spec(&spec.filesystem, &launch_defaults.filesystem);
    resolved_spec.process = resolve_process_spec(&spec.process, &launch_defaults.process);

    KrunResolvedLaunchSpec {
        spec: resolved_spec,
        image_metadata: KrunImageMetadata {
            user: launch_defaults.user.clone(),
            stop_signal: launch_defaults.stop_signal.clone(),
            healthcheck: launch_defaults.healthcheck.clone(),
            labels: launch_defaults.labels.clone(),
            exposed_ports: launch_defaults.exposed_ports.clone(),
        },
    }
}

fn resolve_filesystem_spec(
    spec: &crate::spec::SandboxFilesystemSpec,
    defaults: &crate::spec::SandboxFilesystemSpec,
) -> crate::spec::SandboxFilesystemSpec {
    if !spec.is_unspecified() {
        return spec.clone();
    }

    let mut resolved = defaults.clone();
    resolved.readonly = resolved.readonly || spec.readonly;
    resolved
}

fn resolve_process_spec(
    spec: &crate::spec::SandboxProcessSpec,
    defaults: &crate::spec::SandboxProcessSpec,
) -> crate::spec::SandboxProcessSpec {
    let mut resolved = defaults.clone();

    if !spec.args.is_empty() {
        resolved.args = spec.args.clone();
    }

    resolved.env = if spec.env.is_empty() || spec.uses_default_env() {
        defaults.env.clone()
    } else {
        merge_env_overrides(&defaults.env, &spec.env)
    };

    if !spec.uses_default_cwd() {
        resolved.cwd = spec.cwd.clone();
    }

    resolved.terminal = spec.terminal || defaults.terminal;
    resolved
}

fn apply_guest_user_switch(
    spec: &mut SandboxSpec,
    image_metadata: &KrunImageMetadata,
) -> Result<()> {
    let Some(target_user) = parse_guest_user(image_metadata.user.as_deref())? else {
        return Ok(());
    };

    if spec
        .process
        .args
        .first()
        .is_none_or(|arg| arg != GUEST_USER_HELPER_GUEST_PATH)
    {
        spec.process
            .args
            .insert(0, GUEST_USER_HELPER_GUEST_PATH.to_owned());
    }

    spec.process.env = merge_env_overrides(
        &spec.process.env,
        &[
            format!("{GUEST_USER_UID_ENV}={}", target_user.uid),
            format!("{GUEST_USER_GID_ENV}={}", target_user.gid),
        ],
    );

    Ok(())
}

fn guest_user_switch_mounts(
    config: &KrunSandboxBackendConfig,
    image_metadata: &KrunImageMetadata,
) -> Vec<KrunBundleMount> {
    if image_metadata
        .user
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Vec::new();
    }

    vec![KrunBundleMount {
        destination: GUEST_USER_HELPER_GUEST_ROOT.to_owned(),
        source: config.guest_user_helper_root.clone(),
        options: vec!["rbind".to_owned(), "ro".to_owned()],
    }]
}

fn merge_env_overrides(base: &[String], overrides: &[String]) -> Vec<String> {
    let mut merged = base.to_vec();
    for override_entry in overrides {
        let Some(override_key) = env_key(override_entry) else {
            merged.push(override_entry.clone());
            continue;
        };

        if let Some(index) = merged
            .iter()
            .position(|entry| env_key(entry).is_some_and(|key| key == override_key))
        {
            merged[index] = override_entry.clone();
        } else {
            merged.push(override_entry.clone());
        }
    }
    merged
}

fn env_key(entry: &str) -> Option<&str> {
    let (key, _) = entry.split_once('=')?;
    (!key.is_empty()).then_some(key)
}

pub(super) fn parse_guest_user(user: Option<&str>) -> Result<Option<GuestUserIds>> {
    let Some(user) = user.map(str::trim).filter(|user| !user.is_empty()) else {
        return Ok(None);
    };

    let (uid, gid) = match user.split_once(':') {
        Some((uid, gid)) => (
            parse_guest_user_id("uid", uid, user)?,
            parse_guest_user_id("gid", gid, user)?,
        ),
        None => (parse_guest_user_id("uid", user, user)?, 0),
    };

    Ok(Some(GuestUserIds { uid, gid }))
}

fn parse_guest_user_id(kind: &str, value: &str, user: &str) -> Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| SandboxError::InvalidSpec {
            message: format!(
                "krun guest-side user switching requires a numeric image user, got {user:?} with invalid {kind} component {value:?}"
            ),
        })
}

pub(super) fn ensure_guest_user_helper_available(
    config: &KrunSandboxBackendConfig,
    manifest: &KrunSandboxManifest,
) -> Result<()> {
    if manifest
        .image_metadata
        .user
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Ok(());
    }

    let helper_path = config
        .guest_user_helper_root
        .join(GUEST_USER_HELPER_BINARY_NAME);
    if helper_path.is_file() {
        return Ok(());
    }

    Err(SandboxError::OperationFailed {
        message: format!(
            "sandbox {} requires guest-side user switching, but helper {} is missing",
            manifest.handle.id,
            helper_path.display()
        ),
    })
}

fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_owned();
    }
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'/' || b == b'.')
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

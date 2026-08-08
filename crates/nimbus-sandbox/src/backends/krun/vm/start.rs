use super::readiness::visible_published_endpoints;
use super::*;

struct KrunStartPlanningOptions<'a> {
    launch_defaults: Option<&'a OciImageLaunchDefaults>,
    launch_artifact: Option<KrunLaunchArtifact>,
    provision_network_plan: Option<&'a SandboxProvisionNetworkPlan>,
    prepare_bundle: bool,
}

struct KrunInitialPublicationHooks<Before, After> {
    before_initial_publication: Before,
    after_network_reservation: After,
}

fn no_initial_publication_hook(_manifest: &KrunSandboxManifest) -> Result<()> {
    Ok(())
}

impl KrunSandboxBackend {
    #[cfg(test)]
    pub(super) fn plan_start(&self, spec: &SandboxSpec) -> Result<KrunStartPlan> {
        self.ensure_startup_network_reconciliation_ready()?;
        let sandbox_id = next_sandbox_id(spec.display_name());
        let _lifecycle = self.lock_launch_lifecycle_for(&spec.tenant_id, &sandbox_id)?;
        match &spec.root {
            SandboxRootSpec::Rootfs(_) => {
                self.plan_start_with_id_under_lock(spec, &sandbox_id, None, None)
            }
            SandboxRootSpec::OciImage(image) => {
                self.resource_quota_manager().ensure_launch_quota(spec)?;
                let prepared_launch =
                    self.prepare_oci_image_start(spec, &sandbox_id, &image.source)?;
                self.plan_start_with_materialized_image_under_lock(
                    spec,
                    &sandbox_id,
                    prepared_launch,
                )
            }
        }
    }

    #[cfg(test)]
    pub(super) fn plan_start_with_launch_defaults(
        &self,
        spec: &SandboxSpec,
        launch_defaults: Option<&OciImageLaunchDefaults>,
    ) -> Result<KrunStartPlan> {
        let sandbox_id = next_sandbox_id(spec.display_name());
        self.plan_start_with_id(spec, &sandbox_id, launch_defaults, None)
    }

    #[cfg(test)]
    fn plan_start_with_materialized_image_under_lock(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedMaterializedImageLaunch,
    ) -> Result<KrunStartPlan> {
        self.plan_start_with_materialized_image_at_initial_publication(
            spec,
            sandbox_id,
            prepared_launch,
            |_| Ok(()),
            |_| Ok(()),
        )
    }

    #[cfg(test)]
    pub(super) fn plan_start_with_materialized_image_at_initial_publication_for_test(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedMaterializedImageLaunch,
        before_initial_publication: impl FnOnce(&KrunSandboxManifest) -> Result<()>,
    ) -> Result<KrunStartPlan> {
        let _lifecycle = self.lock_launch_lifecycle_for(&spec.tenant_id, sandbox_id)?;
        self.plan_start_with_materialized_image_at_initial_publication(
            spec,
            sandbox_id,
            prepared_launch,
            before_initial_publication,
            |_| Ok(()),
        )
    }

    #[cfg(test)]
    fn plan_start_with_materialized_image_at_initial_publication(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        prepared_launch: PreparedMaterializedImageLaunch,
        before_initial_publication: impl FnOnce(&KrunSandboxManifest) -> Result<()>,
        after_network_reservation: impl FnOnce(&KrunSandboxManifest) -> Result<()>,
    ) -> Result<KrunStartPlan> {
        let materialized_artifact = prepared_launch.artifact.clone();
        let result = self.plan_start_with_id_under_lock_at_initial_publication(
            spec,
            sandbox_id,
            KrunStartPlanningOptions {
                launch_defaults: Some(&prepared_launch.launch_defaults),
                launch_artifact: Some(KrunLaunchArtifact::Rootfs(prepared_launch.artifact)),
                provision_network_plan: None,
                prepare_bundle: true,
            },
            KrunInitialPublicationHooks {
                before_initial_publication,
                after_network_reservation,
            },
        );
        match result {
            Ok(plan) => Ok(plan),
            Err(primary) => Err(self.compensate_unpublished_materialized_launch(
                spec,
                sandbox_id,
                &materialized_artifact,
                primary,
            )),
        }
    }

    #[cfg(test)]
    fn compensate_unpublished_materialized_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        artifact: &MaterializedImageRootfs,
        primary: SandboxError,
    ) -> SandboxError {
        match self.cleanup_unpublished_materialized_launch(spec, sandbox_id, artifact) {
            Ok(()) => primary,
            Err(cleanup) => SandboxError::OperationFailed {
                message: format!(
                    "krun image-backed launch failed: {primary}; unpublished materialized rootfs \
                     cleanup also failed: {cleanup}"
                ),
            },
        }
    }

    #[cfg(test)]
    fn cleanup_unpublished_materialized_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        artifact: &MaterializedImageRootfs,
    ) -> Result<()> {
        let manifest_path = crate::artifact_paths::manifest_path(
            &self.config.workload_state_root,
            &spec.tenant_id,
            sandbox_id,
        );
        match std::fs::symlink_metadata(&manifest_path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                // Once the canonical manifest exists, its lifecycle state owns
                // compensation. The unstarted-launch path records each effect
                // and must remain the only authority allowed to remove the
                // artifact it durably references.
                return Ok(());
            }
            Ok(_) => {
                // A non-regular canonical target cannot be a published
                // manifest. It may have caused the atomic publication failure,
                // but it has no authority over this exact launch artifact.
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to inspect initial krun manifest publication {} before rootfs \
                         cleanup: {error}",
                        manifest_path.display()
                    ),
                });
            }
        }
        OciImageMaterializer::for_tenant_sandbox(
            &self.config.workload_state_root,
            &spec.tenant_id,
            sandbox_id,
        )
        .remove_owned_artifact(sandbox_id, artifact)
    }

    #[cfg(test)]
    pub(super) fn plan_start_with_id(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        launch_defaults: Option<&OciImageLaunchDefaults>,
        launch_artifact: Option<KrunLaunchArtifact>,
    ) -> Result<KrunStartPlan> {
        self.ensure_startup_network_reconciliation_ready()?;
        let _lifecycle = self.lock_launch_lifecycle_for(&spec.tenant_id, sandbox_id)?;
        self.plan_start_with_id_under_lock(spec, sandbox_id, launch_defaults, launch_artifact)
    }

    #[cfg(test)]
    fn plan_start_with_id_under_lock(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        launch_defaults: Option<&OciImageLaunchDefaults>,
        launch_artifact: Option<KrunLaunchArtifact>,
    ) -> Result<KrunStartPlan> {
        self.plan_start_with_id_under_lock_at_initial_publication(
            spec,
            sandbox_id,
            KrunStartPlanningOptions {
                launch_defaults,
                launch_artifact,
                provision_network_plan: None,
                prepare_bundle: true,
            },
            KrunInitialPublicationHooks {
                before_initial_publication: no_initial_publication_hook,
                after_network_reservation: no_initial_publication_hook,
            },
        )
    }

    pub(super) fn plan_reserved_provision_with_id(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<KrunStartPlan> {
        self.ensure_startup_network_reconciliation_ready()?;
        if self.config.start_mode != KrunStartMode::Execute {
            return Err(SandboxError::InvalidSpec {
                message: "krun provision phases require execute mode".to_owned(),
            });
        }
        let _lifecycle = self.lock_launch_lifecycle_for(&spec.tenant_id, sandbox_id)?;
        self.plan_start_with_id_under_lock_at_initial_publication(
            spec,
            sandbox_id,
            KrunStartPlanningOptions {
                launch_defaults: None,
                launch_artifact: None,
                provision_network_plan: Some(network_plan),
                prepare_bundle: false,
            },
            KrunInitialPublicationHooks {
                before_initial_publication: no_initial_publication_hook,
                after_network_reservation: no_initial_publication_hook,
            },
        )
    }

    #[cfg(test)]
    pub(super) fn plan_start_with_id_at_reserved_publication_for_test(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        after_network_reservation: impl FnOnce(&KrunSandboxManifest) -> Result<()>,
    ) -> Result<KrunStartPlan> {
        self.ensure_startup_network_reconciliation_ready()?;
        let _lifecycle = self.lock_launch_lifecycle_for(&spec.tenant_id, sandbox_id)?;
        self.plan_start_with_id_under_lock_at_initial_publication(
            spec,
            sandbox_id,
            KrunStartPlanningOptions {
                launch_defaults: None,
                launch_artifact: None,
                provision_network_plan: None,
                prepare_bundle: true,
            },
            KrunInitialPublicationHooks {
                before_initial_publication: no_initial_publication_hook,
                after_network_reservation,
            },
        )
    }

    fn plan_start_with_id_under_lock_at_initial_publication<Before, After>(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        options: KrunStartPlanningOptions<'_>,
        hooks: KrunInitialPublicationHooks<Before, After>,
    ) -> Result<KrunStartPlan>
    where
        Before: FnOnce(&KrunSandboxManifest) -> Result<()>,
        After: FnOnce(&KrunSandboxManifest) -> Result<()>,
    {
        let KrunStartPlanningOptions {
            launch_defaults,
            launch_artifact,
            provision_network_plan,
            prepare_bundle,
        } = options;
        let KrunInitialPublicationHooks {
            before_initial_publication,
            after_network_reservation,
        } = hooks;
        self.ensure_startup_network_reconciliation_ready()?;
        if spec.backend != SandboxBackendKind::Krun {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "krun backend cannot lower sandbox spec for backend {:?}",
                    spec.backend
                ),
            });
        }
        let mut resolved_launch = resolve_start_spec(spec, launch_defaults)?;
        apply_guest_user_switch(&mut resolved_launch.spec, &resolved_launch.image_metadata)?;
        self.resource_quota_manager()
            .ensure_launch_quota(&resolved_launch.spec)?;
        let manager = self.port_lease_coordinator();
        if self.config.start_mode == KrunStartMode::PlanOnly {
            // Plan-only port admission is a pure preview. It must precede
            // segment resolution because `network_config` durably allocates
            // the tenant's primary segment even though no attachment hold is
            // acquired. A rejected preview therefore leaves no network
            // authority to compensate or accidentally release.
            let auto_bindings = manager.preview_bindings_for_sandbox(
                &resolved_launch.spec.tenant_id,
                &resolved_launch.spec.port_bindings,
                &resolved_launch.image_metadata.exposed_ports,
            )?;
            resolved_launch.spec.port_bindings.extend(auto_bindings);
        }
        let network_layout = OciNetworkLayout::with_roots(
            &self.config.workload_state_root,
            &self.config.network_state_root,
            &resolved_launch.spec.tenant_id,
            sandbox_id,
        );
        network_layout.ensure_directories()?;
        let bundle_layout = KrunBundleLayout::new(crate::artifact_paths::bundle_dir(
            &self.config.bundle_root,
            &resolved_launch.spec.tenant_id,
            sandbox_id,
        ));
        let conmon_layout = OciConmonLayout::new_for_tenant(
            &self.config.workload_state_root,
            &resolved_launch.spec.tenant_id,
            sandbox_id,
        );
        conmon_layout
            .ensure_directories()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create krun state directories under {}: {error}",
                    self.config.workload_state_root.display()
                ),
            })?;
        let conmon_launch = build_launch_plan(
            &OciConmonConfig {
                conmon_path: self.config.conmon_path.clone(),
                runtime_path: self.config.runtime_path.clone(),
                buildah_path: self.config.buildah_path.clone(),
                use_buildah_unshare: false,
                log_level: self.config.log_level.clone(),
                log_size_max_bytes: resolved_launch.spec.resources.log_limit_bytes,
            },
            &conmon_layout,
            sandbox_id,
            spec.display_name(),
            &bundle_layout.bundle_dir,
            None,
            &krun_vm_config_prelude(&resolved_launch.spec, false)?,
        );
        let handle = SandboxHandle::new(
            resolved_launch.spec.tenant_id.clone(),
            sandbox_id.clone(),
            resolved_launch.spec.display_name().to_owned(),
            SandboxBackendKind::Krun,
            SandboxStatus::Starting,
            visible_published_endpoints(
                self.config.start_mode,
                &resolved_launch.spec,
                SandboxStatus::Starting,
            ),
        );
        let launch_authority = match self.config.start_mode {
            KrunStartMode::PlanOnly => KrunLaunchAuthority::PlanOnly,
            KrunStartMode::Execute => KrunLaunchAuthority::Reserved {
                reservation_claim: new_launch_reservation_claim()?,
            },
        };
        let mut manifest = KrunSandboxManifest {
            handle,
            spec: resolved_launch.spec,
            image_metadata: resolved_launch.image_metadata,
            launch_artifact,
            provision_prepared: false,
            bundle_layout,
            conmon_layout,
            network_layout,
            provision_network_plan: provision_network_plan.cloned(),
            network_config: None,
            port_leases: Vec::new(),
            launch_authority,
            creator_handoff: KrunCreatorHandoffState::NotSpawned,
            provider_failure_cleanup: KrunProviderFailureCleanupState::Inactive,
            egress_proxy: None,
            conmon_launch,
            last_exit_code: None,
            restart_count: 0,
            next_restart_at_millis: None,
            start_mode: self.config.start_mode,
            shutdown_requested: false,
            status: SandboxStatus::Starting,
        };
        if self.config.start_mode == KrunStartMode::Execute {
            // Publish the exact claim before placement can reserve an
            // attachment, IPAM address, or port. A crash from this point has a
            // recoverable claim-only manifest and no fabricated provider
            // evidence.
            before_initial_publication(&manifest)?;
            self.create_manifest(&manifest)?;
            let mut reservations = match self.reserve_execute_launch_network(
                &mut manifest,
                &manager,
                provision_network_plan,
            ) {
                Ok(reservations) => reservations,
                Err(error) => {
                    return Err(self.persist_unstarted_launch_failure(&mut manifest, error));
                }
            };
            if let Err(error) = after_network_reservation(&manifest) {
                return Err(self.persist_unstarted_launch_failure_with_reservations(
                    &mut manifest,
                    error,
                    &reservations,
                ));
            }
            if let Err(error) = self.write_manifest(&manifest) {
                return Err(self.persist_unstarted_launch_failure_with_reservations(
                    &mut manifest,
                    error,
                    &reservations,
                ));
            }
            if let Err(error) = reservations.confirm_manifest_published() {
                return Err(self.persist_unstarted_launch_failure(&mut manifest, error));
            }
        }

        if prepare_bundle {
            let bundle_result = krun_bundle_options(
                &self.config,
                &manifest.spec,
                &manifest.image_metadata,
                sandbox_id,
                manifest.egress_proxy.as_ref(),
            )
            .and_then(|options| {
                write_bundle_config(
                    &manifest.bundle_layout,
                    &hostname_for(&manifest.spec),
                    &manifest.spec,
                    Some(manifest.network_layout.netns_path.as_path()),
                    &options,
                )
            });
            if let Err(error) = bundle_result {
                return match self.config.start_mode {
                    KrunStartMode::PlanOnly => Err(error),
                    KrunStartMode::Execute => {
                        Err(self.persist_unstarted_launch_failure(&mut manifest, error))
                    }
                };
            }
            manifest.provision_prepared = true;
            if self.config.start_mode == KrunStartMode::Execute {
                self.write_manifest(&manifest)?;
            }
        }
        Ok(KrunStartPlan { manifest })
    }

    fn reserve_execute_launch_network(
        &self,
        manifest: &mut KrunSandboxManifest,
        manager: &OciPortLeaseCoordinator,
        provision_network_plan: Option<&SandboxProvisionNetworkPlan>,
    ) -> Result<ReservedLaunchPorts> {
        let reservation_claim = manifest.require_reserved_claim()?.clone();
        let attachment_id = provision_network_plan.map_or_else(
            || default_network_attachment_id(&manifest.handle.id),
            |plan| plan.attachment_id().clone(),
        );
        let mut network_config = self.place_sandbox_config(
            &manifest.spec.tenant_id,
            &manifest.network_layout,
            &manifest.handle.id,
            &attachment_id,
            &reservation_claim,
        )?;
        network_config.network_plan =
            provision_network_plan.map(|plan| plan.network_plan().clone());
        #[cfg(test)]
        if network_config.network_plan.is_none() {
            network_config.network_plan = Some(
                crate::provision::test_support::legacy_start_attachment_network_plan_fixture(
                    &manifest.spec,
                    &manifest.handle.id,
                    "krun-coarse-start",
                ),
            );
        }
        // Keep the exact placed attachment available to the synchronous
        // compensation path before listener reservation can fail. In
        // particular, a compiler-issued attachment ID must never be replaced
        // with a workload-derived fallback during rollback.
        manifest.network_config = Some(network_config.clone());
        let internal_listener = egress_listener_reservation(&network_config)?;
        let reservations = match provision_network_plan {
            Some(plan) => manager.reserve_exact_provision_ports(
                plan,
                Some(internal_listener),
                &reservation_claim,
            )?,
            None => manager.reserve_launch_ports_for_sandbox(
                SandboxLaunchPortPlan::new(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.spec.port_bindings,
                    &manifest.image_metadata.exposed_ports,
                )
                .with_internal_listener(internal_listener),
                &reservation_claim,
            )?,
        };
        let egress_listener = reservations.internal_listener.clone().ok_or_else(|| {
            SandboxError::OperationFailed {
                message: "krun launch reservation omitted the required egress listener".to_owned(),
            }
        })?;
        let egress_proxy = egress_proxy_assignment(&network_config, egress_listener)?;
        manifest.spec.port_bindings = reservations.published_bindings.clone();
        manifest.port_leases = reservations.published_leases.clone();
        manifest.egress_proxy = Some(egress_proxy);
        Ok(reservations)
    }

    fn prepare_image_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        image_reference: &str,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciImageMaterializer::for_tenant_sandbox(
            &self.config.workload_state_root,
            &spec.tenant_id,
            sandbox_id,
        )
        .prepare_image_launch(sandbox_id, image_reference, &spec.process)
    }

    fn prepare_built_image_launch(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
    ) -> Result<PreparedMaterializedImageLaunch> {
        OciDockerfileBuilder::for_tenant_sandbox(
            &self.config.workload_state_root,
            &spec.tenant_id,
            sandbox_id,
        )
        .prepare_built_image_launch(
            sandbox_id,
            image_name,
            dockerfile_path,
            context_path,
            &spec.process,
        )
    }

    pub(super) fn prepare_oci_image_start(
        &self,
        spec: &SandboxSpec,
        sandbox_id: &SandboxId,
        source: &SandboxOciImageSource,
    ) -> Result<PreparedMaterializedImageLaunch> {
        match source {
            SandboxOciImageSource::Reference(reference) => {
                self.prepare_image_launch(spec, sandbox_id, &reference.reference)
            }
            SandboxOciImageSource::Build(build) => self.prepare_built_image_launch(
                spec,
                sandbox_id,
                &build.image_name,
                &build.dockerfile_path,
                &build.context_path,
            ),
        }
    }

    pub(super) fn cleanup_manifest_launch_artifacts(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        let Some(artifact) = manifest.launch_artifact.as_ref() else {
            return Ok(());
        };
        match artifact {
            KrunLaunchArtifact::Rootfs(rootfs) => {
                OciImageMaterializer::for_tenant_sandbox(
                    &self.config.workload_state_root,
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                )
                .remove_owned_artifact(&manifest.handle.id, rootfs)?;
            }
        }
        Ok(())
    }

    /// Assign a host-side egress PEP for an execute-mode launch: the proxy binds
    /// on the bridge gateway address so it is the only outbound path reachable
    /// from inside the sandbox's deny-by-default network namespace.
    /// Bind the sandbox's egress PEP on the gateway of its PLACED block bridge, so
    /// a sandbox on a grown block reaches its own on-link PEP (MTN6).
    #[cfg(test)]
    pub(super) fn allocate_egress_proxy(
        &self,
        network_config: &OciNetworkConfig,
        sandbox_id: &SandboxId,
        spec: &SandboxSpec,
    ) -> Result<EgressProxyAssignment> {
        crate::backends::oci::egress::allocate_egress_proxy(
            network_config,
            &self.port_lease_coordinator(),
            &spec.tenant_id,
            sandbox_id,
        )
    }

    pub(super) fn materialize_krun_vm_config(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        let vm_config_path = krun_vm_config_path(&required_rootfs(&manifest.spec)?.rootfs);
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

    pub(super) fn port_lease_coordinator(&self) -> OciPortLeaseCoordinator {
        self.port_lease_coordinator
            .clone()
            .with_range(self.config.published_port_range.clone())
            .with_max_ports_per_tenant(self.config.max_published_ports_per_tenant)
    }
}

#[cfg(test)]
fn next_sandbox_id(name: &str) -> SandboxId {
    SandboxId::new(format!(
        "{}-{}",
        slugify(name),
        Ulid::new().to_string().to_ascii_lowercase()
    ))
}

pub(super) fn hostname_for(spec: &SandboxSpec) -> String {
    let slug = slugify(spec.display_name());
    if slug.is_empty() {
        "nimbus-sandbox".to_owned()
    } else {
        slug
    }
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

    let vm_config_path = krun_vm_config_path(&required_rootfs(spec)?.rootfs);
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

pub(super) fn resolve_start_spec(
    spec: &SandboxSpec,
    launch_defaults: Option<&OciImageLaunchDefaults>,
) -> Result<KrunResolvedLaunchSpec> {
    let Some(launch_defaults) = launch_defaults else {
        let mut resolved_spec = spec.clone();
        resolved_spec.process = resolve_process_without_image_defaults(&spec.process)?;
        let process_user = resolved_spec.process.user.clone();
        return Ok(KrunResolvedLaunchSpec {
            spec: resolved_spec,
            image_metadata: KrunImageMetadata {
                user: process_user,
                ..KrunImageMetadata::default()
            },
        });
    };

    let mut resolved_spec = spec.clone();
    resolved_spec.root = resolve_root_spec(&spec.root, &launch_defaults.rootfs);
    resolved_spec.process = resolve_process_spec(&spec.process, &launch_defaults.process);

    Ok(KrunResolvedLaunchSpec {
        spec: resolved_spec,
        image_metadata: KrunImageMetadata {
            user: launch_defaults.user.clone(),
            stop_signal: launch_defaults.stop_signal.clone(),
            healthcheck: launch_defaults.healthcheck.clone(),
            labels: launch_defaults.labels.clone(),
            exposed_ports: launch_defaults.exposed_ports.clone(),
        },
    })
}

fn required_rootfs(spec: &SandboxSpec) -> Result<&SandboxRootfsSpec> {
    spec.rootfs().ok_or_else(|| SandboxError::InvalidSpec {
        message: format!(
            "krun sandbox {} must be resolved to a rootfs before writing VM configuration",
            spec.display_name()
        ),
    })
}

pub(super) fn apply_guest_user_switch(
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

fn krun_additional_mounts(
    config: &KrunSandboxBackendConfig,
    spec: &SandboxSpec,
    image_metadata: &KrunImageMetadata,
) -> Result<Vec<KrunBundleMount>> {
    let mut mounts = tenant_volume_mounts(&config.workload_state_root, spec)?;
    mounts.extend(guest_user_switch_mounts(config, image_metadata));
    Ok(mounts)
}

pub(super) fn krun_bundle_options(
    config: &KrunSandboxBackendConfig,
    spec: &SandboxSpec,
    image_metadata: &KrunImageMetadata,
    sandbox_id: &SandboxId,
    egress_proxy: Option<&EgressProxyAssignment>,
) -> Result<KrunBundleOptions> {
    let mut additional_mounts = krun_additional_mounts(config, spec, image_metadata)?;
    let mut egress_trust_anchor_guest_path = None;
    if egress_proxy.is_some() {
        let trust_anchor =
            egress_trust_anchor_mount(&config.workload_state_root, &spec.tenant_id, sandbox_id)?;
        egress_trust_anchor_guest_path = Some(trust_anchor.guest_path.clone());
        additional_mounts.push(KrunBundleMount {
            destination: trust_anchor.guest_path,
            source: trust_anchor.host_path,
            options: egress_trust_anchor_mount_options(),
        });
    }
    Ok(KrunBundleOptions {
        additional_mounts,
        egress_proxy_url: egress_proxy
            .map(EgressProxyAssignment::proxy_url)
            .transpose()?,
        egress_trust_anchor_guest_path,
    })
}

fn tenant_volume_mounts(state_root: &Path, spec: &SandboxSpec) -> Result<Vec<KrunBundleMount>> {
    crate::spec::validate_sandbox_mounts(&spec.mounts)
        .map_err(|message| SandboxError::InvalidSpec { message })?;
    let mut mounts = Vec::new();
    for mount in &spec.mounts {
        let destination = mount.destination.to_string_lossy().into_owned();
        let volume_name = mount
            .tenant_volume_name()
            .ok_or_else(|| SandboxError::InvalidSpec {
                message: "unsupported krun sandbox mount source".to_owned(),
            })?;
        let source =
            crate::artifact_paths::tenant_volume_dir(state_root, &spec.tenant_id, volume_name);
        std::fs::create_dir_all(&source).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create tenant volume {} for sandbox {} under {}: {error}",
                volume_name,
                spec.display_name(),
                source.display()
            ),
        })?;
        mounts.push(KrunBundleMount {
            destination,
            source,
            options: tenant_volume_mount_options(mount.read_only),
        });
    }
    Ok(mounts)
}

fn tenant_volume_mount_options(read_only: bool) -> Vec<String> {
    vec![
        "rbind".to_owned(),
        if read_only { "ro" } else { "rw" }.to_owned(),
        "nosuid".to_owned(),
        "nodev".to_owned(),
    ]
}

fn egress_trust_anchor_mount_options() -> Vec<String> {
    vec![
        "rbind".to_owned(),
        "ro".to_owned(),
        "nosuid".to_owned(),
        "nodev".to_owned(),
        "noexec".to_owned(),
    ]
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

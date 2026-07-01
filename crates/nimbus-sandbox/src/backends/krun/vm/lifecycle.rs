use super::readiness::{running_status, synchronize_handle_status, visible_published_endpoints};
use super::start::{ensure_guest_user_helper_available, hostname_for};
use super::*;

impl KrunSandboxBackend {
    pub(super) fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxHandle>> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };

        manifest.status = match self.config.start_mode {
            KrunStartMode::PlanOnly => manifest.status,
            KrunStartMode::Execute => {
                if self.maybe_restart_after_exit(&mut manifest)? {
                    manifest.status
                } else {
                    self.detect_runtime_status(&manifest)?
                }
            }
        };
        manifest.handle.status = manifest.status;
        manifest.handle.published_endpoints =
            visible_published_endpoints(manifest.start_mode, &manifest.spec, manifest.status);
        self.write_manifest(&manifest)?;
        Ok(Some(manifest.handle))
    }

    pub(super) fn stop_sync(&self, id: &SandboxId) -> Result<()> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };

        match self.config.start_mode {
            KrunStartMode::PlanOnly => {
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                manifest.status = SandboxStatus::Stopped;
                manifest.handle.status = SandboxStatus::Stopped;
                self.cleanup_manifest_launch_artifacts(&manifest)?;
                manifest.launch_artifact = None;
                self.write_manifest(&manifest)
            }
            KrunStartMode::Execute => self.execute_stop(&mut manifest),
        }
    }

    pub(super) fn execute_start(&self, launch_plan: &KrunStartPlan) -> Result<SandboxHandle> {
        ensure_linux_host("krun")?;
        let mut manifest = launch_plan.manifest.clone();
        self.launch_manifest(&mut manifest, true)?;
        Ok(manifest.handle)
    }

    fn execute_stop(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.shutdown_requested = true;
            manifest.next_restart_at_millis = None;
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            synchronize_handle_status(manifest, SandboxStatus::Stopped);
            self.release_network_artifacts(manifest)?;
            return self.write_manifest(manifest);
        }

        manifest.shutdown_requested = true;
        manifest.next_restart_at_millis = None;
        let pid = read_pid(&manifest.conmon_layout.pidfile)?;
        let stop_signal = configured_stop_signal(manifest.image_metadata.stop_signal.as_deref());
        signal_process(&stop_signal, pid)?;
        let stop_timeout = configured_stop_timeout(&manifest.spec, self.config.stop_timeout);
        if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
            signal_process("KILL", pid)?;
            if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "sandbox {} did not write an exit file after TERM/KILL",
                        manifest.handle.id
                    ),
                });
            }
        }

        manifest.last_exit_code = Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
        synchronize_handle_status(manifest, SandboxStatus::Stopped);
        self.release_network_artifacts(manifest)?;
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        self.write_manifest(manifest)
    }

    pub(super) fn detect_runtime_status(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<SandboxStatus> {
        detect_conmon_runtime_status(
            RuntimeStatusProbe {
                exit_status_file: &manifest.conmon_layout.exit_status_file,
                state_command: &manifest.conmon_launch.state_command,
                pidfile: &manifest.conmon_layout.pidfile,
                shutdown_requested: manifest.shutdown_requested,
                current_status: manifest.status,
            },
            || Ok(running_status(manifest)),
        )
    }

    fn maybe_restart_after_exit(&self, manifest: &mut KrunSandboxManifest) -> Result<bool> {
        if manifest.shutdown_requested || !manifest.conmon_layout.exit_status_file.exists() {
            return Ok(false);
        }

        let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
        if !restart_policy_allows_restart(
            manifest.spec.lifecycle.restart_policy,
            exit_code,
            manifest.restart_count,
        ) {
            return Ok(false);
        }

        manifest.last_exit_code = Some(exit_code);
        let now_millis = now_millis()?;
        let next_restart_at_millis = manifest.next_restart_at_millis.get_or_insert_with(|| {
            now_millis.saturating_add(restart_backoff_delay(manifest.restart_count).as_millis() as u64)
        });
        if now_millis < *next_restart_at_millis {
            synchronize_handle_status(manifest, SandboxStatus::Starting);
            return Ok(true);
        }

        manifest.restart_count += 1;
        manifest.next_restart_at_millis = None;
        self.reset_runtime_for_restart(manifest)?;
        self.launch_manifest(manifest, false)?;
        Ok(true)
    }

    fn launch_manifest(
        &self,
        manifest: &mut KrunSandboxManifest,
        clear_last_exit_code: bool,
    ) -> Result<()> {
        ensure_linux_host("krun")?;
        ensure_guest_user_helper_available(&self.config, manifest)?;
        // Stand up the deny-by-default network namespace (deny chain + inbound
        // published-port DNAT) and start the egress PEP on the bridge gateway
        // BEFORE crun launches the VMM into the namespace. Under libkrun TSI the
        // guest's outbound sockets are issued by this host VMM process, so the
        // VMM must only ever run once the namespace's sole outbound path is the
        // policy-enforcing proxy — there is no open-egress window.
        self.configure_network(manifest)?;
        if let Err(error) = self.launch_into_network(manifest, clear_last_exit_code) {
            // Fail-closed: never leave a namespace or a running VMM whose egress
            // is not enforced. Tear the VMM and the netns back down on any error.
            // The original launch `error` stays the returned error (priority
            // unchanged); teardown failures are security-relevant (a leaked netns
            // or running VMM whose egress would no longer be gated), so they are
            // surfaced at warn rather than discarded silently.
            if let Err(teardown_error) =
                run_status_best_effort(&manifest.conmon_launch.delete_command)
            {
                tracing::warn!(
                    sandbox_id = %manifest.handle.id,
                    error = %teardown_error,
                    "failed to delete krun VMM during fail-closed launch teardown"
                );
            }
            if let Err(teardown_error) = self.release_network_artifacts(manifest) {
                tracing::warn!(
                    sandbox_id = %manifest.handle.id,
                    error = %teardown_error,
                    "failed to release krun network artifacts during fail-closed launch teardown"
                );
            }
            return Err(error);
        }
        Ok(())
    }

    fn launch_into_network(
        &self,
        manifest: &mut KrunSandboxManifest,
        clear_last_exit_code: bool,
    ) -> Result<()> {
        self.ensure_egress_proxy_running(manifest)?;
        // Fail-closed readiness gate: the last checkpoint before crun spawns the
        // VMM into the namespace. Permit only when the platform supports
        // enforcement (Linux), the deny-by-default netns is installed, and the
        // per-sandbox egress PEP is running with an active policy generation. Any
        // not-ready precondition returns Err here, which `launch_manifest`
        // converts into a full netns/VMM teardown — no path reaches
        // `spawn_background` with an unenforced egress.
        self.ensure_execute_egress_enforced(manifest)?;
        spawn_background(&manifest.conmon_launch.create_command)?;
        let runtime_state = wait_for_runtime_state(
            &manifest.conmon_launch.state_command,
            self.config.start_timeout,
        )?;
        if runtime_state != "running" {
            run_status_checked(&manifest.conmon_launch.start_command)?;
        }

        manifest.shutdown_requested = false;
        manifest.next_restart_at_millis = None;
        if clear_last_exit_code {
            manifest.last_exit_code = None;
        }
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        self.write_manifest(manifest)
    }

    /// Stand up the sandbox's deny-by-default network namespace: create the
    /// persistent netns, run the shared netavark setup (no-default-route deny
    /// chain + inbound published-port DNAT), then pin egress to this sandbox's
    /// own PEP. Fail-closed: on any failure the half-built namespace is torn
    /// down so the VMM is never launched into an unconfigured or unpinned netns.
    /// Reuses the container backend's shared netns free-functions; no
    /// netns/netavark/IPAM logic is forked here.
    fn configure_network(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        // One-shot: drop the legacy shared nimbus0 bridge before the first
        // per-tenant setup (pre-launch migration, breaking).
        purge_legacy_nimbus0_once(&self.config.state_root.join("networks"))?;
        // Reuse the config resolved + persisted at manifest-prepare; never re-assign
        // it (audit M1 / MTN4) so setup and teardown agree on the bridge.
        let network_config = manifest.network_config.clone();
        create_persistent_network_namespace(&manifest.network_layout.netns_path)?;
        if let Err(error) = setup_container_network(
            &manifest.network_layout,
            &network_config,
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            None,
        ) {
            let _ = remove_persistent_network_namespace(&manifest.network_layout.netns_path);
            return Err(error);
        }
        // Pin the netns so the ONLY reachable egress is this sandbox's own PEP.
        // The netavark deny is route-based, but the shared bridge gateway is
        // on-link and every sibling sandbox's PEP listens on it at a distinct
        // port; without this pin an execute-mode guest could egress through a
        // sibling tenant's proxy and its injected credentials (audit H1). Under
        // libkrun TSI the guest's outbound sockets are issued by this host VMM
        // process inside the netns, so the output-hook pin governs the guest
        // exactly as it governs a container. Tear the namespace back down on
        // failure so the VMM never launches into an unpinned netns.
        if let Some(proxy) = manifest.egress_proxy.as_ref()
            && let Err(error) = pin_netns_egress_to_own_proxy(&manifest.network_layout, proxy)
        {
            let _ = teardown_container_network(
                &manifest.network_layout,
                &network_config,
                &manifest.handle.id,
                manifest.spec.display_name(),
                &hostname_for(&manifest.spec),
                &manifest.spec.port_bindings,
                None,
            );
            let _ = remove_persistent_network_namespace(&manifest.network_layout.netns_path);
            return Err(error);
        }
        // Take the tenant's refcount hold now the netns is up and pinned; the
        // reaper frees the index + bridge when the last hold releases.
        self.segment_allocator()?
            .acquire(&manifest.spec.tenant_id, &manifest.handle.id)?;
        Ok(())
    }

    /// Fail-closed readiness gate evaluated immediately before the VMM launches.
    ///
    /// Permits the launch only when ALL hold: (1) the binary is built for a
    /// Linux target (`ensure_linux_host`, a compile-time `cfg!(target_os =
    /// "linux")` check — it does NOT probe `/dev/kvm`; actual KVM availability
    /// is enforced downstream by crun/libkrun, which fail closed at VMM spawn
    /// when `/dev/kvm` is absent, so a Linux host without KVM never reaches an
    /// enforced-egress-bypassing state), (2) the sandbox's deny-by-default
    /// network namespace is installed, and (3) the per-sandbox egress PEP is
    /// running AND ready (it reports an active policy generation). Any missing
    /// precondition, any lookup error, or a not-ready PEP returns `Err`, which
    /// the caller treats as deny: the VMM is never spawned and the half-built
    /// namespace is torn down. The gate never degrades to allow.
    pub(super) fn ensure_execute_egress_enforced(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        // (1) Platform: the krun execute path is a Linux build target. This is a
        // compile-time cfg check, not a /dev/kvm probe; a Linux host without KVM
        // still fails closed because crun/libkrun cannot spawn the VMM without
        // /dev/kvm. Deny on every non-Linux build.
        ensure_linux_host("krun")?;
        self.ensure_execute_egress_preconditions(
            &manifest.handle.id,
            &manifest.network_layout.netns_path,
        )
    }

    /// Platform-independent half of the readiness gate: deny unless the
    /// deny-by-default netns is installed AND the egress PEP for `id` is running
    /// with an active policy generation. Split out from
    /// [`KrunSandboxBackend::ensure_execute_egress_enforced`] so the deny/permit
    /// matrix is unit-testable without a Linux host or `/dev/kvm`.
    pub(super) fn ensure_execute_egress_preconditions(
        &self,
        id: &SandboxId,
        netns_path: &Path,
    ) -> Result<()> {
        // (1) The deny-by-default network namespace must already be installed.
        //
        // `netns_path.exists()` is a sufficient proxy for "the deny chain is
        // installed" because `configure_network` is atomic with respect to this
        // file: it first creates the persistent netns, then runs the netavark
        // setup (no-default-route deny chain + inbound published-port DNAT), and
        // on ANY netavark failure it removes the netns again
        // (`remove_persistent_network_namespace`). So the netns path only
        // persists once the full deny-chain setup has succeeded; a half-built,
        // open-egress netns never survives to be observed here. Do NOT weaken
        // `configure_network` to "create the netns, then configure it" without
        // also gating on a netavark status artifact here — otherwise an
        // unconfigured (open-egress) netns could satisfy this check.
        if !netns_path.exists() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun sandbox {id} denied launch: deny-by-default network namespace {} is not installed",
                    netns_path.display()
                ),
            });
        }
        // (2) The per-sandbox egress PEP must be running AND ready.
        match self.egress_proxies.readiness(id)? {
            None => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun sandbox {id} denied launch: no egress policy-enforcement proxy is running for the deny-by-default namespace"
                ),
            }),
            Some(readiness) if !readiness.ready || readiness.policy_generation.is_none() => {
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun sandbox {id} denied launch: egress policy-enforcement proxy is not ready (no active policy generation loaded)"
                    ),
                })
            }
            Some(_) => Ok(()),
        }
    }

    /// Start the host-side egress PEP for this sandbox on the bridge gateway
    /// bind address. Fail-closed: a missing assignment or a proxy start error
    /// returns `Err`, which the launch path treats as deny.
    fn ensure_egress_proxy_running(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        crate::backends::oci::egress::ensure_egress_proxy_running(
            &self.egress_proxies,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
            &manifest.spec.egress,
        )
    }

    /// Stop the egress PEP, tear the sandbox network down, and remove the netns,
    /// reusing the container backend's shared teardown free-functions plus the
    /// shared `EgressProxyRegistry::stop`. Errors are collected so a single
    /// failing step never short-circuits the rest of the cleanup.
    fn release_network_artifacts(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        let mut errors = Vec::new();
        if let Err(error) = self.egress_proxies.stop(&manifest.handle.id) {
            errors.push(error.to_string());
        }
        if let Err(error) = teardown_container_network(
            &manifest.network_layout,
            &manifest.network_config,
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            None,
        ) {
            errors.push(error.to_string());
        }
        if let Err(error) = remove_persistent_network_namespace(&manifest.network_layout.netns_path)
        {
            errors.push(error.to_string());
        }
        // Drop this sandbox's hold; on the LAST hold the tenant is drained, so
        // reap EVERY block bridge it grew (netavark won't auto-GC) and free all
        // its indices for reuse.
        match self.segment_allocator() {
            Ok(allocator) => {
                match allocator.release(&manifest.spec.tenant_id, &manifest.handle.id) {
                    Ok(ReleaseOutcome::TenantDrained { segments }) => {
                        for segment in &segments {
                            if let Err(error) = reap_bridge_interface(segment.network_interface()) {
                                errors.push(error.to_string());
                            }
                        }
                    }
                    Ok(ReleaseOutcome::StillLive) => {}
                    Err(error) => errors.push(error.to_string()),
                }
            }
            Err(error) => errors.push(error.to_string()),
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to release krun sandbox {} network artifacts: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }

    fn reset_runtime_for_restart(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        self.release_network_artifacts(manifest)?;
        run_status_checked(&manifest.conmon_launch.delete_command)?;
        remove_if_exists(&manifest.conmon_layout.exit_status_file)?;
        remove_if_exists(&manifest.conmon_layout.pidfile)?;
        remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
        Ok(())
    }

    pub(super) fn read_manifest(&self, id: &SandboxId) -> Result<Option<KrunSandboxManifest>> {
        let Some(manifest_path) =
            crate::artifact_paths::manifest_path_for_sandbox_id(&self.config.state_root, id)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to find krun sandbox manifest for {} under {}: {error}",
                        id,
                        self.config.state_root.display()
                    ),
                })?
        else {
            return Ok(None);
        };
        if !manifest_path.exists() {
            return Ok(None);
        }

        let contents =
            std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        let manifest =
            serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        Ok(Some(manifest))
    }

    pub(super) fn write_manifest(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        std::fs::create_dir_all(&manifest.conmon_layout.container_state_dir).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to create manifest directory {}: {error}",
                    manifest.conmon_layout.container_state_dir.display()
                ),
            }
        })?;
        let rendered =
            serde_json::to_vec_pretty(manifest).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize sandbox manifest: {error}"),
            })?;
        std::fs::write(&manifest.conmon_layout.manifest_path, rendered).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to write sandbox manifest {}: {error}",
                    manifest.conmon_layout.manifest_path.display()
                ),
            }
        })
    }
}

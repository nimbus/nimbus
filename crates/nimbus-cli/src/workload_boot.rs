use std::path::{Path, PathBuf};

use nimbus::{Error, TenantId};
use nimbus_compute::embedded_local_node_identity;
use nimbus_tenant::TenantIsolationMode;
use nimbus_workloads::{
    DesiredWorkload, DesiredWorkloadState, NodeCapacity, PlacementPlan, WorkloadScheduler,
};

use crate::compose::discovery::ResolvedComposeSelection;

#[derive(Debug, Clone)]
pub(crate) struct WorkloadControlBootPlan {
    tenant_id: TenantId,
    tenant_isolation_mode: TenantIsolationMode,
    compose_files: Vec<PathBuf>,
    desired_workloads: Vec<DesiredWorkload>,
    placement_plans: Vec<PlacementPlan>,
}

impl WorkloadControlBootPlan {
    pub(crate) fn desired_workload_count(&self) -> usize {
        self.desired_workloads.len()
    }

    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn tenant_isolation_mode(&self) -> TenantIsolationMode {
        self.tenant_isolation_mode
    }

    pub(crate) fn compose_file_count(&self) -> usize {
        self.compose_files.len()
    }

    pub(crate) fn placement_plan_count(&self) -> usize {
        self.placement_plans.len()
    }

    #[cfg(test)]
    pub(crate) fn compose_files(&self) -> &[PathBuf] {
        &self.compose_files
    }

    #[cfg(test)]
    pub(crate) fn desired_workloads(&self) -> &[DesiredWorkload] {
        &self.desired_workloads
    }

    #[cfg(test)]
    pub(crate) fn placement_plans(&self) -> &[PlacementPlan] {
        &self.placement_plans
    }
}

pub(crate) fn default_local_node_capacity() -> Result<Vec<NodeCapacity>, Error> {
    Ok(vec![NodeCapacity::new(
        embedded_local_node_identity().as_str(),
        u32::MAX,
    )?])
}

pub(crate) fn plan_compose_services(
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    tenant_isolation_mode: TenantIsolationMode,
    nodes: &[NodeCapacity],
) -> Result<WorkloadControlBootPlan, Error> {
    let context = crate::compose::load_compose_project_context_for_selection_with_isolation_mode(
        selection,
        control_data_dir,
        tenant_isolation_mode,
    )?;
    let scheduler = WorkloadScheduler::new();
    let mut desired_workloads = Vec::with_capacity(context.plan.services.len());
    let mut placement_plans = Vec::new();

    for service_name in context.plan.services.keys() {
        let desired = DesiredWorkload::service(
            context.control_plane.local_tenant_id.clone(),
            service_name,
            DesiredWorkloadState::Running,
            1,
        )?;
        placement_plans.push(scheduler.schedule(&desired, nodes));
        desired_workloads.push(desired);
    }

    Ok(WorkloadControlBootPlan {
        tenant_id: context.control_plane.local_tenant_id,
        tenant_isolation_mode,
        compose_files: selection.files.clone(),
        desired_workloads,
        placement_plans,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::compose::discovery::{ResolvedComposeSelection, resolve_compose_selection};
    use crate::test_support::with_current_dir;
    use crate::{Cli, Command, StartCommand};
    use clap::Parser;
    use nimbus_workloads::DesiredWorkloadKind;

    fn write_compose_fixture(root: &Path, body: &str) -> PathBuf {
        let path = root.join("compose.yaml");
        fs::write(&path, body).expect("compose fixture should write");
        path
    }

    fn compose_body() -> &'static str {
        r#"
name: Demo Stack
services:
  api:
    image: ghcr.io/acme/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  db:
    image: docker.io/library/postgres@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
"#
    }

    fn parse_start<I, T>(args: I) -> StartCommand
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = Cli::parse_from(args);
        let Command::Start(command) = cli.command else {
            panic!("start subcommand should parse");
        };
        *command
    }

    fn workload_ids(plan: &WorkloadControlBootPlan) -> Vec<String> {
        plan.desired_workloads()
            .iter()
            .map(|workload| workload.workload_id().to_owned())
            .collect()
    }

    #[test]
    fn dev_start_resource_shapes_match_workload_control() {
        let temp = tempdir().expect("tempdir should build");
        let compose_path = write_compose_fixture(temp.path(), compose_body());
        let selection = ResolvedComposeSelection::explicit(compose_path.clone());
        let nodes = vec![
            NodeCapacity::new("node-b", 1).expect("node-b should build"),
            NodeCapacity::new("node-a", 2).expect("node-a should build"),
        ];

        let plan = plan_compose_services(
            &selection,
            &temp.path().join("control"),
            TenantIsolationMode::LocalDevelopment,
            &nodes,
        )
        .expect("workload-control boot plan should resolve");

        assert_eq!(
            plan.tenant_isolation_mode(),
            TenantIsolationMode::LocalDevelopment
        );
        assert_eq!(plan.compose_files(), &[compose_path]);
        assert_eq!(workload_ids(&plan), ["service:api", "service:db"]);
        assert!(plan.desired_workloads().iter().all(|workload| {
            workload.kind() == DesiredWorkloadKind::Service
                && workload.desired_state() == DesiredWorkloadState::Running
                && workload.generation() == 1
        }));
        assert_eq!(plan.placement_plans().len(), 2);
        assert!(plan.placement_plans().iter().all(|placement| {
            placement.node_id() == Some("node-a")
                && placement
                    .explanation()
                    .reason()
                    .contains("deterministic id order")
        }));
    }

    #[test]
    fn start_builds_ordered_desired_intents_for_compose_services() {
        let temp = tempdir().expect("tempdir should build");
        let compose_path = write_compose_fixture(temp.path(), compose_body());
        let command = parse_start([
            "nimbus",
            "start",
            "--compose-file",
            compose_path.to_str().expect("path should be utf-8"),
        ]);
        let selection = with_current_dir(temp.path(), || {
            crate::compose::discovery::resolve_explicit_compose_selection(
                command.compose_file.as_slice(),
                temp.path(),
            )
        })
        .expect("compose selection should resolve")
        .expect("start explicit compose selection should exist");

        let plan = plan_compose_services(
            &selection,
            &temp.path().join("control"),
            command.tenant_isolation_mode,
            &default_local_node_capacity().expect("local node should build"),
        )
        .expect("start workload-control plan should resolve");

        assert_eq!(
            plan.tenant_isolation_mode(),
            TenantIsolationMode::Production
        );
        assert_eq!(workload_ids(&plan), ["service:api", "service:db"]);
        assert!(
            plan.desired_workloads()
                .iter()
                .all(|workload| workload.tenant_id() == plan.tenant_id())
        );
    }

    #[test]
    fn compose_overrides_deduplicate_desired_intents_in_stable_order() {
        let temp = tempdir().expect("tempdir should build");
        let base_path = write_compose_fixture(
            temp.path(),
            r#"
name: Demo Stack
services:
  zeta:
    image: ghcr.io/acme/zeta@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  api:
    image: ghcr.io/acme/api@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
"#,
        );
        let override_path = temp.path().join("compose.override.yaml");
        fs::write(
            &override_path,
            r#"
services:
  api:
    image: ghcr.io/acme/api@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
  worker:
    image: ghcr.io/acme/worker@sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd
"#,
        )
        .expect("override fixture should write");
        let selection =
            resolve_compose_selection(&[base_path.clone(), override_path.clone()], temp.path())
                .expect("compose selection should resolve")
                .expect("explicit compose selection should exist");

        let plan = plan_compose_services(
            &selection,
            &temp.path().join("control"),
            TenantIsolationMode::Production,
            &default_local_node_capacity().expect("local node should build"),
        )
        .expect("merged workload-control plan should resolve");

        assert_eq!(plan.compose_files(), &[base_path, override_path]);
        assert_eq!(
            workload_ids(&plan),
            ["service:api", "service:worker", "service:zeta"]
        );
        assert_eq!(plan.placement_plan_count(), plan.desired_workload_count());
        assert!(
            plan.placement_plans()
                .iter()
                .zip(plan.desired_workloads())
                .all(|(placement, desired)| placement.workload_id() == desired.workload_id())
        );
    }

    #[test]
    fn dev_start_scheduler_explanations_match() {
        let temp = tempdir().expect("tempdir should build");
        let compose_path = write_compose_fixture(temp.path(), compose_body());
        let selection = resolve_compose_selection(&[compose_path], temp.path())
            .expect("compose selection should resolve")
            .expect("compose selection should exist");
        let nodes = vec![
            NodeCapacity::new("node-b", 2).expect("node-b should build"),
            NodeCapacity::new("node-a", 1)
                .expect("node-a should build")
                .with_binding_key("service:api")
                .expect("binding key should build"),
        ];

        let dev_plan = plan_compose_services(
            &selection,
            &temp.path().join("dev-control"),
            TenantIsolationMode::LocalDevelopment,
            &nodes,
        )
        .expect("dev workload-control plan should resolve");
        let start_plan = plan_compose_services(
            &selection,
            &temp.path().join("start-control"),
            TenantIsolationMode::Production,
            &nodes,
        )
        .expect("start workload-control plan should resolve");

        let dev_explanations = dev_plan
            .placement_plans()
            .iter()
            .map(|placement| {
                (
                    placement.workload_id().to_owned(),
                    placement.explanation().clone(),
                )
            })
            .collect::<Vec<_>>();
        let start_explanations = start_plan
            .placement_plans()
            .iter()
            .map(|placement| {
                (
                    placement.workload_id().to_owned(),
                    placement.explanation().clone(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(dev_explanations, start_explanations);
        assert!(dev_explanations.iter().any(|(workload_id, explanation)| {
            workload_id == "service:api"
                && explanation.selected_node() == Some("node-b")
                && explanation
                    .rejected_nodes()
                    .get("node-a")
                    .is_some_and(|reason| reason.contains("already reserved"))
        }));
    }
}

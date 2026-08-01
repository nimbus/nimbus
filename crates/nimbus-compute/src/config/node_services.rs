use std::sync::Arc;

use nimbus_engine::Engine;
use nimbus_services::{
    EmptyServiceInstanceCatalog, RuntimeServiceRegistry, ServiceInstanceBindingRegistry,
    ServiceInstanceCatalog, ServiceManager,
};
use nimbus_tenant::TenantIsolationMode;

use crate::machine_lifecycle::MachineLifecycleManager;
use crate::node_workloads::NodeWorkloadCoordinator;

#[derive(Clone)]
enum RuntimeServiceSource {
    ServiceInstanceCatalog(Arc<dyn ServiceInstanceCatalog>),
    ServiceManager(Arc<ServiceManager>),
    Resolved {
        runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
        service_manager: Option<Arc<ServiceManager>>,
    },
}

impl RuntimeServiceSource {
    fn default_catalog() -> Self {
        Self::ServiceInstanceCatalog(Arc::new(EmptyServiceInstanceCatalog))
    }

    fn service_manager(&self) -> Option<Arc<ServiceManager>> {
        match self {
            Self::ServiceManager(service_manager) => Some(service_manager.clone()),
            Self::Resolved {
                service_manager, ..
            } => service_manager.clone(),
            Self::ServiceInstanceCatalog(_) => None,
        }
    }

    fn resolve(self, system_state_engine: Arc<Engine>) -> Self {
        match self {
            Self::ServiceInstanceCatalog(service_instances) => Self::Resolved {
                runtime_service_registry: Arc::new(ServiceInstanceBindingRegistry::new(
                    service_instances,
                )),
                service_manager: None,
            },
            Self::ServiceManager(service_manager) => {
                crate::service_manager::attach_system_state_engine(
                    &service_manager,
                    system_state_engine,
                );
                let runtime_service_registry: Arc<dyn RuntimeServiceRegistry> =
                    service_manager.clone();
                Self::Resolved {
                    runtime_service_registry,
                    service_manager: Some(service_manager),
                }
            }
            Self::Resolved { .. } => self,
        }
    }

    fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        match self {
            Self::Resolved {
                runtime_service_registry,
                ..
            } => runtime_service_registry.clone(),
            Self::ServiceManager(service_manager) => service_manager.clone(),
            Self::ServiceInstanceCatalog(service_instances) => Arc::new(
                ServiceInstanceBindingRegistry::new(service_instances.clone()),
            ),
        }
    }
}

#[derive(Clone)]
pub struct NodeServicesConfig {
    runtime_service_source: RuntimeServiceSource,
    machine_lifecycle_manager: Option<Arc<dyn MachineLifecycleManager>>,
    node_workload_coordinator: Option<Arc<NodeWorkloadCoordinator>>,
    tenant_isolation_mode: TenantIsolationMode,
}

impl Default for NodeServicesConfig {
    fn default() -> Self {
        Self {
            runtime_service_source: RuntimeServiceSource::default_catalog(),
            machine_lifecycle_manager: None,
            node_workload_coordinator: None,
            tenant_isolation_mode: TenantIsolationMode::default(),
        }
    }
}

impl NodeServicesConfig {
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn from_runtime_service_registry(
        runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    ) -> Self {
        Self {
            runtime_service_source: RuntimeServiceSource::Resolved {
                runtime_service_registry,
                service_manager: None,
            },
            ..Self::default()
        }
    }

    pub fn resolve(self, system_state_engine: Arc<Engine>) -> Self {
        Self {
            runtime_service_source: self.runtime_service_source.resolve(system_state_engine),
            machine_lifecycle_manager: self.machine_lifecycle_manager,
            node_workload_coordinator: self.node_workload_coordinator,
            tenant_isolation_mode: self.tenant_isolation_mode,
        }
    }

    pub fn with_service_instance_catalog(
        mut self,
        service_instances: Arc<dyn ServiceInstanceCatalog>,
    ) -> Self {
        self.runtime_service_source =
            RuntimeServiceSource::ServiceInstanceCatalog(service_instances);
        self
    }

    pub fn with_service_manager(mut self, service_manager: Arc<ServiceManager>) -> Self {
        self.runtime_service_source = RuntimeServiceSource::ServiceManager(service_manager);
        self
    }

    pub fn with_machine_lifecycle_manager(
        mut self,
        machine_lifecycle_manager: Arc<dyn MachineLifecycleManager>,
    ) -> Self {
        self.machine_lifecycle_manager = Some(machine_lifecycle_manager);
        self
    }

    pub fn with_node_workload_coordinator(
        mut self,
        coordinator: Arc<NodeWorkloadCoordinator>,
    ) -> Self {
        self.node_workload_coordinator = Some(coordinator);
        self
    }

    pub fn with_tenant_isolation_mode(mut self, mode: TenantIsolationMode) -> Self {
        self.tenant_isolation_mode = mode;
        self
    }

    pub fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        self.runtime_service_source.runtime_service_registry()
    }

    pub fn service_manager(&self) -> Option<Arc<ServiceManager>> {
        self.runtime_service_source.service_manager()
    }

    pub fn machine_lifecycle_manager(&self) -> Option<Arc<dyn MachineLifecycleManager>> {
        self.machine_lifecycle_manager.clone()
    }

    pub fn node_workload_coordinator(&self) -> Option<Arc<NodeWorkloadCoordinator>> {
        self.node_workload_coordinator.clone()
    }

    pub fn tenant_isolation_mode(&self) -> TenantIsolationMode {
        self.tenant_isolation_mode
    }
}

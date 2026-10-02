use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WorkloadLocation {
    node_id: Option<String>,
    machine_id: Option<String>,
}

impl WorkloadLocation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    pub fn with_machine_id(mut self, machine_id: impl Into<String>) -> Self {
        self.machine_id = Some(machine_id.into());
        self
    }

    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }

    pub fn machine_id(&self) -> Option<&str> {
        self.machine_id.as_deref()
    }
}

use serde::Serialize;
use std::fmt;
use std::num::{NonZeroU32, NonZeroUsize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMemoryPressureLevel {
    Nominal,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMemoryPressureSourceStatus {
    Observed,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeMemoryPressureSample {
    pub current_usage_bytes: Option<u64>,
    pub high_watermark_bytes: Option<u64>,
    pub critical_watermark_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeMemoryPressureDecision {
    pub level: RuntimeMemoryPressureLevel,
    pub source_status: RuntimeMemoryPressureSourceStatus,
    pub pause_prewarming: bool,
    pub run_idle_low_memory_maintenance: bool,
    pub evict_idle_retained_runtimes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimePrewarmScheduleDecision {
    pub requested_entries: usize,
    pub admitted_entries: usize,
    pub paused_by_memory_pressure: bool,
    pub memory_pressure_level: RuntimeMemoryPressureLevel,
    pub memory_pressure_source_status: RuntimeMemoryPressureSourceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeHostResourceBudget {
    pub host_millicpus: u32,
    pub system_reserved_millicpus: u32,
    pub nimbus_control_plane_reserved_millicpus: u32,
    pub runtime_hard_ceiling_millicpus: Option<u32>,
    pub runtime_seat_millicpus: NonZeroU32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHostPressureLevel {
    Nominal,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHostPressureSourceStatus {
    Observed,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeHostPressureSample {
    pub cpu_level: RuntimeHostPressureLevel,
    pub cpu_source_status: RuntimeHostPressureSourceStatus,
    pub memory_decision: RuntimeMemoryPressureDecision,
    pub control_plane_lag_high: bool,
}

pub trait RuntimeHostPressureSource: fmt::Debug + Send + Sync {
    fn sample(&self) -> RuntimeHostPressureSample;
}

#[derive(Debug, Default)]
pub struct NominalRuntimeHostPressureSource;

impl RuntimeHostPressureSource for NominalRuntimeHostPressureSource {
    fn sample(&self) -> RuntimeHostPressureSample {
        RuntimeHostPressureSample::nominal()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHostWorkClass {
    Guaranteed,
    Burstable,
    BestEffort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHostAdmissionAction {
    Admit,
    Queue,
    Shed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeHostAdmissionDecision {
    pub work_class: RuntimeHostWorkClass,
    pub action: RuntimeHostAdmissionAction,
    pub over_capacity_action: RuntimeHostAdmissionAction,
    pub tenant_quota_remaining: bool,
    pub host_pressure_level: RuntimeHostPressureLevel,
    pub current_host_in_flight: usize,
    pub effective_dispatch_seats: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeHostResourceDecision {
    pub host_pressure_level: RuntimeHostPressureLevel,
    pub cpu_pressure_level: RuntimeHostPressureLevel,
    pub cpu_source_status: RuntimeHostPressureSourceStatus,
    pub memory_pressure_level: RuntimeMemoryPressureLevel,
    pub memory_source_status: RuntimeMemoryPressureSourceStatus,
    pub control_plane_lag_high: bool,
    pub runtime_allocatable_millicpus: u32,
    pub nominal_dispatch_seats: usize,
    pub effective_dispatch_seats: usize,
    pub pause_prewarming: bool,
    pub run_idle_low_memory_maintenance: bool,
    pub evict_idle_retained_runtimes: bool,
}

impl RuntimeMemoryPressureSample {
    pub fn observed(
        current_usage_bytes: u64,
        high_watermark_bytes: u64,
        critical_watermark_bytes: u64,
    ) -> Self {
        assert!(
            high_watermark_bytes <= critical_watermark_bytes,
            "high memory watermark must be <= critical memory watermark"
        );
        Self {
            current_usage_bytes: Some(current_usage_bytes),
            high_watermark_bytes: Some(high_watermark_bytes),
            critical_watermark_bytes: Some(critical_watermark_bytes),
        }
    }

    pub fn unavailable() -> Self {
        Self {
            current_usage_bytes: None,
            high_watermark_bytes: None,
            critical_watermark_bytes: None,
        }
    }

    pub fn classify(self) -> RuntimeMemoryPressureDecision {
        let (Some(current_usage_bytes), Some(high_watermark_bytes), Some(critical_watermark_bytes)) = (
            self.current_usage_bytes,
            self.high_watermark_bytes,
            self.critical_watermark_bytes,
        ) else {
            return RuntimeMemoryPressureDecision::conservative_degraded();
        };

        let level = if current_usage_bytes >= critical_watermark_bytes {
            RuntimeMemoryPressureLevel::Critical
        } else if current_usage_bytes >= high_watermark_bytes {
            RuntimeMemoryPressureLevel::High
        } else {
            RuntimeMemoryPressureLevel::Nominal
        };
        RuntimeMemoryPressureDecision::for_level(level, RuntimeMemoryPressureSourceStatus::Observed)
    }
}

impl RuntimeMemoryPressureDecision {
    pub fn for_level(
        level: RuntimeMemoryPressureLevel,
        source_status: RuntimeMemoryPressureSourceStatus,
    ) -> Self {
        let pressure_active = !matches!(level, RuntimeMemoryPressureLevel::Nominal);
        Self {
            level,
            source_status,
            pause_prewarming: pressure_active,
            run_idle_low_memory_maintenance: pressure_active,
            evict_idle_retained_runtimes: pressure_active,
        }
    }

    fn conservative_degraded() -> Self {
        Self::for_level(
            RuntimeMemoryPressureLevel::High,
            RuntimeMemoryPressureSourceStatus::Unavailable,
        )
    }

    pub fn schedule_prewarm_entries(
        self,
        requested_entries: usize,
    ) -> RuntimePrewarmScheduleDecision {
        let admitted_entries = if self.pause_prewarming {
            0
        } else {
            requested_entries
        };
        RuntimePrewarmScheduleDecision {
            requested_entries,
            admitted_entries,
            paused_by_memory_pressure: self.pause_prewarming,
            memory_pressure_level: self.level,
            memory_pressure_source_status: self.source_status,
        }
    }

    pub fn retained_runtime_eviction_target(self, retained_entries: usize) -> usize {
        if !self.evict_idle_retained_runtimes {
            return 0;
        }
        match self.level {
            RuntimeMemoryPressureLevel::Nominal => 0,
            RuntimeMemoryPressureLevel::High => retained_entries.div_ceil(2),
            RuntimeMemoryPressureLevel::Critical => retained_entries,
        }
    }
}

impl RuntimeHostResourceBudget {
    pub fn conservative_for_logical_cpus(host_logical_cpus: NonZeroUsize) -> Self {
        let host_millicpus = usize_to_u32_saturating(host_logical_cpus.get()).saturating_mul(1000);
        let system_reserved_millicpus = if host_logical_cpus.get() <= 2 {
            500
        } else {
            1000
        };
        let nimbus_control_plane_reserved_millicpus = if host_logical_cpus.get() <= 2 {
            500
        } else {
            1000
        };
        Self {
            host_millicpus,
            system_reserved_millicpus,
            nimbus_control_plane_reserved_millicpus,
            runtime_hard_ceiling_millicpus: None,
            runtime_seat_millicpus: NonZeroU32::new(1000).expect("one CPU seat is nonzero"),
        }
    }

    pub fn runtime_allocatable_millicpus(self) -> u32 {
        let allocatable = self
            .host_millicpus
            .saturating_sub(self.system_reserved_millicpus)
            .saturating_sub(self.nimbus_control_plane_reserved_millicpus);
        match self.runtime_hard_ceiling_millicpus {
            Some(ceiling) => allocatable.min(ceiling),
            None => allocatable,
        }
    }

    pub fn nominal_dispatch_seats(self, configured_max_runtime_instances: usize) -> usize {
        let seats_by_cpu = self.runtime_allocatable_millicpus() / self.runtime_seat_millicpus.get();
        u32_to_usize_saturating(seats_by_cpu).min(configured_max_runtime_instances)
    }

    pub fn decide(
        self,
        configured_max_runtime_instances: usize,
        pressure: RuntimeHostPressureSample,
    ) -> RuntimeHostResourceDecision {
        let nominal_dispatch_seats = self.nominal_dispatch_seats(configured_max_runtime_instances);
        let host_pressure_level = pressure.overall_level();
        let effective_dispatch_seats = match host_pressure_level {
            RuntimeHostPressureLevel::Nominal => nominal_dispatch_seats,
            RuntimeHostPressureLevel::High => nominal_dispatch_seats.saturating_add(1) / 2,
            RuntimeHostPressureLevel::Critical => 0,
        };
        let host_pressure_active =
            !matches!(host_pressure_level, RuntimeHostPressureLevel::Nominal);
        RuntimeHostResourceDecision {
            host_pressure_level,
            cpu_pressure_level: pressure.cpu_level,
            cpu_source_status: pressure.cpu_source_status,
            memory_pressure_level: pressure.memory_decision.level,
            memory_source_status: pressure.memory_decision.source_status,
            control_plane_lag_high: pressure.control_plane_lag_high,
            runtime_allocatable_millicpus: self.runtime_allocatable_millicpus(),
            nominal_dispatch_seats,
            effective_dispatch_seats,
            pause_prewarming: host_pressure_active || pressure.memory_decision.pause_prewarming,
            run_idle_low_memory_maintenance: pressure
                .memory_decision
                .run_idle_low_memory_maintenance,
            evict_idle_retained_runtimes: pressure.memory_decision.evict_idle_retained_runtimes,
        }
    }
}

impl RuntimeHostPressureSample {
    pub fn observed(
        cpu_level: RuntimeHostPressureLevel,
        memory_decision: RuntimeMemoryPressureDecision,
        control_plane_lag_high: bool,
    ) -> Self {
        Self {
            cpu_level,
            cpu_source_status: RuntimeHostPressureSourceStatus::Observed,
            memory_decision,
            control_plane_lag_high,
        }
    }

    pub fn unavailable(memory_decision: RuntimeMemoryPressureDecision) -> Self {
        Self {
            cpu_level: RuntimeHostPressureLevel::High,
            cpu_source_status: RuntimeHostPressureSourceStatus::Unavailable,
            memory_decision,
            control_plane_lag_high: false,
        }
    }

    pub fn nominal() -> Self {
        Self::observed(
            RuntimeHostPressureLevel::Nominal,
            RuntimeMemoryPressureDecision::for_level(
                RuntimeMemoryPressureLevel::Nominal,
                RuntimeMemoryPressureSourceStatus::Observed,
            ),
            false,
        )
    }

    fn overall_level(self) -> RuntimeHostPressureLevel {
        if matches!(self.cpu_level, RuntimeHostPressureLevel::Critical)
            || matches!(
                self.memory_decision.level,
                RuntimeMemoryPressureLevel::Critical
            )
        {
            return RuntimeHostPressureLevel::Critical;
        }
        if self.control_plane_lag_high
            || matches!(self.cpu_level, RuntimeHostPressureLevel::High)
            || matches!(
                self.cpu_source_status,
                RuntimeHostPressureSourceStatus::Unavailable
            )
            || !matches!(
                self.memory_decision.level,
                RuntimeMemoryPressureLevel::Nominal
            )
            || matches!(
                self.memory_decision.source_status,
                RuntimeMemoryPressureSourceStatus::Unavailable
            )
        {
            return RuntimeHostPressureLevel::High;
        }
        RuntimeHostPressureLevel::Nominal
    }
}

impl RuntimeHostResourceDecision {
    pub fn admission_for_in_flight(
        self,
        current_host_in_flight: usize,
        work_class: RuntimeHostWorkClass,
        tenant_quota_remaining: bool,
    ) -> RuntimeHostAdmissionDecision {
        let over_capacity_action = self.over_capacity_action_for(work_class);
        let action = if matches!(over_capacity_action, RuntimeHostAdmissionAction::Shed) {
            RuntimeHostAdmissionAction::Shed
        } else if current_host_in_flight < self.effective_dispatch_seats {
            RuntimeHostAdmissionAction::Admit
        } else {
            RuntimeHostAdmissionAction::Queue
        };
        RuntimeHostAdmissionDecision {
            work_class,
            action,
            over_capacity_action,
            tenant_quota_remaining,
            host_pressure_level: self.host_pressure_level,
            current_host_in_flight,
            effective_dispatch_seats: self.effective_dispatch_seats,
        }
    }

    pub fn over_capacity_action_for(
        self,
        work_class: RuntimeHostWorkClass,
    ) -> RuntimeHostAdmissionAction {
        match (self.host_pressure_level, work_class) {
            (RuntimeHostPressureLevel::Nominal, _) => RuntimeHostAdmissionAction::Admit,
            (RuntimeHostPressureLevel::High, RuntimeHostWorkClass::Guaranteed) => {
                RuntimeHostAdmissionAction::Admit
            }
            (RuntimeHostPressureLevel::High, RuntimeHostWorkClass::Burstable) => {
                RuntimeHostAdmissionAction::Queue
            }
            (RuntimeHostPressureLevel::High, RuntimeHostWorkClass::BestEffort) => {
                RuntimeHostAdmissionAction::Shed
            }
            (RuntimeHostPressureLevel::Critical, RuntimeHostWorkClass::Guaranteed) => {
                RuntimeHostAdmissionAction::Queue
            }
            (RuntimeHostPressureLevel::Critical, _) => RuntimeHostAdmissionAction::Shed,
        }
    }
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn u32_to_usize_saturating(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

use std::path::{Path, PathBuf};

use nimbus_core::{Error, Result};
use nimbus_runtime::{
    RuntimeHostPressureLevel, RuntimeHostPressureSample, RuntimeHostPressureSource,
    RuntimeHostPressureSourceStatus, RuntimeMemoryPressureSample,
};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupV2MemoryPressureSource {
    cgroup_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CgroupV2HostPressureSource {
    memory_source: CgroupV2MemoryPressureSource,
    cpu_thresholds: CgroupV2CpuPressureThresholds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CgroupV2CpuPressureThresholds {
    high_some_avg10_centipercent: u32,
    critical_some_avg10_centipercent: u32,
    critical_full_avg10_centipercent: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostMemoryPressureObservation {
    cgroup_path: String,
    current_usage_bytes: Option<u64>,
    high_watermark_bytes: Option<u64>,
    critical_watermark_bytes: Option<u64>,
    unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostCpuPressureObservation {
    cgroup_path: String,
    some_avg10_centipercent: Option<u32>,
    full_avg10_centipercent: Option<u32>,
    nr_periods: Option<u64>,
    nr_throttled: Option<u64>,
    throttled_usec: Option<u64>,
    pressure_level: RuntimeHostPressureLevel,
    source_status: RuntimeHostPressureSourceStatus,
    unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostPressureObservation {
    cgroup_path: String,
    cpu: HostCpuPressureObservation,
    memory: HostMemoryPressureObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CgroupV2CpuPressureSnapshot {
    some_avg10_centipercent: u32,
    full_avg10_centipercent: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CgroupV2CpuStatSnapshot {
    nr_periods: u64,
    nr_throttled: u64,
    throttled_usec: u64,
}

impl CgroupV2MemoryPressureSource {
    pub fn new(cgroup_path: impl Into<PathBuf>) -> Result<Self> {
        let cgroup_path = cgroup_path.into();
        if cgroup_path.as_os_str().is_empty() {
            return Err(Error::InvalidInput(
                "cgroup memory pressure path must not be empty".to_string(),
            ));
        }
        if !cgroup_path.is_absolute() {
            return Err(Error::InvalidInput(format!(
                "cgroup memory pressure path `{}` must be absolute",
                cgroup_path.display()
            )));
        }
        Ok(Self { cgroup_path })
    }

    pub fn cgroup_path(&self) -> &Path {
        &self.cgroup_path
    }

    pub fn observe(&self) -> HostMemoryPressureObservation {
        match self.read_observed_sample() {
            Ok(sample) => HostMemoryPressureObservation {
                cgroup_path: self.cgroup_path.display().to_string(),
                current_usage_bytes: sample.current_usage_bytes,
                high_watermark_bytes: sample.high_watermark_bytes,
                critical_watermark_bytes: sample.critical_watermark_bytes,
                unavailable_reason: None,
            },
            Err(err) => HostMemoryPressureObservation {
                cgroup_path: self.cgroup_path.display().to_string(),
                current_usage_bytes: None,
                high_watermark_bytes: None,
                critical_watermark_bytes: None,
                unavailable_reason: Some(err),
            },
        }
    }

    fn read_observed_sample(&self) -> std::result::Result<RuntimeMemoryPressureSample, String> {
        let current_usage_bytes = read_required_cgroup_bytes(&self.cgroup_path, "memory.current")?;
        let high_watermark_bytes = read_required_cgroup_bytes(&self.cgroup_path, "memory.high")?;
        let critical_watermark_bytes = read_required_cgroup_bytes(&self.cgroup_path, "memory.max")?;
        if high_watermark_bytes > critical_watermark_bytes {
            return Err(format!(
                "cgroup memory.high ({high_watermark_bytes}) exceeds memory.max ({critical_watermark_bytes})"
            ));
        }
        Ok(RuntimeMemoryPressureSample::observed(
            current_usage_bytes,
            high_watermark_bytes,
            critical_watermark_bytes,
        ))
    }
}

impl CgroupV2HostPressureSource {
    pub fn new(cgroup_path: impl Into<PathBuf>) -> Result<Self> {
        Self::with_cpu_thresholds(cgroup_path, CgroupV2CpuPressureThresholds::default())
    }

    pub fn with_cpu_thresholds(
        cgroup_path: impl Into<PathBuf>,
        cpu_thresholds: CgroupV2CpuPressureThresholds,
    ) -> Result<Self> {
        Ok(Self {
            memory_source: CgroupV2MemoryPressureSource::new(cgroup_path)?,
            cpu_thresholds,
        })
    }

    #[cfg(target_os = "linux")]
    pub fn for_current_process() -> Result<Self> {
        let cgroup_path = current_process_cgroup_v2_path()?;
        ensure_required_host_pressure_files(&cgroup_path)?;
        Self::new(cgroup_path)
    }

    pub fn observe(&self) -> HostPressureObservation {
        let memory = self.memory_source.observe();
        let cpu = self.observe_cpu();
        HostPressureObservation {
            cgroup_path: self.memory_source.cgroup_path().display().to_string(),
            cpu,
            memory,
        }
    }

    fn observe_cpu(&self) -> HostCpuPressureObservation {
        match self.read_observed_cpu() {
            Ok((pressure, stat)) => HostCpuPressureObservation {
                cgroup_path: self.memory_source.cgroup_path().display().to_string(),
                some_avg10_centipercent: Some(pressure.some_avg10_centipercent),
                full_avg10_centipercent: Some(pressure.full_avg10_centipercent),
                nr_periods: Some(stat.nr_periods),
                nr_throttled: Some(stat.nr_throttled),
                throttled_usec: Some(stat.throttled_usec),
                pressure_level: self.cpu_thresholds.classify(pressure),
                source_status: RuntimeHostPressureSourceStatus::Observed,
                unavailable_reason: None,
            },
            Err(err) => HostCpuPressureObservation {
                cgroup_path: self.memory_source.cgroup_path().display().to_string(),
                some_avg10_centipercent: None,
                full_avg10_centipercent: None,
                nr_periods: None,
                nr_throttled: None,
                throttled_usec: None,
                pressure_level: RuntimeHostPressureLevel::High,
                source_status: RuntimeHostPressureSourceStatus::Unavailable,
                unavailable_reason: Some(err),
            },
        }
    }

    fn read_observed_cpu(
        &self,
    ) -> std::result::Result<(CgroupV2CpuPressureSnapshot, CgroupV2CpuStatSnapshot), String> {
        let pressure = read_required_cpu_pressure(self.memory_source.cgroup_path())?;
        let stat = read_required_cpu_stat(self.memory_source.cgroup_path())?;
        Ok((pressure, stat))
    }
}

impl RuntimeHostPressureSource for CgroupV2HostPressureSource {
    fn sample(&self) -> RuntimeHostPressureSample {
        self.observe().sample()
    }
}

impl Default for CgroupV2CpuPressureThresholds {
    fn default() -> Self {
        Self {
            high_some_avg10_centipercent: 5_000,
            critical_some_avg10_centipercent: 8_000,
            critical_full_avg10_centipercent: 2_500,
        }
    }
}

impl CgroupV2CpuPressureThresholds {
    pub fn new(
        high_some_avg10_centipercent: u32,
        critical_some_avg10_centipercent: u32,
        critical_full_avg10_centipercent: u32,
    ) -> Result<Self> {
        if high_some_avg10_centipercent > critical_some_avg10_centipercent {
            return Err(Error::InvalidInput(
                "CPU high PSI threshold must be <= critical PSI threshold".to_string(),
            ));
        }
        if critical_some_avg10_centipercent > 10_000 || critical_full_avg10_centipercent > 10_000 {
            return Err(Error::InvalidInput(
                "CPU PSI thresholds must be <= 100.00 percent".to_string(),
            ));
        }
        Ok(Self {
            high_some_avg10_centipercent,
            critical_some_avg10_centipercent,
            critical_full_avg10_centipercent,
        })
    }

    fn classify(self, snapshot: CgroupV2CpuPressureSnapshot) -> RuntimeHostPressureLevel {
        if snapshot.some_avg10_centipercent >= self.critical_some_avg10_centipercent
            || snapshot.full_avg10_centipercent >= self.critical_full_avg10_centipercent
        {
            return RuntimeHostPressureLevel::Critical;
        }
        if snapshot.some_avg10_centipercent >= self.high_some_avg10_centipercent
            || snapshot.full_avg10_centipercent > 0
        {
            return RuntimeHostPressureLevel::High;
        }
        RuntimeHostPressureLevel::Nominal
    }
}

impl HostMemoryPressureObservation {
    pub fn sample(&self) -> RuntimeMemoryPressureSample {
        match (
            self.current_usage_bytes,
            self.high_watermark_bytes,
            self.critical_watermark_bytes,
        ) {
            (Some(current), Some(high), Some(critical)) => {
                RuntimeMemoryPressureSample::observed(current, high, critical)
            }
            _ => RuntimeMemoryPressureSample::unavailable(),
        }
    }

    pub fn cgroup_path(&self) -> &str {
        &self.cgroup_path
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable_reason.as_deref()
    }
}

impl HostCpuPressureObservation {
    pub fn pressure_level(&self) -> RuntimeHostPressureLevel {
        self.pressure_level
    }

    pub fn source_status(&self) -> RuntimeHostPressureSourceStatus {
        self.source_status
    }

    pub fn cgroup_path(&self) -> &str {
        &self.cgroup_path
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable_reason.as_deref()
    }
}

impl HostPressureObservation {
    pub fn sample(&self) -> RuntimeHostPressureSample {
        let memory_decision = self.memory.sample().classify();
        if matches!(
            self.cpu.source_status,
            RuntimeHostPressureSourceStatus::Observed
        ) {
            RuntimeHostPressureSample::observed(self.cpu.pressure_level, memory_decision, false)
        } else {
            RuntimeHostPressureSample::unavailable(memory_decision)
        }
    }

    pub fn cgroup_path(&self) -> &str {
        &self.cgroup_path
    }

    pub fn cpu(&self) -> &HostCpuPressureObservation {
        &self.cpu
    }

    pub fn memory(&self) -> &HostMemoryPressureObservation {
        &self.memory
    }
}

fn read_required_cgroup_bytes(
    cgroup_path: &Path,
    file_name: &str,
) -> std::result::Result<u64, String> {
    let raw = read_required_cgroup_text(cgroup_path, file_name)?;
    parse_cgroup_bytes(raw.trim(), file_name)
}

fn read_required_cgroup_text(
    cgroup_path: &Path,
    file_name: &str,
) -> std::result::Result<String, String> {
    let path = cgroup_path.join(file_name);
    std::fs::read_to_string(&path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))
}

fn parse_cgroup_bytes(raw: &str, file_name: &str) -> std::result::Result<u64, String> {
    if raw == "max" {
        return Err(format!("cgroup {file_name} is unlimited"));
    }
    raw.parse::<u64>()
        .map_err(|err| format!("invalid cgroup {file_name} value `{raw}`: {err}"))
}

fn read_required_cpu_pressure(
    cgroup_path: &Path,
) -> std::result::Result<CgroupV2CpuPressureSnapshot, String> {
    let raw = read_required_cgroup_text(cgroup_path, "cpu.pressure")?;
    parse_cpu_pressure(&raw)
}

fn read_required_cpu_stat(
    cgroup_path: &Path,
) -> std::result::Result<CgroupV2CpuStatSnapshot, String> {
    let raw = read_required_cgroup_text(cgroup_path, "cpu.stat")?;
    parse_cpu_stat(&raw)
}

fn parse_cpu_pressure(raw: &str) -> std::result::Result<CgroupV2CpuPressureSnapshot, String> {
    let mut some_avg10 = None;
    let mut full_avg10 = None;
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        let Some(kind) = fields.next() else {
            continue;
        };
        for field in fields {
            let Some(value) = field.strip_prefix("avg10=") else {
                continue;
            };
            match kind {
                "some" => {
                    some_avg10 = Some(parse_cpu_pressure_centipercent(value, "some avg10")?);
                }
                "full" => {
                    full_avg10 = Some(parse_cpu_pressure_centipercent(value, "full avg10")?);
                }
                _ => {}
            }
        }
    }
    Ok(CgroupV2CpuPressureSnapshot {
        some_avg10_centipercent: some_avg10
            .ok_or_else(|| "cpu.pressure missing some avg10".to_string())?,
        full_avg10_centipercent: full_avg10
            .ok_or_else(|| "cpu.pressure missing full avg10".to_string())?,
    })
}

fn parse_cpu_pressure_centipercent(raw: &str, field: &str) -> std::result::Result<u32, String> {
    let (whole, fraction) = raw
        .split_once('.')
        .map_or((raw, ""), |(whole, fraction)| (whole, fraction));
    if fraction.len() > 2 {
        return Err(format!(
            "invalid cpu.pressure {field} value `{raw}`: expected at most two decimal places"
        ));
    }
    let whole = whole.parse::<u32>().map_err(|err| {
        format!("invalid cpu.pressure {field} value `{raw}` whole percent: {err}")
    })?;
    let mut fraction_centipercent = 0u32;
    for (index, digit) in fraction.as_bytes().iter().enumerate() {
        if !digit.is_ascii_digit() {
            return Err(format!(
                "invalid cpu.pressure {field} value `{raw}` fraction"
            ));
        }
        let value = u32::from(digit - b'0');
        fraction_centipercent += if index == 0 { value * 10 } else { value };
    }
    let centipercent = whole
        .checked_mul(100)
        .and_then(|value| value.checked_add(fraction_centipercent))
        .ok_or_else(|| format!("invalid cpu.pressure {field} value `{raw}` overflowed"))?;
    if centipercent > 10_000 {
        return Err(format!(
            "invalid cpu.pressure {field} value `{raw}` exceeds 100.00"
        ));
    }
    Ok(centipercent)
}

fn parse_cpu_stat(raw: &str) -> std::result::Result<CgroupV2CpuStatSnapshot, String> {
    let mut nr_periods = None;
    let mut nr_throttled = None;
    let mut throttled_usec = None;
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        let Some(value) = fields.next() else {
            continue;
        };
        let parsed = value
            .parse::<u64>()
            .map_err(|err| format!("invalid cpu.stat {name} value `{value}`: {err}"))?;
        match name {
            "nr_periods" => nr_periods = Some(parsed),
            "nr_throttled" => nr_throttled = Some(parsed),
            "throttled_usec" => throttled_usec = Some(parsed),
            _ => {}
        }
    }
    Ok(CgroupV2CpuStatSnapshot {
        nr_periods: nr_periods.ok_or_else(|| "cpu.stat missing nr_periods".to_string())?,
        nr_throttled: nr_throttled.ok_or_else(|| "cpu.stat missing nr_throttled".to_string())?,
        throttled_usec: throttled_usec
            .ok_or_else(|| "cpu.stat missing throttled_usec".to_string())?,
    })
}

#[cfg(target_os = "linux")]
fn current_process_cgroup_v2_path() -> Result<PathBuf> {
    current_process_cgroup_v2_path_from("/proc/self/cgroup", "/sys/fs/cgroup")
}

#[cfg(target_os = "linux")]
fn current_process_cgroup_v2_path_from(
    proc_self_cgroup: impl AsRef<Path>,
    cgroup_root: impl AsRef<Path>,
) -> Result<PathBuf> {
    let proc_self_cgroup = proc_self_cgroup.as_ref();
    let cgroup_root = cgroup_root.as_ref();
    let raw = std::fs::read_to_string(proc_self_cgroup).map_err(|err| {
        Error::Internal(format!(
            "failed to read {} for current cgroup: {err}",
            proc_self_cgroup.display()
        ))
    })?;
    let relative = parse_current_cgroup_v2_relative_path(&raw)?;
    if relative == Path::new("/") {
        return Ok(cgroup_root.to_path_buf());
    }
    let relative = relative.strip_prefix("/").unwrap_or(&relative);
    Ok(cgroup_root.join(relative))
}

#[cfg(target_os = "linux")]
fn parse_current_cgroup_v2_relative_path(raw: &str) -> Result<PathBuf> {
    for line in raw.lines() {
        let mut parts = line.splitn(3, ':');
        let _hierarchy = parts.next();
        let Some(controllers) = parts.next() else {
            continue;
        };
        let Some(path) = parts.next() else {
            continue;
        };
        if controllers.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    Err(Error::Internal(
        "failed to find unified cgroup v2 entry in /proc/self/cgroup".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn ensure_required_host_pressure_files(cgroup_path: &Path) -> Result<()> {
    for file_name in [
        "cpu.pressure",
        "cpu.stat",
        "memory.current",
        "memory.high",
        "memory.max",
    ] {
        let path = cgroup_path.join(file_name);
        if !path.is_file() {
            return Err(Error::Internal(format!(
                "current cgroup {} is missing required host pressure file {}",
                cgroup_path.display(),
                file_name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use nimbus_runtime::{
        RuntimeHostPressureLevel, RuntimeHostPressureSourceStatus, RuntimeMemoryPressureLevel,
        RuntimeMemoryPressureSourceStatus,
    };

    use super::*;

    #[test]
    fn cgroup_v2_memory_pressure_observes_high_pressure() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        write_memory_file(temp.path(), "memory.current", "900");
        write_memory_file(temp.path(), "memory.high", "800");
        write_memory_file(temp.path(), "memory.max", "1000");

        let source =
            CgroupV2MemoryPressureSource::new(temp.path()).expect("cgroup source should build");
        let observation = source.observe();
        let decision = observation.sample().classify();

        assert_eq!(observation.current_usage_bytes, Some(900));
        assert_eq!(observation.high_watermark_bytes, Some(800));
        assert_eq!(observation.critical_watermark_bytes, Some(1000));
        assert_eq!(observation.unavailable_reason(), None);
        assert_eq!(decision.level, RuntimeMemoryPressureLevel::High);
        assert_eq!(
            decision.source_status,
            RuntimeMemoryPressureSourceStatus::Observed
        );
        assert!(decision.pause_prewarming);
        assert!(decision.evict_idle_retained_runtimes);
    }

    #[test]
    fn cgroup_v2_memory_pressure_observes_critical_pressure() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        write_memory_file(temp.path(), "memory.current", "1000");
        write_memory_file(temp.path(), "memory.high", "800");
        write_memory_file(temp.path(), "memory.max", "1000");

        let source =
            CgroupV2MemoryPressureSource::new(temp.path()).expect("cgroup source should build");
        let decision = source.observe().sample().classify();

        assert_eq!(decision.level, RuntimeMemoryPressureLevel::Critical);
        assert_eq!(
            decision.source_status,
            RuntimeMemoryPressureSourceStatus::Observed
        );
    }

    #[test]
    fn cgroup_v2_memory_pressure_degrades_when_watermark_is_unavailable() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        write_memory_file(temp.path(), "memory.current", "700");
        write_memory_file(temp.path(), "memory.high", "max");
        write_memory_file(temp.path(), "memory.max", "1000");

        let source =
            CgroupV2MemoryPressureSource::new(temp.path()).expect("cgroup source should build");
        let observation = source.observe();
        let decision = observation.sample().classify();

        assert!(
            observation
                .unavailable_reason()
                .is_some_and(|reason| reason.contains("memory.high is unlimited")),
            "unexpected observation: {observation:?}"
        );
        assert_eq!(decision.level, RuntimeMemoryPressureLevel::High);
        assert_eq!(
            decision.source_status,
            RuntimeMemoryPressureSourceStatus::Unavailable
        );
        assert!(decision.pause_prewarming);
        assert!(decision.evict_idle_retained_runtimes);
    }

    #[test]
    fn cgroup_v2_memory_pressure_source_requires_absolute_path() {
        let error = CgroupV2MemoryPressureSource::new("relative/cgroup")
            .expect_err("relative cgroup source path should be rejected");

        assert!(
            error
                .to_string()
                .contains("cgroup memory pressure path `relative/cgroup` must be absolute"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn cgroup_v2_host_pressure_observes_high_cpu_pressure() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        write_memory_file(temp.path(), "memory.current", "100");
        write_memory_file(temp.path(), "memory.high", "800");
        write_memory_file(temp.path(), "memory.max", "1000");
        write_cpu_pressure_file(temp.path(), "60.00", "0.00");
        write_cpu_stat_file(temp.path(), 100, 3, 2500);

        let source =
            CgroupV2HostPressureSource::new(temp.path()).expect("cgroup source should build");
        let observation = source.observe();
        let sample = observation.sample();

        assert_eq!(
            observation.cpu().source_status(),
            RuntimeHostPressureSourceStatus::Observed
        );
        assert_eq!(
            observation.cpu().pressure_level(),
            RuntimeHostPressureLevel::High
        );
        assert_eq!(sample.cpu_level, RuntimeHostPressureLevel::High);
        assert_eq!(
            sample.cpu_source_status,
            RuntimeHostPressureSourceStatus::Observed
        );
        assert_eq!(
            sample.memory_decision.level,
            RuntimeMemoryPressureLevel::Nominal
        );
    }

    #[test]
    fn cgroup_v2_host_pressure_observes_critical_cpu_pressure() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        write_memory_file(temp.path(), "memory.current", "100");
        write_memory_file(temp.path(), "memory.high", "800");
        write_memory_file(temp.path(), "memory.max", "1000");
        write_cpu_pressure_file(temp.path(), "10.00", "30.00");
        write_cpu_stat_file(temp.path(), 100, 60, 90000);

        let source =
            CgroupV2HostPressureSource::new(temp.path()).expect("cgroup source should build");
        let sample = observation_sample(&source);

        assert_eq!(sample.cpu_level, RuntimeHostPressureLevel::Critical);
        assert_eq!(
            sample.cpu_source_status,
            RuntimeHostPressureSourceStatus::Observed
        );
    }

    #[test]
    fn cgroup_v2_host_pressure_degrades_when_cpu_pressure_is_unavailable() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        write_memory_file(temp.path(), "memory.current", "100");
        write_memory_file(temp.path(), "memory.high", "800");
        write_memory_file(temp.path(), "memory.max", "1000");
        write_cpu_stat_file(temp.path(), 100, 0, 0);

        let source =
            CgroupV2HostPressureSource::new(temp.path()).expect("cgroup source should build");
        let observation = source.observe();
        let sample = observation.sample();

        assert!(
            observation
                .cpu()
                .unavailable_reason()
                .is_some_and(|reason| reason.contains("cpu.pressure")),
            "unexpected observation: {observation:?}"
        );
        assert_eq!(sample.cpu_level, RuntimeHostPressureLevel::High);
        assert_eq!(
            sample.cpu_source_status,
            RuntimeHostPressureSourceStatus::Unavailable
        );
        assert_eq!(
            sample.memory_decision.level,
            RuntimeMemoryPressureLevel::Nominal
        );
    }

    #[test]
    fn cgroup_v2_cpu_pressure_thresholds_reject_inverted_values() {
        let error = CgroupV2CpuPressureThresholds::new(8_001, 8_000, 2_500)
            .expect_err("inverted CPU threshold should be rejected");

        assert!(
            error
                .to_string()
                .contains("CPU high PSI threshold must be <= critical PSI threshold"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn cpu_pressure_parser_accepts_one_decimal_centipercent() {
        let pressure = parse_cpu_pressure("some avg10=1.2 avg60=0.00 avg300=0.00 total=1\nfull avg10=0.3 avg60=0.00 avg300=0.00 total=1\n")
            .expect("cpu pressure should parse");

        assert_eq!(pressure.some_avg10_centipercent, 120);
        assert_eq!(pressure.full_avg10_centipercent, 30);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn current_cgroup_v2_path_uses_unified_proc_entry() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let proc_file = temp.path().join("self_cgroup");
        let cgroup_root = temp.path().join("sys_fs_cgroup");
        std::fs::create_dir_all(&cgroup_root).expect("cgroup root should create");
        std::fs::write(&proc_file, "0::/user.slice/nimbus.service\n")
            .expect("proc cgroup should write");

        let path = current_process_cgroup_v2_path_from(&proc_file, &cgroup_root)
            .expect("cgroup path should parse");

        assert_eq!(path, cgroup_root.join("user.slice/nimbus.service"));
    }

    fn write_memory_file(root: &Path, file_name: &str, value: &str) {
        std::fs::write(root.join(file_name), format!("{value}\n"))
            .expect("memory cgroup file should write");
    }

    fn write_cpu_pressure_file(root: &Path, some_avg10: &str, full_avg10: &str) {
        std::fs::write(
            root.join("cpu.pressure"),
            format!(
                "some avg10={some_avg10} avg60=0.00 avg300=0.00 total=100\nfull avg10={full_avg10} avg60=0.00 avg300=0.00 total=10\n"
            ),
        )
        .expect("cpu pressure file should write");
    }

    fn write_cpu_stat_file(root: &Path, nr_periods: u64, nr_throttled: u64, throttled_usec: u64) {
        std::fs::write(
            root.join("cpu.stat"),
            format!(
                "usage_usec 100\nnr_periods {nr_periods}\nnr_throttled {nr_throttled}\nthrottled_usec {throttled_usec}\n"
            ),
        )
        .expect("cpu stat file should write");
    }

    fn observation_sample(source: &CgroupV2HostPressureSource) -> RuntimeHostPressureSample {
        source.observe().sample()
    }
}

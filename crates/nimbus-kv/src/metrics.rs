use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct NimbusKvMetrics {
    inner: Arc<MetricsInner>,
}

#[derive(Debug, Default)]
struct MetricsInner {
    connected_clients: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    durable_writes_started: AtomicU64,
    durable_writes_completed: AtomicU64,
    durable_writes_in_flight: AtomicU64,
    durable_write_latency_us_total: AtomicU64,
    commands: Mutex<BTreeMap<CommandMetricLabel, CommandMetrics>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CommandMetricLabel {
    Auth,
    Hello,
    Quit,
    Ping,
    Echo,
    Command,
    Client,
    Select,
    Get,
    Set,
    Del,
    FlushAll,
    Function,
    Expire,
    Ttl,
    Incr,
    NimbusReady,
    NimbusMetrics,
    Unknown,
}

impl CommandMetricLabel {
    fn classify(name: &str) -> Self {
        if name.eq_ignore_ascii_case("AUTH") {
            Self::Auth
        } else if name.eq_ignore_ascii_case("HELLO") {
            Self::Hello
        } else if name.eq_ignore_ascii_case("QUIT") {
            Self::Quit
        } else if name.eq_ignore_ascii_case("PING") {
            Self::Ping
        } else if name.eq_ignore_ascii_case("ECHO") {
            Self::Echo
        } else if name.eq_ignore_ascii_case("COMMAND") {
            Self::Command
        } else if name.eq_ignore_ascii_case("CLIENT") {
            Self::Client
        } else if name.eq_ignore_ascii_case("SELECT") {
            Self::Select
        } else if name.eq_ignore_ascii_case("GET") {
            Self::Get
        } else if name.eq_ignore_ascii_case("SET") {
            Self::Set
        } else if name.eq_ignore_ascii_case("DEL") {
            Self::Del
        } else if name.eq_ignore_ascii_case("FLUSHALL") {
            Self::FlushAll
        } else if name.eq_ignore_ascii_case("FUNCTION") {
            Self::Function
        } else if name.eq_ignore_ascii_case("EXPIRE") {
            Self::Expire
        } else if name.eq_ignore_ascii_case("TTL") {
            Self::Ttl
        } else if name.eq_ignore_ascii_case("INCR") {
            Self::Incr
        } else if name.eq_ignore_ascii_case("NIMBUS.READY") {
            Self::NimbusReady
        } else if name.eq_ignore_ascii_case("NIMBUS.METRICS") {
            Self::NimbusMetrics
        } else {
            Self::Unknown
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Auth => "AUTH",
            Self::Hello => "HELLO",
            Self::Quit => "QUIT",
            Self::Ping => "PING",
            Self::Echo => "ECHO",
            Self::Command => "COMMAND",
            Self::Client => "CLIENT",
            Self::Select => "SELECT",
            Self::Get => "GET",
            Self::Set => "SET",
            Self::Del => "DEL",
            Self::FlushAll => "FLUSHALL",
            Self::Function => "FUNCTION",
            Self::Expire => "EXPIRE",
            Self::Ttl => "TTL",
            Self::Incr => "INCR",
            Self::NimbusReady => "NIMBUS.READY",
            Self::NimbusMetrics => "NIMBUS.METRICS",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CommandMetrics {
    calls: u64,
    errors: u64,
    latency_us_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimbusKvMetricsSnapshot {
    pub connected_clients: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub durable_writes_started: u64,
    pub durable_writes_completed: u64,
    pub durable_writes_in_flight: u64,
    pub durable_write_latency_us_total: u64,
    pub commands: BTreeMap<String, CommandMetricsSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandMetricsSnapshot {
    pub calls: u64,
    pub errors: u64,
    pub latency_us_total: u64,
}

pub struct ClientConnectionGuard {
    metrics: NimbusKvMetrics,
}

pub struct DurableWriteGuard {
    metrics: NimbusKvMetrics,
    started_at: Instant,
    finished: bool,
}

impl NimbusKvMetrics {
    #[must_use]
    pub fn snapshot(&self) -> NimbusKvMetricsSnapshot {
        let commands = self
            .inner
            .commands
            .lock()
            .map(|commands| {
                commands
                    .iter()
                    .map(|(label, metrics)| {
                        (
                            label.label().to_owned(),
                            CommandMetricsSnapshot {
                                calls: metrics.calls,
                                errors: metrics.errors,
                                latency_us_total: metrics.latency_us_total,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        NimbusKvMetricsSnapshot {
            connected_clients: self.inner.connected_clients.load(Ordering::Relaxed),
            cache_hits: self.inner.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.inner.cache_misses.load(Ordering::Relaxed),
            durable_writes_started: self.inner.durable_writes_started.load(Ordering::Relaxed),
            durable_writes_completed: self.inner.durable_writes_completed.load(Ordering::Relaxed),
            durable_writes_in_flight: self.inner.durable_writes_in_flight.load(Ordering::Relaxed),
            durable_write_latency_us_total: self
                .inner
                .durable_write_latency_us_total
                .load(Ordering::Relaxed),
            commands,
        }
    }

    #[must_use]
    pub fn render_text(&self) -> String {
        self.snapshot().render_text()
    }

    pub(crate) fn client_connected(&self) -> ClientConnectionGuard {
        self.inner.connected_clients.fetch_add(1, Ordering::Relaxed);
        ClientConnectionGuard {
            metrics: self.clone(),
        }
    }

    pub(crate) fn record_cache_hit(&self) {
        self.inner.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cache_miss(&self) {
        self.inner.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn start_durable_write(&self) -> DurableWriteGuard {
        self.inner
            .durable_writes_started
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .durable_writes_in_flight
            .fetch_add(1, Ordering::Relaxed);
        DurableWriteGuard {
            metrics: self.clone(),
            started_at: Instant::now(),
            finished: false,
        }
    }

    pub(crate) fn record_command(&self, name: &str, elapsed: Duration, error: bool) {
        let label = CommandMetricLabel::classify(name);
        let Ok(mut commands) = self.inner.commands.lock() else {
            return;
        };
        let metrics = commands.entry(label).or_default();
        metrics.calls = metrics.calls.saturating_add(1);
        if error {
            metrics.errors = metrics.errors.saturating_add(1);
        }
        metrics.latency_us_total = metrics
            .latency_us_total
            .saturating_add(duration_micros(elapsed));
    }
}

impl NimbusKvMetricsSnapshot {
    #[must_use]
    pub fn render_text(&self) -> String {
        let total_cache = self.cache_hits.saturating_add(self.cache_misses);
        let cache_hit_ratio_ppm = self
            .cache_hits
            .saturating_mul(1_000_000)
            .checked_div(total_cache)
            .unwrap_or(0);

        let mut out = String::new();
        let _ = writeln!(out, "# Nimbus KV");
        let _ = writeln!(out, "readiness:ready");
        let _ = writeln!(out, "connected_clients:{}", self.connected_clients);
        let _ = writeln!(out, "cache_hits:{}", self.cache_hits);
        let _ = writeln!(out, "cache_misses:{}", self.cache_misses);
        let _ = writeln!(out, "cache_hit_ratio_ppm:{cache_hit_ratio_ppm}");
        let _ = writeln!(
            out,
            "durable_writes_started:{}",
            self.durable_writes_started
        );
        let _ = writeln!(
            out,
            "durable_writes_completed:{}",
            self.durable_writes_completed
        );
        let _ = writeln!(
            out,
            "durable_writes_in_flight:{}",
            self.durable_writes_in_flight
        );
        let _ = writeln!(
            out,
            "durable_write_latency_us_total:{}",
            self.durable_write_latency_us_total
        );
        for (name, command) in &self.commands {
            let _ = writeln!(out, "command.{name}.calls:{}", command.calls);
            let _ = writeln!(out, "command.{name}.errors:{}", command.errors);
            let _ = writeln!(
                out,
                "command.{name}.latency_us_total:{}",
                command.latency_us_total
            );
        }
        out
    }
}

impl DurableWriteGuard {
    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.metrics
            .inner
            .durable_writes_in_flight
            .fetch_sub(1, Ordering::Relaxed);
        self.metrics
            .inner
            .durable_writes_completed
            .fetch_add(1, Ordering::Relaxed);
        self.metrics.inner.durable_write_latency_us_total.fetch_add(
            duration_micros(self.started_at.elapsed()),
            Ordering::Relaxed,
        );
    }
}

impl Drop for DurableWriteGuard {
    fn drop(&mut self) {
        self.finish();
    }
}

impl Drop for ClientConnectionGuard {
    fn drop(&mut self) {
        self.metrics
            .inner
            .connected_clients
            .fetch_sub(1, Ordering::Relaxed);
    }
}

fn duration_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUPPORTED_COMMANDS: [&str; 18] = [
        "AUTH",
        "HELLO",
        "QUIT",
        "PING",
        "ECHO",
        "COMMAND",
        "CLIENT",
        "SELECT",
        "GET",
        "SET",
        "DEL",
        "FLUSHALL",
        "FUNCTION",
        "EXPIRE",
        "TTL",
        "INCR",
        "NIMBUS.READY",
        "NIMBUS.METRICS",
    ];

    #[test]
    fn supported_commands_keep_stable_case_insensitive_labels() {
        let metrics = NimbusKvMetrics::default();
        for name in SUPPORTED_COMMANDS {
            metrics.record_command(&name.to_ascii_lowercase(), Duration::from_micros(2), false);
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.commands.len(), SUPPORTED_COMMANDS.len());
        for name in SUPPORTED_COMMANDS {
            let command = snapshot
                .commands
                .get(name)
                .unwrap_or_else(|| panic!("missing stable metric label for {name}"));
            assert_eq!(command.calls, 1, "wrong call count for {name}");
            assert_eq!(command.errors, 0, "wrong error count for {name}");
            assert_eq!(command.latency_us_total, 2, "wrong latency for {name}");
        }
        assert_eq!(snapshot.commands.len(), 18);
        assert!(!snapshot.render_text().contains("command.unknown"));
    }

    #[test]
    fn unknown_client_commands_share_one_bounded_metric_label() {
        let metrics = NimbusKvMetrics::default();
        for index in 0..128 {
            metrics.record_command(
                &format!("CLIENT-SUPPLIED-UNKNOWN-{index}"),
                Duration::from_micros(1),
                true,
            );
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.commands.len(), 1);
        let unknown = snapshot
            .commands
            .get("UNKNOWN")
            .expect("unsupported commands must share one metric label");
        assert_eq!(unknown.calls, 128);
        assert_eq!(unknown.errors, 128);
        assert_eq!(unknown.latency_us_total, 128);
        assert!(
            snapshot
                .commands
                .keys()
                .all(|name| !name.starts_with("CLIENT-SUPPLIED")),
            "untrusted command names must not become metric keys"
        );
    }

    #[test]
    fn command_metric_cardinality_never_exceeds_closed_label_set() {
        let metrics = NimbusKvMetrics::default();
        for name in SUPPORTED_COMMANDS {
            metrics.record_command(name, Duration::ZERO, false);
        }
        for index in 0..128 {
            metrics.record_command(&format!("unknown-{index}"), Duration::ZERO, true);
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.commands.len(), SUPPORTED_COMMANDS.len() + 1);
        assert_eq!(snapshot.commands["UNKNOWN"].calls, 128);
    }
}

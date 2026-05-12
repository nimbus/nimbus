use super::common::read_round_override;
use super::*;

#[derive(Debug, Clone)]
pub(super) struct BenchmarkConfig {
    pub(super) markdown_output: Option<PathBuf>,
    pub(super) workload_filter: Option<WorkloadKind>,
    pub(super) encryption_mode: EncryptionMode,
}

impl BenchmarkConfig {
    pub(super) fn from_args() -> BenchResult<Self> {
        let mut markdown_output = None;
        let mut workload_filter = None;
        let mut encryption_mode = EncryptionMode::Disabled;
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--markdown" => {
                    let Some(path) = args.next() else {
                        return Err("expected a path after --markdown".into());
                    };
                    markdown_output = Some(PathBuf::from(path));
                }
                "--workload" => {
                    let Some(workload) = args.next() else {
                        return Err("expected a workload after --workload".into());
                    };
                    workload_filter = Some(WorkloadKind::parse(workload.as_str())?);
                }
                "--local-encryption" => {
                    let Some(mode) = args.next() else {
                        return Err("expected a value after --local-encryption".into());
                    };
                    encryption_mode = EncryptionMode::parse(mode.as_str())?;
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                "--bench" => {
                    // Cargo forwards this marker to benchmark binaries even when
                    // `harness = false`; ignore it so repo-owned flags keep working.
                }
                _ => {
                    return Err(format!("unknown argument: {arg}").into());
                }
            }
        }
        Ok(Self {
            markdown_output,
            workload_filter,
            encryption_mode,
        })
    }
}

fn print_usage() {
    println!(
        "Usage: cargo bench -p nimbus-engine --bench embedded-provider-benchmarks -- [--markdown <path>] [--workload <slug>] [--local-encryption <disabled|temp-master-key-file>]"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum EncryptionMode {
    #[default]
    Disabled,
    TempMasterKeyFile,
}

impl EncryptionMode {
    pub(super) fn parse(value: &str) -> BenchResult<Self> {
        match value {
            "disabled" => Ok(Self::Disabled),
            "temp-master-key-file" => Ok(Self::TempMasterKeyFile),
            _ => Err(format!("unknown local encryption mode: {value}").into()),
        }
    }

    pub(super) fn cli_value(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::TempMasterKeyFile => "temp-master-key-file",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Disabled => "plaintext local files",
            Self::TempMasterKeyFile => "manifest-backed local encryption",
        }
    }

    pub(super) fn notes(self) -> &'static str {
        match self {
            Self::Disabled => {
                "uses the current plaintext local-file path with no manifest or DEK unwrap work"
            }
            Self::TempMasterKeyFile => {
                "enables the real startup path with a benchmark-only master key file so every local database still uses a manifest-backed random DEK"
            }
        }
    }

    pub(super) fn is_enabled(self) -> bool {
        matches!(self, Self::TempMasterKeyFile)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkloadKind {
    CrudThroughput,
    PointReadLatency,
    IndexedQueryLatency,
    CompositeIndexedQueryLatency,
    DurableJournalStreamLatency,
    DurableJournalBootstrapLatency,
    SubscriptionFanoutLatency,
    MixedMultiTenantLoad,
}

impl WorkloadKind {
    pub(super) fn parse(value: &str) -> BenchResult<Self> {
        match value {
            "crud" => Ok(Self::CrudThroughput),
            "point-read" => Ok(Self::PointReadLatency),
            "indexed-query" => Ok(Self::IndexedQueryLatency),
            "composite-indexed-query" => Ok(Self::CompositeIndexedQueryLatency),
            "journal-stream" => Ok(Self::DurableJournalStreamLatency),
            "journal-bootstrap" => Ok(Self::DurableJournalBootstrapLatency),
            "subscription-fanout" => Ok(Self::SubscriptionFanoutLatency),
            "mixed-load" => Ok(Self::MixedMultiTenantLoad),
            _ => Err(format!("unknown workload slug: {value}").into()),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::CrudThroughput => "document CRUD throughput",
            Self::PointReadLatency => "point read latency",
            Self::IndexedQueryLatency => "indexed query latency",
            Self::CompositeIndexedQueryLatency => "composite indexed query latency",
            Self::DurableJournalStreamLatency => "durable journal stream latency",
            Self::DurableJournalBootstrapLatency => "durable journal bootstrap latency",
            Self::SubscriptionFanoutLatency => "subscription fan-out latency",
            Self::MixedMultiTenantLoad => "concurrent multi-tenant mixed read/write load",
        }
    }

    pub(super) fn notes(self) -> &'static str {
        match self {
            Self::CrudThroughput => {
                "async insert + update + delete through the Service mutation path"
            }
            Self::PointReadLatency => "batched async `get_document_async` over preseeded documents",
            Self::IndexedQueryLatency => {
                "single-field `status` equality query through planner-selected index path"
            }
            Self::CompositeIndexedQueryLatency => {
                "three-field composite index query with exact-prefix + range filters"
            }
            Self::DurableJournalStreamLatency => {
                "async `stream_durable_journal_async` from cursor 0 with a fixed page limit"
            }
            Self::DurableJournalBootstrapLatency => {
                "async `export_durable_journal_bootstrap_async` on a seeded tenant"
            }
            Self::SubscriptionFanoutLatency => {
                "time from one matching write to receipt of updates across all active subscriptions"
            }
            Self::MixedMultiTenantLoad => {
                "concurrent per-tenant mix of point reads, indexed queries, inserts, and updates"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BenchmarkLane {
    SteadyState,
    ColdStart,
}

impl BenchmarkLane {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::SteadyState => "Steady-State",
            Self::ColdStart => "Cold-Start",
        }
    }

    pub(super) fn notes(self) -> &'static str {
        match self {
            Self::SteadyState => {
                "reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process"
            }
            Self::ColdStart => {
                "measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result"
            }
        }
    }

    pub(super) fn warmup_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NIMBUS_BENCH_STEADY_WARMUP_ROUNDS",
                STEADY_STATE_WARMUP_ROUNDS,
            ),
            Self::ColdStart => {
                read_round_override("NIMBUS_BENCH_COLD_WARMUP_ROUNDS", COLD_START_WARMUP_ROUNDS)
            }
        }
    }

    pub(super) fn measure_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NIMBUS_BENCH_STEADY_MEASURE_ROUNDS",
                STEADY_STATE_MEASURE_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NIMBUS_BENCH_COLD_MEASURE_ROUNDS",
                COLD_START_MEASURE_ROUNDS,
            ),
        }
    }
}

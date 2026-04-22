use super::common::read_round_override;
use super::*;

#[derive(Debug, Clone)]
pub(super) struct BenchmarkConfig {
    pub(super) markdown_output: Option<PathBuf>,
    pub(super) workload_filters: Vec<WorkloadKind>,
    pub(super) local_cache_encryption: LocalCacheEncryptionMode,
    pub(super) primary_url: String,
    pub(super) auth_token: Option<String>,
    pub(super) admin_api_url: String,
    pub(super) admin_auth_header: Option<String>,
}

impl BenchmarkConfig {
    pub(super) fn from_args() -> BenchResult<Self> {
        let mut markdown_output = None;
        let mut workload_filters = Vec::new();
        let mut local_cache_encryption = LocalCacheEncryptionMode::Disabled;
        let mut primary_url = env::var(LIBSQL_URL_ENV).ok();
        let mut auth_token = env::var(LIBSQL_AUTH_TOKEN_ENV).ok();
        let mut admin_api_url = env::var(LIBSQL_ADMIN_URL_ENV).ok();
        let mut admin_auth_header = env::var(LIBSQL_ADMIN_AUTH_HEADER_ENV).ok();
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
                    let workload = WorkloadKind::parse(workload.as_str())?;
                    if !workload_filters.contains(&workload) {
                        workload_filters.push(workload);
                    }
                }
                "--local-cache-encryption" => {
                    let Some(mode) = args.next() else {
                        return Err("expected a value after --local-cache-encryption".into());
                    };
                    local_cache_encryption = LocalCacheEncryptionMode::parse(mode.as_str())?;
                }
                "--libsql-url" => {
                    let Some(url) = args.next() else {
                        return Err("expected a URL after --libsql-url".into());
                    };
                    primary_url = Some(url);
                }
                "--libsql-auth-token" => {
                    let Some(token) = args.next() else {
                        return Err("expected a token after --libsql-auth-token".into());
                    };
                    auth_token = Some(token);
                }
                "--libsql-admin-url" => {
                    let Some(url) = args.next() else {
                        return Err("expected a URL after --libsql-admin-url".into());
                    };
                    admin_api_url = Some(url);
                }
                "--libsql-admin-auth-header" => {
                    let Some(header) = args.next() else {
                        return Err("expected a header after --libsql-admin-auth-header".into());
                    };
                    admin_auth_header = Some(header);
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                "--bench" => {
                    // Cargo forwards this marker to benchmark binaries even when
                    // `harness = false`; ignore it so repo-owned flags keep working.
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }

        let Some(primary_url) = primary_url else {
            return Err(format!(
                "set {LIBSQL_URL_ENV} or pass --libsql-url for the benchmark target"
            )
            .into());
        };
        let Some(admin_api_url) = admin_api_url else {
            return Err(format!(
                "set {LIBSQL_ADMIN_URL_ENV} or pass --libsql-admin-url for the benchmark target"
            )
            .into());
        };

        Ok(Self {
            markdown_output,
            workload_filters,
            local_cache_encryption,
            primary_url,
            auth_token,
            admin_api_url,
            admin_auth_header,
        })
    }
}

fn print_usage() {
    println!(
        "Usage: cargo bench -p neovex-engine --bench libsql-replica-provider-benchmarks -- [--markdown <path>] [--workload <slug>] [--local-cache-encryption <disabled|temp-master-key-file>] [--libsql-url <url>] [--libsql-auth-token <token>] [--libsql-admin-url <url>] [--libsql-admin-auth-header <header>]"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum LocalCacheEncryptionMode {
    #[default]
    Disabled,
    TempMasterKeyFile,
}

impl LocalCacheEncryptionMode {
    pub(super) fn parse(value: &str) -> BenchResult<Self> {
        match value {
            "disabled" => Ok(Self::Disabled),
            "temp-master-key-file" => Ok(Self::TempMasterKeyFile),
            _ => Err(format!("unknown local cache encryption mode: {value}").into()),
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
            Self::Disabled => "plaintext local cache",
            Self::TempMasterKeyFile => "manifest-backed encrypted local cache",
        }
    }

    pub(super) fn notes(self) -> &'static str {
        match self {
            Self::Disabled => {
                "uses the current plaintext cache path for the local replica copy and control-plane files"
            }
            Self::TempMasterKeyFile => {
                "enables the real service startup path with a benchmark-only master key file so control-plane redb and replica cache SQLite files both reopen through manifest-backed DEKs"
            }
        }
    }

    pub(super) fn is_enabled(self) -> bool {
        matches!(self, Self::TempMasterKeyFile)
    }
}

pub(super) struct BenchmarkEnvironment {
    pub(super) primary_url: String,
    pub(super) auth_token: Option<String>,
    pub(super) admin_api_url: String,
    pub(super) admin_auth_header: Option<String>,
}

impl BenchmarkEnvironment {
    pub(super) fn new(config: &BenchmarkConfig) -> Self {
        Self {
            primary_url: config.primary_url.clone(),
            auth_token: config.auth_token.clone(),
            admin_api_url: config.admin_api_url.clone(),
            admin_auth_header: config.admin_auth_header.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkloadKind {
    CrudThroughput,
    PointReadLatency,
    IndexedQueryLatency,
    CompositeIndexedQueryLatency,
    MixedMultiTenantLoad,
    BarrierRefreshLatency,
    PeerCatchUpLatency,
}

impl WorkloadKind {
    pub(super) fn parse(value: &str) -> BenchResult<Self> {
        match value {
            "crud" => Ok(Self::CrudThroughput),
            "point-read" => Ok(Self::PointReadLatency),
            "indexed-query" => Ok(Self::IndexedQueryLatency),
            "composite-indexed-query" => Ok(Self::CompositeIndexedQueryLatency),
            "mixed-load" => Ok(Self::MixedMultiTenantLoad),
            "barrier-refresh" => Ok(Self::BarrierRefreshLatency),
            "peer-catch-up" => Ok(Self::PeerCatchUpLatency),
            _ => Err(format!("unknown workload slug: {value}").into()),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::CrudThroughput => "document CRUD throughput",
            Self::PointReadLatency => "point read latency",
            Self::IndexedQueryLatency => "indexed query latency",
            Self::CompositeIndexedQueryLatency => "composite indexed query latency",
            Self::MixedMultiTenantLoad => "concurrent multi-tenant mixed read/write load",
            Self::BarrierRefreshLatency => "same-service barrier refresh latency",
            Self::PeerCatchUpLatency => "peer catch-up / delegated-write visibility latency",
        }
    }

    pub(super) fn cli_value(self) -> &'static str {
        match self {
            Self::CrudThroughput => "crud",
            Self::PointReadLatency => "point-read",
            Self::IndexedQueryLatency => "indexed-query",
            Self::CompositeIndexedQueryLatency => "composite-indexed-query",
            Self::MixedMultiTenantLoad => "mixed-load",
            Self::BarrierRefreshLatency => "barrier-refresh",
            Self::PeerCatchUpLatency => "peer-catch-up",
        }
    }

    pub(super) fn notes(self) -> &'static str {
        match self {
            Self::CrudThroughput => {
                "async insert + update + delete through the canonical service mutation path"
            }
            Self::PointReadLatency => "batched async `get_document_async` over seeded documents",
            Self::IndexedQueryLatency => {
                "single-field `status` equality query through the planner-selected index path"
            }
            Self::CompositeIndexedQueryLatency => {
                "three-field composite index query with exact-prefix + range filters"
            }
            Self::MixedMultiTenantLoad => {
                "concurrent per-tenant mix of point reads, indexed queries, inserts, and updates"
            }
            Self::BarrierRefreshLatency => {
                "time from a committed replica-backed write returning to the first same-service read completing against a refreshed derivative cache"
            }
            Self::PeerCatchUpLatency => {
                "time from a delegated write on one replica-backed service to visibility on a second service through poll-driven catch-up"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BenchmarkLane {
    SteadyState,
    ColdStart,
    ReplicaOperational,
}

impl BenchmarkLane {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::SteadyState => "Steady-State",
            Self::ColdStart => "Cold-Start",
            Self::ReplicaOperational => "Replica-Operational",
        }
    }

    pub(super) fn notes(self) -> &'static str {
        match self {
            Self::SteadyState => "reuses warmed services and alternates backend order every round",
            Self::ColdStart => {
                "times a fresh service/runtime open plus the first representative execution"
            }
            Self::ReplicaOperational => {
                "reuses warmed replica-backed services and measures the explicit refresh/catch-up drills that define semantic freshness for this provider family"
            }
        }
    }

    pub(super) fn warmup_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_STEADY_WARMUP_ROUNDS",
                STEADY_STATE_WARMUP_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_COLD_WARMUP_ROUNDS",
                COLD_START_WARMUP_ROUNDS,
            ),
            Self::ReplicaOperational => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_OPERATIONAL_WARMUP_ROUNDS",
                OPERATIONAL_WARMUP_ROUNDS,
            ),
        }
    }

    pub(super) fn measure_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_STEADY_MEASURE_ROUNDS",
                STEADY_STATE_MEASURE_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_COLD_MEASURE_ROUNDS",
                COLD_START_MEASURE_ROUNDS,
            ),
            Self::ReplicaOperational => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_OPERATIONAL_MEASURE_ROUNDS",
                OPERATIONAL_MEASURE_ROUNDS,
            ),
        }
    }
}

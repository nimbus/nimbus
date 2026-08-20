//! The provider qualification matrix: one row per provider, one column per
//! storage semantic contract, every cell an explicit decision.
//!
//! Before this matrix existed, provider coverage was a property of whatever
//! tests happened to be written. Deleting a provider scenario left the suite
//! fully green and emitted no diagnostic at all, and a provider whose fixtures
//! were absent reported the same green as one that had actually run. Both are
//! silent losses of qualification, and invariant 12 forbids the second one:
//! an unavailable lane is `UNVERIFIED`, never green.
//!
//! The matrix closes both. It is a closed product -- every provider in the
//! roster names a position on every dimension -- and the gate below checks it
//! three ways:
//!
//! 1. Against `traits/provider_impls.rs`, so registering a seventh provider,
//!    or moving a provider onto or off the fenced apply, fails here until the
//!    matrix says where it stands.
//! 2. Against the test tree, so a `Qualified` cell whose named scenario is
//!    renamed or deleted fails instead of quietly evaporating.
//! 3. Against runtime availability, so a provider whose feature is off or
//!    whose external fixtures are absent reports `UNVERIFIED` rather than
//!    inheriting the green of a skipped test.
//!
//! `NotOwned` is a declared position, not a gap: redb, SQLite, and the
//! in-memory store carry `impl_unsupported_fenced_durable_apply!` because a
//! single-process backend has no second writer to fence off. The gate derives
//! that same set from source and fails when a row disagrees with it.

use crate::diagnostics::{SemanticContractProfile, SemanticQualification};

/// A provider in the qualification roster.
///
/// The roster is exactly the set of stores that `impl_durable_journal!`
/// registers, which is the seam every dimension here is stated against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Provider {
    Redb,
    Memory,
    Sqlite,
    Postgres,
    MySql,
    Libsql,
}

/// A storage semantic contract a provider is qualified against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Dimension {
    AtomicEffects,
    CommitterFencing,
    ConditionalAdmission,
    JournalProgress,
    DurableRecovery,
    WriteIsolation,
    PositionParity,
}

/// The matrix position of one (provider, dimension) pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cell {
    /// The provider owns the contract, and the named scenario qualifies it.
    Qualified(&'static str),
    /// The provider does not implement the contract, with the source-derived
    /// reason it does not.
    NotOwned(&'static str),
}

/// Whether this build can run a provider's scenarios at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Availability {
    /// Compiled in, with whatever fixtures it needs present.
    Available,
    /// The provider's cargo feature is off, so its tests are not in this
    /// binary.
    FeatureDisabled(&'static str),
    /// Compiled in, but the external fixture this provider connects to is not
    /// configured, so its tests skip.
    FixtureAbsent(&'static str),
}

/// What the matrix reports for one cell in *this* build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// The scenario ran and qualified the contract.
    Qualified(&'static str),
    /// The provider does not own the contract.
    NotOwned(&'static str),
    /// Nothing ran, so nothing is qualified. Never green.
    Unverified(String),
}

const PROVIDERS: [Provider; 6] = [
    Provider::Redb,
    Provider::Memory,
    Provider::Sqlite,
    Provider::Postgres,
    Provider::MySql,
    Provider::Libsql,
];

const DIMENSIONS: [Dimension; 7] = [
    Dimension::AtomicEffects,
    Dimension::CommitterFencing,
    Dimension::ConditionalAdmission,
    Dimension::JournalProgress,
    Dimension::DurableRecovery,
    Dimension::WriteIsolation,
    Dimension::PositionParity,
];

/// The reason every single-process backend carries for `CommitterFencing`.
const NO_LEASE_TO_FENCE: &str =
    "impl_unsupported_fenced_durable_apply!: no committer lease, so no stale owner to fence";

/// The closed matrix. Forty-two cells, no omissions, no defaults.
const MATRIX: [(Provider, Dimension, Cell); 42] = [
    // redb
    (
        Provider::Redb,
        Dimension::AtomicEffects,
        Cell::Qualified("redb_tenant_store_durable_journal_conformance"),
    ),
    (
        Provider::Redb,
        Dimension::CommitterFencing,
        Cell::NotOwned(NO_LEASE_TO_FENCE),
    ),
    (
        Provider::Redb,
        Dimension::ConditionalAdmission,
        Cell::Qualified(
            "redb_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes",
        ),
    ),
    (
        Provider::Redb,
        Dimension::JournalProgress,
        Cell::Qualified("redb_journal_progress_round_trips_through_insert_update_delete"),
    ),
    (
        Provider::Redb,
        Dimension::DurableRecovery,
        Cell::Qualified("redb_durable_recovery_replays_durable_but_unapplied_records"),
    ),
    (
        Provider::Redb,
        Dimension::WriteIsolation,
        Cell::Qualified("redb_pending_prefix_blocks_generic_zero_write"),
    ),
    (
        Provider::Redb,
        Dimension::PositionParity,
        Cell::Qualified("redb_materialized_position_matches_the_provider_independent_reference"),
    ),
    // in-memory
    (
        Provider::Memory,
        Dimension::AtomicEffects,
        Cell::Qualified("memory_tenant_store_durable_journal_conformance"),
    ),
    (
        Provider::Memory,
        Dimension::CommitterFencing,
        Cell::NotOwned(NO_LEASE_TO_FENCE),
    ),
    (
        Provider::Memory,
        Dimension::ConditionalAdmission,
        Cell::Qualified(
            "memory_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes",
        ),
    ),
    (
        Provider::Memory,
        Dimension::JournalProgress,
        Cell::Qualified("memory_journal_progress_round_trips_through_insert_update_delete"),
    ),
    (
        Provider::Memory,
        Dimension::DurableRecovery,
        Cell::Qualified("memory_durable_recovery_replays_durable_but_unapplied_records"),
    ),
    (
        Provider::Memory,
        Dimension::WriteIsolation,
        Cell::Qualified("memory_pending_prefix_blocks_generic_zero_write"),
    ),
    (
        Provider::Memory,
        Dimension::PositionParity,
        Cell::Qualified("memory_materialized_position_matches_the_provider_independent_reference"),
    ),
    // SQLite
    (
        Provider::Sqlite,
        Dimension::AtomicEffects,
        Cell::Qualified("sqlite_execution_unit_batch_rolls_back_when_schedule_ops_fail"),
    ),
    (
        Provider::Sqlite,
        Dimension::CommitterFencing,
        Cell::NotOwned(NO_LEASE_TO_FENCE),
    ),
    (
        Provider::Sqlite,
        Dimension::ConditionalAdmission,
        Cell::Qualified(
            "sqlite_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes",
        ),
    ),
    (
        Provider::Sqlite,
        Dimension::JournalProgress,
        Cell::Qualified("sqlite_journal_progress_round_trips_through_insert_update_delete"),
    ),
    (
        Provider::Sqlite,
        Dimension::DurableRecovery,
        Cell::Qualified("sqlite_recovery_replays_durable_but_unapplied_records"),
    ),
    (
        Provider::Sqlite,
        Dimension::WriteIsolation,
        Cell::Qualified("sqlite_pending_prefix_blocks_generic_zero_write"),
    ),
    (
        Provider::Sqlite,
        Dimension::PositionParity,
        Cell::Qualified("sqlite_materialized_position_matches_the_provider_independent_reference"),
    ),
    // PostgreSQL
    (
        Provider::Postgres,
        Dimension::AtomicEffects,
        Cell::Qualified("postgres_sql_pipeline_cancellation_rolls_back"),
    ),
    (
        Provider::Postgres,
        Dimension::CommitterFencing,
        Cell::Qualified("postgres_fenced_durable_apply_contract_is_atomic"),
    ),
    (
        Provider::Postgres,
        Dimension::ConditionalAdmission,
        Cell::Qualified(
            "postgres_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes",
        ),
    ),
    (
        Provider::Postgres,
        Dimension::JournalProgress,
        Cell::Qualified("postgres_direct_writes_dedupe_and_journal_progress_round_trip"),
    ),
    (
        Provider::Postgres,
        Dimension::DurableRecovery,
        Cell::Qualified("postgres_durable_journal_recovery_applies_pending_records"),
    ),
    (
        Provider::Postgres,
        Dimension::WriteIsolation,
        Cell::Qualified("postgres_pending_prefix_blocks_generic_zero_write"),
    ),
    (
        Provider::Postgres,
        Dimension::PositionParity,
        Cell::Qualified(
            "postgres_materialized_position_matches_the_provider_independent_reference",
        ),
    ),
    // MySQL
    (
        Provider::MySql,
        Dimension::AtomicEffects,
        Cell::Qualified("mysql_packet_bounded_journal_chunks_commit_atomically"),
    ),
    (
        Provider::MySql,
        Dimension::CommitterFencing,
        Cell::Qualified("mysql_fenced_durable_apply_contract_is_atomic"),
    ),
    (
        Provider::MySql,
        Dimension::ConditionalAdmission,
        Cell::Qualified(
            "mysql_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes",
        ),
    ),
    (
        Provider::MySql,
        Dimension::JournalProgress,
        Cell::Qualified("mysql_direct_writes_dedupe_and_journal_progress_round_trip"),
    ),
    (
        Provider::MySql,
        Dimension::DurableRecovery,
        Cell::Qualified("mysql_durable_journal_recovery_applies_pending_records"),
    ),
    (
        Provider::MySql,
        Dimension::WriteIsolation,
        Cell::Qualified("mysql_pending_prefix_blocks_generic_zero_write"),
    ),
    (
        Provider::MySql,
        Dimension::PositionParity,
        Cell::Qualified("mysql_materialized_position_matches_the_provider_independent_reference"),
    ),
    // libSQL replica
    (
        Provider::Libsql,
        Dimension::AtomicEffects,
        Cell::Qualified("libsql_pre_visibility_fault_rolls_back_and_leaves_the_store_writable"),
    ),
    (
        Provider::Libsql,
        Dimension::CommitterFencing,
        Cell::Qualified("libsql_fenced_durable_apply_contract_is_atomic"),
    ),
    (
        Provider::Libsql,
        Dimension::ConditionalAdmission,
        Cell::Qualified(
            "libsql_ppsc_different_content_sequence_reuse_is_rejected_for_all_write_shapes",
        ),
    ),
    (
        Provider::Libsql,
        Dimension::JournalProgress,
        Cell::Qualified(
            "libsql_direct_writes_refresh_derivative_cache_and_round_trip_journal_progress",
        ),
    ),
    (
        Provider::Libsql,
        Dimension::DurableRecovery,
        Cell::Qualified(
            "libsql_durable_journal_recovery_refreshes_local_cache_from_remote_records",
        ),
    ),
    (
        Provider::Libsql,
        Dimension::WriteIsolation,
        Cell::Qualified("libsql_pending_prefix_blocks_generic_zero_write"),
    ),
    (
        Provider::Libsql,
        Dimension::PositionParity,
        Cell::Qualified("libsql_materialized_position_matches_the_provider_independent_reference"),
    ),
];

impl Provider {
    /// The store type as `traits/provider_impls.rs` spells it.
    fn store_type(self) -> &'static str {
        match self {
            Self::Redb => "TenantStore",
            Self::Memory => "MemoryTenantStore",
            Self::Sqlite => "SqliteTenantStore",
            Self::Postgres => "PostgresTenantStore",
            Self::MySql => "MySqlTenantStore",
            Self::Libsql => "LibsqlReplicaTenantStore",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Redb => "redb",
            Self::Memory => "memory",
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::MySql => "mysql",
            Self::Libsql => "libsql",
        }
    }

    /// The semantic profile this provider publishes through `diagnostics`.
    ///
    /// Read out of the production `diagnostics` source rather than restated
    /// here, so editing `storage_capabilities` cannot drift away from the
    /// matrix. A remote store needs a live fixture before it can be
    /// constructed, so every provider takes the same textual route and the
    /// gate covers unavailable lanes exactly like available ones.
    fn published_profile(self) -> SemanticContractProfile {
        let source = diagnostics_source();
        let anchor = format!("impl {} {{", self.store_type());
        let start = source.find(&anchor).unwrap_or_else(|| {
            panic!(
                "{} publishes no `impl` block in diagnostics.rs",
                self.store_type()
            )
        });
        let block_end = source[start + anchor.len()..]
            .find("\n}\n")
            .map(|offset| start + anchor.len() + offset)
            .unwrap_or(source.len());
        let block = &source[start..block_end];
        assert!(
            block.contains("pub fn storage_capabilities("),
            "{} must publish storage capabilities",
            self.store_type()
        );

        let profiles: Vec<&str> = block
            .match_indices("SemanticContractProfile::")
            .map(|(offset, marker)| {
                let rest = &block[offset + marker.len()..];
                let end = rest
                    .find(|c: char| !c.is_ascii_uppercase() && c != '_')
                    .unwrap_or(rest.len());
                &rest[..end]
            })
            .collect();
        assert_eq!(
            profiles.len(),
            1,
            "{} must publish exactly one semantic profile, found {profiles:?}",
            self.store_type()
        );

        match profiles[0] {
            "FENCED" => SemanticContractProfile::FENCED,
            "LOCAL_UNFENCED" => SemanticContractProfile::LOCAL_UNFENCED,
            other => panic!(
                "{} publishes `SemanticContractProfile::{other}`, which this gate does not know; \
                 add the profile to the matrix before publishing it",
                self.store_type()
            ),
        }
    }

    /// Whether this build can actually run the provider's scenarios.
    fn availability(self) -> Availability {
        match self {
            Self::Redb | Self::Memory | Self::Sqlite => Availability::Available,
            Self::Postgres => {
                if cfg!(feature = "postgres") {
                    fixture_availability(&["NIMBUS_TEST_POSTGRES_URL"])
                } else {
                    Availability::FeatureDisabled("cargo feature `postgres` is off")
                }
            }
            Self::MySql => {
                if cfg!(feature = "mysql") {
                    fixture_availability(&["NIMBUS_MYSQL_URL"])
                } else {
                    Availability::FeatureDisabled("cargo feature `mysql` is off")
                }
            }
            Self::Libsql => {
                if cfg!(feature = "libsql") {
                    fixture_availability(&[
                        "NIMBUS_LIBSQL_URL",
                        "NIMBUS_LIBSQL_ADMIN_URL",
                        "NIMBUS_LIBSQL_ADMIN_AUTH_HEADER",
                    ])
                } else {
                    Availability::FeatureDisabled("cargo feature `libsql` is off")
                }
            }
        }
    }
}

/// An external provider is available only when every environment variable its
/// fixture needs is set and the run has not explicitly disabled implicit
/// fixtures. This mirrors `external_provider_fixture_mode` without calling it,
/// because that function panics on a partially configured fixture and this
/// gate must report rather than abort.
fn fixture_availability(required_env: &[&str]) -> Availability {
    if std::env::var_os(crate::provider_test_fixtures::DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV)
        .is_some()
    {
        return Availability::FixtureAbsent("external provider fixtures disabled for this run");
    }
    let missing = required_env
        .iter()
        .any(|name| std::env::var_os(name).is_none_or(|value| value.is_empty()));
    if missing {
        Availability::FixtureAbsent("external provider fixture environment is not configured")
    } else {
        Availability::Available
    }
}

impl Dimension {
    fn label(self) -> &'static str {
        match self {
            Self::AtomicEffects => "atomic-effects",
            Self::CommitterFencing => "committer-fencing",
            Self::ConditionalAdmission => "conditional-admission",
            Self::JournalProgress => "journal-progress",
            Self::DurableRecovery => "durable-recovery",
            Self::WriteIsolation => "write-isolation",
            Self::PositionParity => "position-parity",
        }
    }

    /// The field of the published semantic profile this dimension states.
    fn published(self, profile: &SemanticContractProfile) -> SemanticQualification {
        match self {
            Self::AtomicEffects => profile.atomic_effects,
            Self::CommitterFencing => profile.committer_fencing,
            Self::ConditionalAdmission => profile.conditional_admission,
            Self::JournalProgress => profile.journal_progress,
            Self::DurableRecovery => profile.durable_recovery,
            Self::WriteIsolation => profile.write_isolation,
            Self::PositionParity => profile.position_parity,
        }
    }
}

fn cell(provider: Provider, dimension: Dimension) -> Cell {
    MATRIX
        .iter()
        .find(|(row, column, _)| *row == provider && *column == dimension)
        .map(|(_, _, cell)| *cell)
        .expect("the matrix is closed over every provider and dimension")
}

/// What this build reports for one cell.
///
/// A `Qualified` cell degrades to `Unverified` when the provider is not
/// available: the scenario is registered but did not run, and a skipped test
/// proves no guarantee.
fn status(provider: Provider, dimension: Dimension) -> Status {
    match (cell(provider, dimension), provider.availability()) {
        (Cell::NotOwned(reason), _) => Status::NotOwned(reason),
        (Cell::Qualified(test), Availability::Available) => Status::Qualified(test),
        (Cell::Qualified(test), Availability::FeatureDisabled(reason))
        | (Cell::Qualified(test), Availability::FixtureAbsent(reason)) => {
            Status::Unverified(format!("{test} did not run: {reason}"))
        }
    }
}

/// The operator-readable matrix report for this build.
fn report() -> String {
    let mut lines = vec!["provider qualification matrix".to_string()];
    for provider in PROVIDERS {
        let availability = match provider.availability() {
            Availability::Available => "available".to_string(),
            Availability::FeatureDisabled(reason) => format!("unavailable ({reason})"),
            Availability::FixtureAbsent(reason) => format!("unavailable ({reason})"),
        };
        lines.push(format!("  {} [{availability}]", provider.label()));
        for dimension in DIMENSIONS {
            let rendered = match status(provider, dimension) {
                Status::Qualified(test) => format!("QUALIFIED  {test}"),
                Status::NotOwned(reason) => format!("NOT-OWNED  {reason}"),
                Status::Unverified(reason) => format!("UNVERIFIED {reason}"),
            };
            lines.push(format!("    {:<22} {rendered}", dimension.label()));
        }
    }
    lines.join("\n")
}

/// Every `.rs` file under the crate's `src/tests` tree, plus `tests.rs`.
fn test_tree_sources() -> Vec<String> {
    let src_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-storage tests")
        .join("src");

    let mut sources =
        vec![std::fs::read_to_string(src_dir.join("tests.rs")).expect("tests.rs must be readable")];
    let mut pending = vec![src_dir.join("tests")];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("test dir must be readable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                sources.push(std::fs::read_to_string(&path).expect("test file must be readable"));
            }
        }
    }
    sources
}

/// The production `diagnostics` source that publishes each store's profile.
fn diagnostics_source() -> String {
    let path = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-storage tests")
        .join("src/diagnostics.rs");
    std::fs::read_to_string(&path).expect("diagnostics.rs must be readable")
}

/// The store types one macro in `traits/provider_impls.rs` registers.
fn registered_store_types(macro_name: &str) -> Vec<String> {
    let path = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-storage tests")
        .join("src/traits/provider_impls.rs");
    let source = std::fs::read_to_string(&path).expect("provider_impls.rs must be readable");

    let needle = format!("{macro_name}!(");
    let mut registered = Vec::new();
    for (offset, _) in source.match_indices(&needle) {
        let rest = &source[offset + needle.len()..];
        let end = rest
            .find(')')
            .expect("a macro invocation must close its argument list");
        for argument in rest[..end].split(',') {
            let argument = argument.trim();
            if !argument.is_empty() && !registered.iter().any(|seen| seen == argument) {
                registered.push(argument.to_string());
            }
        }
    }
    registered.sort();
    registered
}

/// The gate. Nothing in the matrix is optional, derived, or defaulted.
#[test]
fn provider_contract_matrix_is_complete() {
    // 1. The roster is exactly the set of stores registered on the journal
    //    seam. A seventh provider fails here until it declares seven cells.
    let mut expected_roster: Vec<String> = PROVIDERS
        .iter()
        .map(|provider| provider.store_type().to_string())
        .collect();
    expected_roster.sort();
    assert_eq!(
        registered_store_types("impl_durable_journal"),
        expected_roster,
        "the matrix roster must equal the stores registered on impl_durable_journal!"
    );
    assert_eq!(
        registered_store_types("impl_point_write"),
        expected_roster,
        "the matrix roster must equal the stores registered on impl_point_write!"
    );

    // 2. The matrix is closed: every pair appears exactly once, and nothing
    //    else appears at all.
    assert_eq!(
        MATRIX.len(),
        PROVIDERS.len() * DIMENSIONS.len(),
        "the matrix must hold one cell per provider and dimension"
    );
    for provider in PROVIDERS {
        for dimension in DIMENSIONS {
            let declared = MATRIX
                .iter()
                .filter(|(row, column, _)| *row == provider && *column == dimension)
                .count();
            assert_eq!(
                declared,
                1,
                "{} x {} must be declared exactly once, found {declared}",
                provider.label(),
                dimension.label()
            );
        }
    }

    // 3. `NotOwned` on committer fencing is source-derived, not opinion: it
    //    must name exactly the stores carrying the unsupported fenced apply,
    //    and every other store must hold a lease implementation.
    let unfenced = registered_store_types("impl_unsupported_fenced_durable_apply");
    let leased = registered_store_types("impl_committer_lease_store");
    for provider in PROVIDERS {
        let store_type = provider.store_type().to_string();
        let owns_fencing = matches!(
            cell(provider, Dimension::CommitterFencing),
            Cell::Qualified(_)
        );
        assert_eq!(
            owns_fencing,
            leased.contains(&store_type),
            "{} declares fencing ownership that disagrees with impl_committer_lease_store!",
            provider.label()
        );
        assert_eq!(
            !owns_fencing,
            unfenced.contains(&store_type),
            "{} declares fencing ownership that disagrees with impl_unsupported_fenced_durable_apply!",
            provider.label()
        );
    }

    // 4. Every qualified cell names a scenario that exists. Deleting or
    //    renaming one fails here instead of silently reducing coverage.
    let sources = test_tree_sources();
    for (provider, dimension, cell) in MATRIX {
        let Cell::Qualified(test) = cell else {
            continue;
        };
        let declaration = format!("fn {test}(");
        assert!(
            sources.iter().any(|source| source.contains(&declaration)),
            "{} x {} names `{test}`, which no test in the tree declares",
            provider.label(),
            dimension.label()
        );
    }

    // 5. The published semantic profile and the matrix state the same thing.
    for provider in PROVIDERS {
        let profile = provider.published_profile();
        for dimension in DIMENSIONS {
            let published = dimension.published(&profile);
            let declared = match cell(provider, dimension) {
                Cell::Qualified(_) => SemanticQualification::Qualified,
                Cell::NotOwned(_) => SemanticQualification::NotOwned,
            };
            assert_eq!(
                published,
                declared,
                "{} x {}: the profile published through diagnostics disagrees with the matrix",
                provider.label(),
                dimension.label()
            );
        }
    }

    // 6. Non-vacuity: the three always-compiled providers must report every
    //    dimension as qualified or explicitly not owned, never unverified.
    for provider in [Provider::Redb, Provider::Memory, Provider::Sqlite] {
        assert_eq!(
            provider.availability(),
            Availability::Available,
            "{} is compiled into every build and must always be available",
            provider.label()
        );
        for dimension in DIMENSIONS {
            assert!(
                !matches!(status(provider, dimension), Status::Unverified(_)),
                "{} x {} must not be unverified in a default build",
                provider.label(),
                dimension.label()
            );
        }
    }

    println!("{}", report());
}

/// Invariant 12: an unavailable provider lane is `UNVERIFIED`, never green.
///
/// The three remote providers skip their scenarios when the feature is off or
/// the fixture environment is absent, and a skipped test reports the same
/// `ok` as one that ran. This gate makes the matrix refuse to inherit that
/// green.
#[test]
fn provider_contract_matrix_reports_unavailable_lanes_as_unverified() {
    for provider in [Provider::Postgres, Provider::MySql, Provider::Libsql] {
        let availability = provider.availability();
        for dimension in DIMENSIONS {
            let status = status(provider, dimension);
            match availability {
                Availability::Available => assert!(
                    !matches!(status, Status::Unverified(_)),
                    "{} is available, so {} must not report unverified",
                    provider.label(),
                    dimension.label()
                ),
                Availability::FeatureDisabled(_) | Availability::FixtureAbsent(_) => assert!(
                    matches!(status, Status::Unverified(_)),
                    "{} is unavailable, so {} must report unverified rather than green",
                    provider.label(),
                    dimension.label()
                ),
            }
        }
    }
}

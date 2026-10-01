//! Typed storage configuration.
//!
//! [`StorageConfig::from_env`] is the only runtime environment parse in this
//! crate. A composition root calls it once and passes the value to the
//! providers and stores that it opens. Tests build the struct directly.
//!
//! `StorageTestHarness::from_env` is the only other environment read. It
//! serves test-harness inputs (fixture URLs, case filters, shard selectors,
//! and seeds) that select tests and fixtures but never change storage
//! behavior. The generated-history simulation harness in the library reads
//! it too, so it is not gated to test builds.
//! `docs/private/operating/engine-storage-config.md` lists every variable.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::sqlite::MIN_SQLITE_READ_CONNECTIONS;

pub const SQLITE_MAX_READ_CONNECTIONS_ENV: &str = "NIMBUS_SQLITE_MAX_READ_CONNECTIONS";
pub const CONTROL_PLANE_PROFILE_ENV: &str = "NIMBUS_CONTROL_PLANE_PROFILE";
pub const REDB_OPEN_PROFILE_ENV: &str = "NIMBUS_REDB_OPEN_PROFILE";
pub const REDB_IO_PROFILE_ENV: &str = "NIMBUS_REDB_IO_PROFILE";
pub const REDB_JOURNAL_PROFILE_ENV: &str = "NIMBUS_REDB_JOURNAL_PROFILE";
pub const SQLITE_OPEN_PROFILE_ENV: &str = "NIMBUS_SQLITE_OPEN_PROFILE";
pub const PROFILE_ONLY_COLD_SAMPLES_ENV: &str = "NIMBUS_PROFILE_ONLY_COLD_SAMPLES";

/// Path marker that selects a store for profiling when
/// [`StorageProfileConfig::only_cold_samples`] is set.
const COLD_SAMPLE_PATH_MARKER: &str = "cold-sample";

/// Storage settings that a composition root resolves once.
///
/// `Default` is the shipped behavior: no diagnostic profiling and a SQLite
/// read-connection cap derived from host parallelism.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageConfig {
    pub profile: StorageProfileConfig,
    /// Read-connection cap for a SQLite store that is opened without an
    /// explicit cap. `None` uses host parallelism, floored at
    /// [`MIN_SQLITE_READ_CONNECTIONS`].
    pub sqlite_max_read_connections: Option<usize>,
}

impl StorageConfig {
    /// Parses every storage variable from the process environment.
    ///
    /// An absent, malformed, or zero cap keeps the default. A profile flag is
    /// on when its variable is present, whatever its value.
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var_os(key))
    }

    /// Parses every storage variable through `lookup`. A crate that embeds
    /// storage settings in its own configuration composes this parse.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        let sqlite_max_read_connections = lookup(SQLITE_MAX_READ_CONNECTIONS_ENV)
            .and_then(|value| value.into_string().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);
        Self {
            profile: StorageProfileConfig::from_lookup(&lookup),
            sqlite_max_read_connections,
        }
    }

    /// Returns the read-connection cap for a SQLite store that is opened
    /// without an explicit cap.
    pub fn sqlite_read_connection_limit(&self) -> usize {
        self.sqlite_max_read_connections.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|parallelism| parallelism.get().max(MIN_SQLITE_READ_CONNECTIONS))
                .unwrap_or(MIN_SQLITE_READ_CONNECTIONS)
        })
    }
}

/// Diagnostic timing output on standard error. Every flag is off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageProfileConfig {
    /// Emits `control-plane-profile` when the control-plane store opens.
    pub control_plane: bool,
    /// Emits `redb-open-profile` when a redb tenant store opens.
    pub redb_open: bool,
    /// Counts encrypted redb page reads. `redb-open-profile` reports them.
    pub redb_io: bool,
    /// Emits `redb-journal-profile` and `redb-read-profile` lines for
    /// snapshot export, journal stream, and schema reads.
    pub redb_journal: bool,
    /// Emits `sqlite-open-profile` and `sqlite-connection-profile` lines.
    pub sqlite_open: bool,
    /// Limits path-scoped profiles to store paths that contain
    /// `cold-sample`, so a benchmark reports only its cold reopen.
    pub only_cold_samples: bool,
}

impl StorageProfileConfig {
    fn from_lookup(lookup: &impl Fn(&str) -> Option<OsString>) -> Self {
        let present = |key| lookup(key).is_some();
        Self {
            control_plane: present(CONTROL_PLANE_PROFILE_ENV),
            redb_open: present(REDB_OPEN_PROFILE_ENV),
            redb_io: present(REDB_IO_PROFILE_ENV),
            redb_journal: present(REDB_JOURNAL_PROFILE_ENV),
            sqlite_open: present(SQLITE_OPEN_PROFILE_ENV),
            only_cold_samples: present(PROFILE_ONLY_COLD_SAMPLES_ENV),
        }
    }

    /// Returns whether a path-scoped profile may report on `path`.
    pub fn allows_path(&self, path: &Path) -> bool {
        !self.only_cold_samples || path.to_string_lossy().contains(COLD_SAMPLE_PATH_MARKER)
    }

    pub(crate) fn encrypted_read_counters(&self) -> bool {
        self.redb_open || self.redb_io
    }
}

const MANIFEST_DIR_ENV: &str = "CARGO_MANIFEST_DIR";
pub const REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str =
    "NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES";
pub const DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV: &str =
    "NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES";
pub const TEST_POSTGRES_URL_ENV: &str = "NIMBUS_TEST_POSTGRES_URL";
pub const MYSQL_URL_ENV: &str = "NIMBUS_MYSQL_URL";
pub const LIBSQL_URL_ENV: &str = "NIMBUS_LIBSQL_URL";
pub const LIBSQL_AUTH_TOKEN_ENV: &str = "NIMBUS_LIBSQL_AUTH_TOKEN";
pub const LIBSQL_ADMIN_URL_ENV: &str = "NIMBUS_LIBSQL_ADMIN_URL";
pub const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NIMBUS_LIBSQL_ADMIN_AUTH_HEADER";
pub const VERIFICATION_CASE_FILTER_ENV: &str = "NIMBUS_VERIFY_CASE";
pub const VERIFICATION_SHARD_ENV: &str = "NIMBUS_HARNESS_SHARD";
pub const STORAGE_CONFORMANCE_SEED_ENV: &str = "NIMBUS_STORAGE_CONFORMANCE_SEED";
pub const SQLITE_CRASH_DB_ENV: &str = "NIMBUS_SIC6_CRASH_DB";

/// Test-harness inputs for storage and provider tests.
///
/// Harness inputs select fixtures, cases, shards, and seeds. They never
/// configure storage behavior, so they stay out of [`StorageConfig`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageTestHarness {
    /// Crate root. Cargo and nextest set it at run time, and nextest remaps
    /// it for an archived run, so tests never read it at compile time.
    pub manifest_dir: Option<PathBuf>,
    /// Raw verification case filter. The simulation harness rejects a value
    /// that is not valid UTF-8, so a malformed filter never widens the run.
    pub verify_case: Option<OsString>,
    /// Raw verification shard selector, as `N/M`.
    pub harness_shard: Option<OsString>,
    /// Generated-history seed. A malformed value reads as absent.
    pub storage_conformance_seed: Option<u64>,
    /// Database path for the SQLite crash-durability child process.
    pub sqlite_crash_db: Option<PathBuf>,
    pub external_providers: ExternalProviderFixtureInputs,
}

impl StorageTestHarness {
    /// Reads every harness input from the process environment.
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var_os(key))
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        let utf8 = |key| lookup(key).and_then(|value| value.into_string().ok());
        Self {
            manifest_dir: lookup(MANIFEST_DIR_ENV).map(PathBuf::from),
            verify_case: lookup(VERIFICATION_CASE_FILTER_ENV),
            harness_shard: lookup(VERIFICATION_SHARD_ENV),
            storage_conformance_seed: utf8(STORAGE_CONFORMANCE_SEED_ENV)
                .and_then(|value| value.parse().ok()),
            sqlite_crash_db: lookup(SQLITE_CRASH_DB_ENV).map(PathBuf::from),
            external_providers: ExternalProviderFixtureInputs {
                required: lookup(REQUIRE_EXTERNAL_PROVIDER_FIXTURES_ENV).is_some(),
                implicit_disabled: lookup(DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV).is_some(),
                postgres_url: utf8(TEST_POSTGRES_URL_ENV),
                mysql_url: utf8(MYSQL_URL_ENV),
                libsql_url: utf8(LIBSQL_URL_ENV),
                libsql_auth_token: utf8(LIBSQL_AUTH_TOKEN_ENV),
                libsql_admin_url: utf8(LIBSQL_ADMIN_URL_ENV),
                libsql_admin_auth_header: utf8(LIBSQL_ADMIN_AUTH_HEADER_ENV),
            },
        }
    }

    /// Returns the crate root, or panics when the test runner did not set it.
    pub fn required_manifest_dir(&self) -> &Path {
        self.manifest_dir
            .as_deref()
            .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for storage tests")
    }
}

/// Fixture inputs for tests that run against an external provider.
///
/// A URL or credential that is not valid UTF-8 reads as absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalProviderFixtureInputs {
    /// The run must use explicit fixtures and fail when one is absent.
    pub required: bool,
    /// The run omits provider tests that have no explicit fixture.
    pub implicit_disabled: bool,
    pub postgres_url: Option<String>,
    pub mysql_url: Option<String>,
    pub libsql_url: Option<String>,
    pub libsql_auth_token: Option<String>,
    pub libsql_admin_url: Option<String>,
    pub libsql_admin_auth_header: Option<String>,
}

impl ExternalProviderFixtureInputs {
    /// Returns the fixture input that `name` identifies.
    ///
    /// # Panics
    ///
    /// Panics when `name` is not a provider fixture variable, so a fixture
    /// gate can never check a variable that this struct does not parse.
    pub fn value(&self, name: &str) -> Option<&str> {
        let value = match name {
            TEST_POSTGRES_URL_ENV => &self.postgres_url,
            MYSQL_URL_ENV => &self.mysql_url,
            LIBSQL_URL_ENV => &self.libsql_url,
            LIBSQL_AUTH_TOKEN_ENV => &self.libsql_auth_token,
            LIBSQL_ADMIN_URL_ENV => &self.libsql_admin_url,
            LIBSQL_ADMIN_AUTH_HEADER_ENV => &self.libsql_admin_auth_header,
            other => panic!("{other} is not an external provider fixture variable"),
        };
        value.as_deref()
    }

    /// Returns whether the fixture input that `name` identifies is non-empty.
    pub fn is_nonempty(&self, name: &str) -> bool {
        self.value(name).is_some_and(|value| !value.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn lookup(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let values: BTreeMap<String, OsString> = values
            .iter()
            .map(|(key, value)| ((*key).to_string(), OsString::from(value)))
            .collect();
        move |key| values.get(key).cloned()
    }

    #[test]
    fn empty_environment_yields_shipped_defaults() {
        assert_eq!(
            StorageConfig::from_lookup(lookup(&[])),
            StorageConfig::default()
        );
    }

    #[test]
    fn profile_flags_follow_variable_presence() {
        let config = StorageConfig::from_lookup(lookup(&[
            (CONTROL_PLANE_PROFILE_ENV, ""),
            (REDB_OPEN_PROFILE_ENV, "1"),
            (REDB_IO_PROFILE_ENV, "0"),
            (REDB_JOURNAL_PROFILE_ENV, "yes"),
            (SQLITE_OPEN_PROFILE_ENV, "1"),
            (PROFILE_ONLY_COLD_SAMPLES_ENV, "1"),
        ]));
        assert_eq!(
            config.profile,
            StorageProfileConfig {
                control_plane: true,
                redb_open: true,
                redb_io: true,
                redb_journal: true,
                sqlite_open: true,
                only_cold_samples: true,
            }
        );
    }

    #[test]
    fn sqlite_read_connection_cap_accepts_only_positive_integers() {
        let parsed = |value| {
            StorageConfig::from_lookup(lookup(&[(SQLITE_MAX_READ_CONNECTIONS_ENV, value)]))
                .sqlite_max_read_connections
        };
        assert_eq!(parsed("3"), Some(3));
        assert_eq!(parsed("0"), None);
        assert_eq!(parsed("-1"), None);
        assert_eq!(parsed("many"), None);

        let explicit = StorageConfig {
            sqlite_max_read_connections: Some(2),
            ..StorageConfig::default()
        };
        assert_eq!(explicit.sqlite_read_connection_limit(), 2);
        assert!(
            StorageConfig::default().sqlite_read_connection_limit() >= MIN_SQLITE_READ_CONNECTIONS
        );
    }

    #[test]
    fn test_harness_parses_typed_inputs() {
        let harness = StorageTestHarness::from_lookup(lookup(&[
            (MANIFEST_DIR_ENV, "/src/crates/nimbus-storage"),
            (STORAGE_CONFORMANCE_SEED_ENV, "not-a-seed"),
            (DISABLE_EXTERNAL_PROVIDER_FIXTURES_ENV, "1"),
            (MYSQL_URL_ENV, ""),
            (LIBSQL_URL_ENV, "http://127.0.0.1:8080"),
        ]));
        assert_eq!(
            harness.required_manifest_dir(),
            Path::new("/src/crates/nimbus-storage")
        );
        assert_eq!(harness.storage_conformance_seed, None);
        assert_eq!(harness.verify_case, None);
        let providers = &harness.external_providers;
        assert!(providers.implicit_disabled);
        assert!(!providers.required);
        assert!(!providers.is_nonempty(MYSQL_URL_ENV));
        assert!(!providers.is_nonempty(TEST_POSTGRES_URL_ENV));
        assert_eq!(
            providers.value(LIBSQL_URL_ENV),
            Some("http://127.0.0.1:8080")
        );

        let seeded =
            StorageTestHarness::from_lookup(lookup(&[(STORAGE_CONFORMANCE_SEED_ENV, "91")]));
        assert_eq!(seeded.storage_conformance_seed, Some(91));
    }

    #[test]
    #[should_panic(expected = "is not an external provider fixture variable")]
    fn fixture_input_lookup_rejects_unknown_variables() {
        ExternalProviderFixtureInputs::default().value("NIMBUS_UNKNOWN_URL");
    }

    #[test]
    fn cold_sample_scope_filters_paths_only_when_enabled() {
        let warm = Path::new("/tmp/point-read/tenant.redb");
        let cold = Path::new("/tmp/point-read-cold-sample/tenant.redb");
        let unscoped = StorageProfileConfig::default();
        assert!(unscoped.allows_path(warm));
        assert!(unscoped.allows_path(cold));

        let scoped = StorageProfileConfig {
            only_cold_samples: true,
            ..StorageProfileConfig::default()
        };
        assert!(!scoped.allows_path(warm));
        assert!(scoped.allows_path(cold));
    }
}

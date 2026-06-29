//! CLI enablement for the protocol adapter surfaces. Under D7 every
//! adapter serves by default: Firestore routes mount on the main HTTP
//! listener, MongoDB serves on its conventional port (27017), and DynamoDB
//! serves on its conventional port (8000), each with a `--no-*` opt-out.
//! A busy conventional port skips that listener with a warning unless the
//! operator asked for an explicit port — explicit ports fail loud instead.
//! Operator flags/env provide credentials when present; otherwise the
//! listeners use the generated wire-credential store under the control
//! data dir (D5), shared with `nimbus dev`. The same non-loopback opt-in
//! gate as the main listener applies.

use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use nimbus::{Error, TenantId};
use nimbus_server::{
    CloudflareConfig, DynamoDbConfig, FirebaseConfig, MongoDbAuthConfig, MongoDbConfig,
    MongoDbCredentialRegistry, ProjectTenantRegistry,
};

use crate::wire_credentials::{WireCredentials, load_or_generate};

use super::StartCommand;
use super::network_bind::ensure_host_opt_in;

pub(super) const MONGODB_USERNAME_ENV: &str = "NIMBUS_MONGODB_USERNAME";
pub(super) const MONGODB_PASSWORD_ENV: &str = "NIMBUS_MONGODB_PASSWORD";
/// Per-tenant MongoDB credential bindings (M9a). Comma-separated
/// `USERNAME:TENANT:PASSWORD` entries, mirroring the DynamoDB
/// [`DYNAMODB_ACCESS_KEYS_ENV`] convention. When set with at least one binding
/// the listener runs in bound mode (authentication decides the tenant), which a
/// non-loopback host requires; otherwise the listener stays in today's unbound,
/// loopback-only mode.
pub(super) const MONGODB_CREDENTIALS_ENV: &str = "NIMBUS_MONGODB_CREDENTIALS";
pub(super) const DYNAMODB_ACCESS_KEYS_ENV: &str = "NIMBUS_DYNAMODB_ACCESS_KEYS";
/// Per-tenant Firebase project->tenant bindings. Comma-separated
/// `PROJECT:TENANT` entries, mirroring the MongoDB [`MONGODB_CREDENTIALS_ENV`]
/// convention. When set the Firebase adapter resolves each request's project to
/// its bound tenant through this registry; when unset the adapter keeps the
/// default empty registry from [`FirebaseConfig::new`], which refuses every
/// request because no project maps to a tenant. A malformed entry is a hard
/// boot error, never a silent permissive default.
pub(super) const FIREBASE_PROJECTS_ENV: &str = "NIMBUS_FIREBASE_PROJECTS";

pub(crate) const MONGODB_CONVENTIONAL_PORT: u16 = 27017;
pub(crate) const DYNAMODB_CONVENTIONAL_PORT: u16 = 8000;

/// Tenant the generated wire-credential DynamoDB key binds to when the
/// operator provides no bindings. `nimbus dev` overrides this with its
/// auto-tenant by passing an explicit binding.
pub(super) const DEFAULT_DYNAMODB_TENANT: &str = "default";

/// Adapter configs resolved from the start command. `None` means the
/// surface does not serve this boot — opted out, or its conventional
/// port was busy with no explicit port requested.
#[derive(Debug)]
pub(crate) struct AdapterEnablement {
    pub(crate) firebase: Option<FirebaseConfig>,
    pub(crate) cloudflare: Option<CloudflareConfig>,
    pub(crate) mongodb: Option<MongoDbConfig>,
    pub(crate) dynamodb: Option<DynamoDbConfig>,
}

impl AdapterEnablement {
    /// Startup-summary lines describing which adapter surfaces serve this
    /// boot. Every surface reports — `off` is as informative as an address
    /// — and the lines carry bind addresses only, never credentials.
    pub(super) fn status_lines(&self) -> Vec<String> {
        let firestore = match &self.firebase {
            Some(_) => "mounted on the main listener".to_string(),
            None => "off".to_string(),
        };
        let cloudflare = match &self.cloudflare {
            Some(config) if config.bindings().is_empty() => {
                "mounted on the main listener".to_string()
            }
            Some(config) => format!(
                "mounted on the main listener ({} KV, {} DO, {} D1, {} R2 bindings)",
                config.bindings().kv_namespaces().len(),
                config.bindings().durable_objects().len(),
                config.bindings().d1_databases().len(),
                config.bindings().r2_buckets().len()
            ),
            None => "off".to_string(),
        };
        let mongodb = self
            .mongodb
            .as_ref()
            .map_or_else(|| "off".to_string(), |config| config.bind_addr.to_string());
        let dynamodb = self
            .dynamodb
            .as_ref()
            .map_or_else(|| "off".to_string(), |config| config.bind_addr.to_string());
        vec![
            format!("firestore routes:\t{firestore}"),
            format!("cloudflare routes:\t{cloudflare}"),
            format!("mongodb listener:\t{mongodb}"),
            format!("dynamodb listener:\t{dynamodb}"),
        ]
    }

    /// Mounts every resolved adapter surface onto the serve options.
    pub(crate) fn apply_to(
        self,
        mut options: nimbus_server::ServeOptions,
    ) -> nimbus_server::ServeOptions {
        if let Some(firebase) = self.firebase {
            options = options.with_firebase_config(firebase);
        }
        if let Some(cloudflare) = self.cloudflare {
            options = options.with_cloudflare(cloudflare);
        }
        if let Some(mongodb) = self.mongodb {
            options = options.with_mongodb(mongodb);
        }
        if let Some(dynamodb) = self.dynamodb {
            options = options.with_dynamodb(dynamodb);
        }
        options
    }
}

/// Lazy handle on the shared persisted wire-credential store (D5). Disk is
/// touched only when a default-on listener actually needs generated
/// credentials, so opted-out and operator-credentialed boots never create
/// the file.
struct CredentialStore<'a> {
    data_dir: &'a Path,
    cached: Option<WireCredentials>,
}

impl<'a> CredentialStore<'a> {
    fn new(data_dir: &'a Path) -> Self {
        Self {
            data_dir,
            cached: None,
        }
    }

    fn get(&mut self) -> Result<&WireCredentials, Error> {
        if self.cached.is_none() {
            self.cached = Some(load_or_generate(self.data_dir).map_err(store_error)?);
        }
        Ok(self
            .cached
            .as_ref()
            .expect("credentials cached by the branch above"))
    }
}

/// A malformed store is operator-fixable (the underlying error carries the
/// "delete it to regenerate" hint), so it maps to `InvalidInput`; anything
/// else is an environment failure.
fn store_error(error: std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::InvalidData {
        Error::InvalidInput(error.to_string())
    } else {
        Error::Internal(error.to_string())
    }
}

pub(super) fn resolve_adapter_enablement(
    command: &StartCommand,
    control_data_dir: &Path,
    app_dir: Option<&Path>,
) -> Result<AdapterEnablement, Error> {
    resolve_adapter_enablement_with_env_and_app_dir(
        command,
        control_data_dir,
        app_dir,
        |name| std::env::var(name).ok(),
        |port| std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
    )
}

#[cfg(test)]
pub(crate) fn resolve_adapter_enablement_with_env(
    command: &StartCommand,
    control_data_dir: &Path,
    env_lookup: impl Fn(&str) -> Option<String>,
    port_is_free: impl Fn(u16) -> bool,
) -> Result<AdapterEnablement, Error> {
    resolve_adapter_enablement_with_env_and_app_dir(
        command,
        control_data_dir,
        None,
        env_lookup,
        port_is_free,
    )
}

pub(crate) fn resolve_adapter_enablement_with_env_and_app_dir(
    command: &StartCommand,
    control_data_dir: &Path,
    app_dir: Option<&Path>,
    env_lookup: impl Fn(&str) -> Option<String>,
    port_is_free: impl Fn(u16) -> bool,
) -> Result<AdapterEnablement, Error> {
    let mut store = CredentialStore::new(control_data_dir);
    let mongodb = resolve_mongodb(command, &env_lookup, &port_is_free, &mut store)?;
    let dynamodb = resolve_dynamodb(command, &env_lookup, &port_is_free, &mut store)?;
    let cloudflare = resolve_cloudflare(command, app_dir, &mut store)?;
    Ok(AdapterEnablement {
        firebase: resolve_firebase(command, &env_lookup)?,
        cloudflare,
        mongodb,
        dynamodb,
    })
}

/// Resolve the Firebase adapter config, ingesting the project->tenant registry
/// from [`FIREBASE_PROJECTS_ENV`] when present.
///
/// When the surface is opted out this returns `Ok(None)`. When enabled the
/// adapter starts from [`FirebaseConfig::new`] (an empty, strict refuse-all
/// registry) and installs operator bindings only when the env is set, parsed by
/// the same [`ProjectTenantRegistry::from_operator_spec`] the registry uses
/// elsewhere. A malformed spec is a hard `InvalidInput` boot error, mirroring
/// the MongoDB credential ingestion; an unset env never falls back to a
/// permissive registry.
fn resolve_firebase(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<FirebaseConfig>, Error> {
    if !command.firestore {
        return Ok(None);
    }
    let mut config = FirebaseConfig::new();
    if let Some(auto_tenant) = &command.auto_tenant {
        let tenant = TenantId::new(auto_tenant)
            .map_err(|error| Error::InvalidInput(format!("invalid auto tenant: {error}")))?;
        config = config
            .with_emulator_token_verification_bypass()
            .with_project_registry(ProjectTenantRegistry::new().bind(auto_tenant, tenant));
    } else if let Some(raw) = env_lookup(FIREBASE_PROJECTS_ENV) {
        let registry = ProjectTenantRegistry::from_operator_spec(&raw)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        config = config.with_project_registry(registry);
    }
    Ok(Some(config))
}

fn resolve_cloudflare(
    command: &StartCommand,
    app_dir: Option<&Path>,
    store: &mut CredentialStore<'_>,
) -> Result<Option<CloudflareConfig>, Error> {
    if !command.cloudflare {
        return Ok(None);
    }
    // Cloudflare routes share the refuse_non_loopback_bind posture enforced by `ensure_host_opt_in`.
    ensure_host_opt_in(&command.host, command.allow_network)
        .map_err(|error| Error::InvalidInput(format!("--host for Cloudflare routes: {error}")))?;
    let mut config = match app_dir {
        Some(app_dir) => CloudflareConfig::from_app_dir(app_dir)
            .map_err(|error| Error::InvalidInput(error.to_string()))?,
        None => CloudflareConfig::default(),
    };
    let credentials = store.get()?;
    let tenant = TenantId::new(DEFAULT_DYNAMODB_TENANT)?;
    config = config.with_signed_access_key(
        credentials.dynamodb_access_key_id.clone(),
        tenant,
        credentials.dynamodb_secret_access_key.clone(),
    );
    Ok(Some(config))
}

fn resolve_mongodb(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
    port_is_free: &impl Fn(u16) -> bool,
    store: &mut CredentialStore<'_>,
) -> Result<Option<MongoDbConfig>, Error> {
    if !command.mongodb {
        if command.mongodb_port.is_some() || command.mongodb_username.is_some() {
            return Err(Error::InvalidInput(
                "--no-mongodb conflicts with --mongodb-port/--mongodb-username; \
                 drop the configuration flags or re-enable the listener"
                    .to_string(),
            ));
        }
        return Ok(None);
    }

    // Bound mode (M9a): per-tenant credential bindings make authentication —
    // not the wire `$db` — decide the tenant, so a bound listener may bind a
    // non-loopback host. Built from the SAME parser the acceptance test
    // exercises (`CredentialRegistry::from_operator_spec`), mirroring the
    // DynamoDB access-key ingestion. The env presence is the switch: with at
    // least one binding the listener runs bound; otherwise it falls through to
    // today's unbound, loopback-only path unchanged.
    if let Some(registry) = resolve_bound_mongodb_registry(command, env_lookup)? {
        let Some(port) = resolve_mongodb_port(command, port_is_free) else {
            return Ok(None);
        };
        // A bound listener may go non-loopback, gated by the same
        // `--allow-network` opt-in as the main and DynamoDB listeners.
        ensure_host_opt_in(&command.mongodb_host, command.allow_network)
            .map_err(|error| Error::InvalidInput(format!("--mongodb-host: {error}")))?;
        let bind_addr = adapter_bind_addr(&command.mongodb_host, port, "--mongodb-host")?;
        return Ok(Some(MongoDbConfig::bound(bind_addr, registry)));
    }

    // Unbound mode: the server hard-guards the MongoDB listener to loopback
    // (`guard_bind_address`): SCRAM runs over a plaintext channel under a single
    // tenant-agnostic credential, so the wire endpoint never binds a
    // network-reachable address. Validate here for a flag-shaped error instead
    // of a late bind failure.
    if !host_is_loopback_name(&command.mongodb_host) {
        return Err(Error::InvalidInput(format!(
            "--mongodb-host: the MongoDB listener is loopback-only (`{host}` refused); \
             supply per-tenant credentials ({MONGODB_CREDENTIALS_ENV}) for a non-loopback bind, \
             or front it with a TLS-terminating proxy for remote access",
            host = command.mongodb_host
        )));
    }
    let Some(port) = resolve_mongodb_port(command, port_is_free) else {
        return Ok(None);
    };
    let (username, password) = if command.mongodb_credentials_from_store {
        // `nimbus dev` advertises the store credentials in the app's
        // `.env.local`; ambient operator env must not desync the listener
        // from what that file carries.
        let credentials = store.get()?;
        (
            credentials.mongodb_username.clone(),
            credentials.mongodb_password.clone(),
        )
    } else {
        let username = command
            .mongodb_username
            .clone()
            .or_else(|| env_lookup(MONGODB_USERNAME_ENV));
        let password = env_lookup(MONGODB_PASSWORD_ENV);
        match (username, password) {
            (Some(username), Some(password)) => (username, password),
            (Some(_), None) => {
                return Err(Error::InvalidInput(format!(
                    "the MongoDB listener requires the {MONGODB_PASSWORD_ENV} environment \
                     variable (the password is env-only so it never appears in process \
                     listings)"
                )));
            }
            (None, Some(_)) => {
                return Err(Error::InvalidInput(format!(
                    "{MONGODB_PASSWORD_ENV} is set without a username; pass \
                     --mongodb-username (or set {MONGODB_USERNAME_ENV}) to use operator \
                     credentials, or unset the password to use the generated \
                     wire-credential store"
                )));
            }
            (None, None) => {
                let credentials = store.get()?;
                (
                    credentials.mongodb_username.clone(),
                    credentials.mongodb_password.clone(),
                )
            }
        }
    };
    let auth = MongoDbAuthConfig::try_new(username, password)
        .map_err(|error| Error::Internal(error.to_string()))?;
    let bind_addr = adapter_bind_addr(&command.mongodb_host, port, "--mongodb-host")?;
    Ok(Some(MongoDbConfig::new(bind_addr, auth)))
}

/// Resolve the MongoDB listener port, shared by bound and unbound modes.
///
/// An explicit `--mongodb-port` is always honored. Without one the conventional
/// port (27017) is used when free; when busy the listener is skipped (returns
/// `None`) with a warning — the same default-port behavior as DynamoDB.
fn resolve_mongodb_port(
    command: &StartCommand,
    port_is_free: &impl Fn(u16) -> bool,
) -> Option<u16> {
    match command.mongodb_port {
        Some(port) => Some(port),
        None => {
            if port_is_free(MONGODB_CONVENTIONAL_PORT) {
                Some(MONGODB_CONVENTIONAL_PORT)
            } else {
                tracing::warn!(
                    "MongoDB conventional port {MONGODB_CONVENTIONAL_PORT} is busy; \
                     skipping the default MongoDB listener — pass --mongodb-port to \
                     serve on another port"
                );
                None
            }
        }
    }
}

/// Parse the `NIMBUS_MONGODB_CREDENTIALS` env into a per-tenant credential
/// registry (bound mode), if present and non-empty.
///
/// Returns `Ok(None)` when the env is unset, holds no bindings (whitespace or
/// just separators), or the listener is in `nimbus dev` store-credential mode —
/// each is the signal to use today's unbound, loopback-only path. Returns
/// `Ok(Some(registry))` for one or more well-formed bindings. A malformed entry
/// or a reserved-tenant binding is a hard `InvalidInput` error surfaced cleanly
/// at boot, not a silent runtime auth failure. The parser is shared with the
/// acceptance test, so ingestion and the test agree by construction.
fn resolve_bound_mongodb_registry(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<MongoDbCredentialRegistry>, Error> {
    // `nimbus dev` pins the listener to its generated store credentials and
    // advertises them in `.env.local`; ambient NIMBUS_MONGODB_CREDENTIALS in the
    // developer's shell must not desync the two, exactly as ambient
    // NIMBUS_MONGODB_USERNAME/PASSWORD are ignored in store mode.
    if command.mongodb_credentials_from_store {
        return Ok(None);
    }
    let Some(raw) = env_lookup(MONGODB_CREDENTIALS_ENV) else {
        return Ok(None);
    };
    let registry = MongoDbCredentialRegistry::from_operator_spec(&raw)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    Ok((!registry.is_empty()).then_some(registry))
}

fn resolve_dynamodb(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
    port_is_free: &impl Fn(u16) -> bool,
    store: &mut CredentialStore<'_>,
) -> Result<Option<DynamoDbConfig>, Error> {
    if !command.dynamodb {
        if command.dynamodb_port.is_some() || !command.dynamodb_access_key.is_empty() {
            return Err(Error::InvalidInput(
                "--no-dynamodb conflicts with --dynamodb-port/--dynamodb-access-key; \
                 drop the configuration flags or re-enable the listener"
                    .to_string(),
            ));
        }
        return Ok(None);
    }
    ensure_host_opt_in(&command.dynamodb_host, command.allow_network)
        .map_err(|error| Error::InvalidInput(format!("--dynamodb-host: {error}")))?;
    let port = match command.dynamodb_port {
        Some(port) => port,
        None => {
            if !port_is_free(DYNAMODB_CONVENTIONAL_PORT) {
                tracing::warn!(
                    "DynamoDB conventional port {DYNAMODB_CONVENTIONAL_PORT} is busy; \
                     skipping the default DynamoDB listener — pass --dynamodb-port to \
                     serve on another port"
                );
                return Ok(None);
            }
            DYNAMODB_CONVENTIONAL_PORT
        }
    };
    let raw_bindings: Vec<String> = if command.dynamodb_access_key.is_empty() {
        env_lookup(DYNAMODB_ACCESS_KEYS_ENV)
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        command.dynamodb_access_key.clone()
    };
    let mut config = DynamoDbConfig::new(port).with_bind_addr(adapter_bind_addr(
        &command.dynamodb_host,
        port,
        "--dynamodb-host",
    )?);
    if raw_bindings.is_empty() {
        // Every request still authenticates: the generated store key binds
        // to the `default` tenant, so an unconfigured boot serves signed
        // requests instead of rejecting everything.
        let credentials = store.get()?;
        let tenant = TenantId::new(DEFAULT_DYNAMODB_TENANT)?;
        config = config.with_signed_access_key(
            credentials.dynamodb_access_key_id.clone(),
            tenant,
            credentials.dynamodb_secret_access_key.clone(),
        );
    } else {
        for binding in &raw_bindings {
            let (key_id, secret, tenant) = parse_access_key_binding(binding)?;
            config = config.with_signed_access_key(key_id, tenant, secret);
        }
    }
    Ok(Some(config))
}

/// Parse `ACCESS_KEY_ID:SECRET:TENANT`. AWS secret access keys use the
/// base64 alphabet (no `:`), so a three-way split is unambiguous.
fn parse_access_key_binding(binding: &str) -> Result<(String, String, TenantId), Error> {
    let mut parts = binding.splitn(3, ':');
    let (Some(key_id), Some(secret), Some(tenant)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::InvalidInput(format!(
            "invalid DynamoDB access-key binding `{binding}`: expected ACCESS_KEY_ID:SECRET:TENANT"
        )));
    };
    if key_id.is_empty() || secret.is_empty() || tenant.is_empty() {
        return Err(Error::InvalidInput(format!(
            "invalid DynamoDB access-key binding `{binding}`: every segment must be non-empty"
        )));
    }
    let tenant = TenantId::new(tenant)?;
    Ok((key_id.to_string(), secret.to_string(), tenant))
}

fn host_is_loopback_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

fn adapter_bind_addr(host: &str, port: u16, flag: &str) -> Result<SocketAddr, Error> {
    let ip: IpAddr = if host.eq_ignore_ascii_case("localhost") {
        IpAddr::from([127, 0, 0, 1])
    } else {
        host.parse().map_err(|_| {
            Error::InvalidInput(format!("{flag}: `{host}` is not a valid IP address"))
        })?
    };
    Ok(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire_credentials::wire_credentials_path;

    fn base_command() -> StartCommand {
        StartCommand::default()
    }

    /// Resolve with an always-free port probe so default-on tests stay
    /// deterministic on machines running a real `mongod` or DynamoDB Local.
    fn resolve(
        command: &StartCommand,
        data_dir: &Path,
        env_lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<AdapterEnablement, Error> {
        resolve_adapter_enablement_with_env(command, data_dir, env_lookup, |_| true)
    }

    #[test]
    fn start_serves_all_adapters_by_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve(&base_command(), temp.path(), |_| None)
            .expect("the default command must resolve every surface");

        let firebase = resolved
            .firebase
            .as_ref()
            .expect("firestore routes default on");
        assert!(
            !firebase.allows_emulator_token_verification_bypass(),
            "plain start must not enable the dev-only Firebase emulator bypass"
        );
        assert!(
            firebase.project_registry().resolve("demo").is_err(),
            "plain start must keep Firestore strict until projects are explicitly bound"
        );
        assert!(
            resolved.cloudflare.is_some(),
            "cloudflare routes default on"
        );

        let mongodb = resolved.mongodb.expect("mongodb listener defaults on");
        assert_eq!(
            mongodb.bind_addr,
            format!("127.0.0.1:{MONGODB_CONVENTIONAL_PORT}")
                .parse()
                .unwrap()
        );

        let dynamodb = resolved.dynamodb.expect("dynamodb listener defaults on");
        assert_eq!(
            dynamodb.bind_addr,
            format!("127.0.0.1:{DYNAMODB_CONVENTIONAL_PORT}")
                .parse()
                .unwrap()
        );

        // Credentials came from the persisted store: the file now exists,
        // its MongoDB user backs the listener auth, and its DynamoDB key
        // is bound (to the `default` tenant).
        assert!(
            wire_credentials_path(temp.path()).exists(),
            "a default-on boot must persist the generated credentials"
        );
        let store = load_or_generate(temp.path()).expect("reload the persisted store");
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            store.mongodb_username
        );
        assert!(
            dynamodb
                .access_keys
                .binding(&store.dynamodb_access_key_id)
                .is_ok(),
            "the store access key must authenticate"
        );
        assert!(
            resolved
                .cloudflare
                .as_ref()
                .expect("cloudflare should resolve")
                .access_keys()
                .binding(&store.dynamodb_access_key_id)
                .is_ok(),
            "the store access key must authenticate the Cloudflare surface"
        );
    }

    #[test]
    fn dev_auto_tenant_enables_loopback_firebase_project_binding() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.auto_tenant = Some("dev-project".to_string());
        let resolved = resolve(&command, temp.path(), |name| {
            (name == FIREBASE_PROJECTS_ENV).then(|| "other:other".to_string())
        })
        .expect("dev auto-tenant Firestore config should resolve");
        let firebase = resolved.firebase.expect("firestore routes stay mounted");

        assert!(
            firebase.allows_emulator_token_verification_bypass(),
            "dev auto-tenant mode must accept Firebase emulator mock tokens on loopback"
        );
        assert_eq!(
            firebase
                .project_registry()
                .resolve("dev-project")
                .expect("dev project should resolve"),
            TenantId::new("dev-project").expect("tenant id should parse")
        );
        assert!(
            firebase.project_registry().resolve("other").is_err(),
            "ambient project bindings must not desync nimbus dev from its auto tenant"
        );
    }

    #[test]
    fn status_lines_report_every_surface_without_credentials() {
        let temp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve(&base_command(), temp.path(), |_| None)
            .expect("the default command must resolve every surface");
        let lines = resolved.status_lines();
        assert_eq!(
            lines,
            vec![
                "firestore routes:\tmounted on the main listener".to_string(),
                "cloudflare routes:\tmounted on the main listener".to_string(),
                format!("mongodb listener:\t127.0.0.1:{MONGODB_CONVENTIONAL_PORT}"),
                format!("dynamodb listener:\t127.0.0.1:{DYNAMODB_CONVENTIONAL_PORT}"),
            ]
        );
        let store = load_or_generate(temp.path()).expect("reload the persisted store");
        for line in &lines {
            assert!(
                !line.contains(&store.mongodb_password)
                    && !line.contains(&store.dynamodb_access_key_id)
                    && !line.contains(&store.dynamodb_secret_access_key),
                "status lines must never carry credential material: {line}"
            );
        }

        let mut command = base_command();
        command.firestore = false;
        command.cloudflare = false;
        command.mongodb = false;
        command.dynamodb = false;
        let opted_out = resolve(&command, temp.path(), |_| None)
            .expect("a fully opted-out command should resolve");
        assert_eq!(
            opted_out.status_lines(),
            vec![
                "firestore routes:\toff".to_string(),
                "cloudflare routes:\toff".to_string(),
                "mongodb listener:\toff".to_string(),
                "dynamodb listener:\toff".to_string(),
            ]
        );
    }

    #[test]
    fn adapter_opt_out_flags_disable_surfaces() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.firestore = false;
        command.cloudflare = false;
        command.mongodb = false;
        command.dynamodb = false;
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("a fully opted-out command should resolve");
        assert!(resolved.firebase.is_none());
        assert!(resolved.cloudflare.is_none());
        assert!(resolved.mongodb.is_none());
        assert!(resolved.dynamodb.is_none());
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "an opted-out boot must never touch the credential store"
        );

        // Opting out while also configuring the surface is a conflict, not
        // a silent ignore.
        let mut command = base_command();
        command.mongodb = false;
        command.mongodb_port = Some(27017);
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("--no-mongodb with --mongodb-port must conflict");
        assert!(error.to_string().contains("--no-mongodb"));

        let mut command = base_command();
        command.dynamodb = false;
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("--no-dynamodb with --dynamodb-access-key must conflict");
        assert!(error.to_string().contains("--no-dynamodb"));
    }

    #[test]
    fn cloudflare_reads_wrangler_bindings_from_app_dir() {
        let data_dir = tempfile::tempdir().expect("data tempdir");
        let app_dir = tempfile::tempdir().expect("app tempdir");
        std::fs::write(
            app_dir.path().join("wrangler.jsonc"),
            r#"
            {
              "kv_namespaces": [
                { "binding": "CACHE", "id": "kv-prod", },
              ],
              "durable_objects": {
                "bindings": [
                  { "name": "COUNTERS", "class_name": "Counter", },
                ],
              },
            }
            "#,
        )
        .expect("wrangler config should write");

        let resolved = resolve_adapter_enablement_with_env_and_app_dir(
            &base_command(),
            data_dir.path(),
            Some(app_dir.path()),
            |_| None,
            |_| true,
        )
        .expect("wrangler-backed Cloudflare config should resolve");
        let cloudflare = resolved
            .cloudflare
            .as_ref()
            .expect("cloudflare should be enabled");

        assert_eq!(cloudflare.bindings().kv_namespaces()[0].binding, "CACHE");
        assert_eq!(cloudflare.bindings().durable_objects()[0].name, "COUNTERS");
        assert_eq!(
            resolved.status_lines()[1],
            "cloudflare routes:\tmounted on the main listener (1 KV, 1 DO, 0 D1, 0 R2 bindings)"
        );
    }

    #[test]
    fn busy_conventional_ports_skip_default_listeners() {
        let temp = tempfile::tempdir().expect("tempdir");
        let resolved =
            resolve_adapter_enablement_with_env(&base_command(), temp.path(), |_| None, |_| false)
                .expect("busy conventional ports must not fail boot");
        assert!(resolved.firebase.is_some(), "firestore routes are portless");
        assert!(resolved.mongodb.is_none(), "busy 27017 skips the listener");
        assert!(resolved.dynamodb.is_none(), "busy 8000 skips the listener");

        // An explicit port never probes: the operator asked for it, so a
        // conflict surfaces as a loud bind failure at serve time instead.
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.dynamodb_port = Some(8000);
        let resolved =
            resolve_adapter_enablement_with_env(&command, temp.path(), |_| None, |_| false)
                .expect("explicit ports must resolve regardless of the probe");
        assert!(resolved.mongodb.is_some());
        assert!(resolved.dynamodb.is_some());
    }

    #[test]
    fn mongodb_listener_requires_scram_credentials() {
        // Even with nothing configured, the listener never serves
        // unauthenticated: the generated store provides the SCRAM user.
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("store credentials should back the listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        let store = load_or_generate(temp.path()).expect("reload the persisted store");
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            store.mongodb_username
        );

        // Operator env credentials take precedence over the store.
        let resolved = resolve(&command, temp.path(), |name| match name {
            MONGODB_USERNAME_ENV => Some("ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("secret".to_string()),
            _ => None,
        })
        .expect("env credentials should enable the listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert_eq!(mongodb.bind_addr, "127.0.0.1:27017".parse().unwrap());
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            "ops"
        );
    }

    #[test]
    fn mongodb_password_is_env_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_username = Some("ops".to_string());
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("a username without the env password must be rejected");
        assert!(error.to_string().contains(MONGODB_PASSWORD_ENV));

        // The reverse half-pair is rejected too: a password with no
        // username is ambiguous between operator intent and leftovers.
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_username = None;
        let error = resolve(&command, temp.path(), |name| {
            (name == MONGODB_PASSWORD_ENV).then(|| "secret".to_string())
        })
        .expect_err("a password without a username must be rejected");
        assert!(error.to_string().contains("without a username"));
    }

    #[test]
    fn mongodb_credentials_without_port_apply_to_the_default_listener() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        // Opt DynamoDB out so its store-backed default key can't create the
        // file this test asserts stays absent. Cloudflare also uses the
        // generated store credential by default, so it is out of this
        // MongoDB-only assertion too.
        command.dynamodb = false;
        command.cloudflare = false;
        command.mongodb_username = Some("ops".to_string());
        let resolved = resolve(&command, temp.path(), |name| {
            (name == MONGODB_PASSWORD_ENV).then(|| "secret".to_string())
        })
        .expect("operator credentials should apply to the default listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert_eq!(mongodb.bind_addr.port(), MONGODB_CONVENTIONAL_PORT);
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            "ops"
        );
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "operator credentials must not touch the store"
        );
    }

    #[test]
    fn dev_store_only_credentials_ignore_ambient_operator_env() {
        // `nimbus dev` pins the listener to the store credentials its
        // `.env.local` advertises; ambient NIMBUS_MONGODB_* in the
        // developer's shell must not desync the two.
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_credentials_from_store = true;
        let resolved = resolve(&command, temp.path(), |name| match name {
            MONGODB_USERNAME_ENV => Some("ambient-ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("ambient-secret".to_string()),
            _ => None,
        })
        .expect("store-only mode should resolve");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        let store = load_or_generate(temp.path()).expect("reload the persisted store");
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            store.mongodb_username
        );
        assert_ne!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            "ambient-ops"
        );
    }

    #[test]
    fn dynamodb_listener_parses_access_key_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.cloudflare = false;
        command.mongodb = false;
        command.dynamodb_port = Some(8000);
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:sEcr3t/Key+=:demo".to_string()];
        let resolved =
            resolve(&command, temp.path(), |_| None).expect("valid binding should resolve");
        let dynamodb = resolved.dynamodb.expect("dynamodb config should resolve");
        assert_eq!(dynamodb.bind_addr, "127.0.0.1:8000".parse().unwrap());
        assert!(dynamodb.access_keys.binding("AKIDEXAMPLE").is_ok());
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "operator bindings must not touch the store"
        );
    }

    #[test]
    fn dynamodb_env_bindings_apply_without_flags() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        let resolved = resolve(&command, temp.path(), |name| {
            (name == DYNAMODB_ACCESS_KEYS_ENV)
                .then(|| "AKIDONE:s1:alpha, AKIDTWO:s2:beta".to_string())
        })
        .expect("env bindings should resolve");
        let dynamodb = resolved.dynamodb.expect("dynamodb config should resolve");
        assert!(dynamodb.access_keys.binding("AKIDONE").is_ok());
        assert!(dynamodb.access_keys.binding("AKIDTWO").is_ok());
    }

    #[test]
    fn dynamodb_bindings_without_port_apply_to_the_default_listener() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("bindings should apply to the default listener");
        let dynamodb = resolved.dynamodb.expect("dynamodb config should resolve");
        assert_eq!(dynamodb.bind_addr.port(), DYNAMODB_CONVENTIONAL_PORT);
        assert!(dynamodb.access_keys.binding("AKIDEXAMPLE").is_ok());
    }

    #[test]
    fn dynamodb_rejects_malformed_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        command.dynamodb_access_key = vec!["only-two:parts".to_string()];
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("malformed binding must be rejected");
        assert!(error.to_string().contains("ACCESS_KEY_ID:SECRET:TENANT"));
    }

    #[test]
    fn mongodb_host_is_loopback_only_even_with_allow_network() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_host = "0.0.0.0".to_string();
        command.allow_network = true;
        let error = resolve(&command, temp.path(), |name| match name {
            MONGODB_USERNAME_ENV => Some("ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("secret".to_string()),
            _ => None,
        })
        .expect_err("the MongoDB listener must refuse non-loopback hosts outright");
        let message = error.to_string();
        assert!(
            message.contains("loopback-only") && message.contains("proxy"),
            "refusal should explain the posture, got: {message}"
        );
    }

    #[test]
    fn mongodb_bound_credentials_enable_a_tenant_bound_listener() {
        // M9a: NIMBUS_MONGODB_CREDENTIALS ingests per-tenant bindings, so the
        // listener runs in bound mode (authentication decides the tenant).
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        // Opt DynamoDB out so its store-backed default key can't create the
        // file this test asserts stays absent. Cloudflare also uses the
        // generated store credential by default, so it is out of this
        // MongoDB-only assertion too.
        command.dynamodb = false;
        command.cloudflare = false;
        let resolved = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV)
                .then(|| "user-a:tenant-a:secret-a,user-b:tenant-b:secret-b".to_string())
        })
        .expect("bound credentials should enable the listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert!(
            mongodb.is_tenant_bound(),
            "operator credentials must produce a bound listener"
        );
        assert!(
            mongodb.auth_config().is_none(),
            "bound mode exposes no single tenant-agnostic credential"
        );
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "operator credentials must not touch the store"
        );
    }

    #[test]
    fn mongodb_bound_credentials_admit_a_non_loopback_host_with_opt_in() {
        // The loopback-only check is relaxed in bound mode, gated by the same
        // --allow-network opt-in as the main and DynamoDB listeners.
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_host = "0.0.0.0".to_string();

        // Without --allow-network the non-loopback bound host is still refused.
        let error = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV).then(|| "user-a:tenant-a:secret-a".to_string())
        })
        .expect_err("a non-loopback bound host without --allow-network must be refused");
        assert!(
            error.to_string().contains("--allow-network"),
            "refusal must name the opt-in flag, got: {error}"
        );

        // With --allow-network it binds the non-loopback address.
        command.allow_network = true;
        let resolved = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV).then(|| "user-a:tenant-a:secret-a".to_string())
        })
        .expect("--allow-network should admit the non-loopback bound host");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert!(mongodb.is_tenant_bound());
        assert_eq!(mongodb.bind_addr, "0.0.0.0:27017".parse().unwrap());
    }

    #[test]
    fn mongodb_malformed_bound_credentials_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let error = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV).then(|| "only-two:parts".to_string())
        })
        .expect_err("a malformed credential binding must be rejected");
        assert!(
            error.to_string().contains("USERNAME:TENANT:PASSWORD"),
            "refusal must name the expected format, got: {error}"
        );
    }

    #[test]
    fn mongodb_reserved_tenant_bound_credentials_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let error = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV)
                .then(|| "user-evil:_nimbus_internal:secret".to_string())
        })
        .expect_err("binding a reserved Nimbus-internal tenant must be refused");
        assert!(
            error.to_string().contains("reserved"),
            "refusal must explain the reserved-tenant rejection, got: {error}"
        );
    }

    #[test]
    fn mongodb_empty_credentials_env_keeps_the_unbound_loopback_path() {
        // An env present but empty (no bindings) is not "bound mode": today's
        // unbound, loopback-only behavior holds and a non-loopback host is
        // refused with the loopback-only message.
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let resolved = resolve(&command, temp.path(), |name| match name {
            MONGODB_CREDENTIALS_ENV => Some("  ,  ".to_string()),
            _ => None,
        })
        .expect("an empty credentials env should fall back to the unbound path");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert!(
            !mongodb.is_tenant_bound(),
            "an empty credentials env must not enable bound mode"
        );

        command.mongodb_host = "0.0.0.0".to_string();
        command.allow_network = true;
        let error = resolve(&command, temp.path(), |name| match name {
            MONGODB_CREDENTIALS_ENV => Some("".to_string()),
            MONGODB_USERNAME_ENV => Some("ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("secret".to_string()),
            _ => None,
        })
        .expect_err("the unbound path must still refuse a non-loopback host");
        assert!(error.to_string().contains("loopback-only"));
    }

    #[test]
    fn dynamodb_listener_respects_the_network_opt_in_gate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        command.dynamodb_host = "0.0.0.0".to_string();
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("non-loopback dynamodb host without --allow-network must be refused");
        assert!(error.to_string().contains("--allow-network"));

        command.allow_network = true;
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("--allow-network should admit the non-loopback dynamodb host");
        assert_eq!(
            resolved
                .dynamodb
                .expect("dynamodb should resolve")
                .bind_addr,
            "0.0.0.0:8000".parse().unwrap()
        );
    }
}

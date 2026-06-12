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
use nimbus_server::{DynamoDbConfig, FirebaseConfig, MongoDbAuthConfig, MongoDbConfig};

use crate::wire_credentials::{WireCredentials, load_or_generate};

use super::StartCommand;
use super::network_bind::ensure_host_opt_in;

pub(super) const MONGODB_USERNAME_ENV: &str = "NIMBUS_MONGODB_USERNAME";
pub(super) const MONGODB_PASSWORD_ENV: &str = "NIMBUS_MONGODB_PASSWORD";
pub(super) const DYNAMODB_ACCESS_KEYS_ENV: &str = "NIMBUS_DYNAMODB_ACCESS_KEYS";

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
    pub(crate) mongodb: Option<MongoDbConfig>,
    pub(crate) dynamodb: Option<DynamoDbConfig>,
}

impl AdapterEnablement {
    /// Mounts every resolved adapter surface onto the serve options.
    pub(crate) fn apply_to(
        self,
        mut options: nimbus_server::ServeOptions,
    ) -> nimbus_server::ServeOptions {
        if let Some(firebase) = self.firebase {
            options = options.with_firebase_config(firebase);
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
) -> Result<AdapterEnablement, Error> {
    resolve_adapter_enablement_with_env(
        command,
        control_data_dir,
        |name| std::env::var(name).ok(),
        |port| std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
    )
}

pub(crate) fn resolve_adapter_enablement_with_env(
    command: &StartCommand,
    control_data_dir: &Path,
    env_lookup: impl Fn(&str) -> Option<String>,
    port_is_free: impl Fn(u16) -> bool,
) -> Result<AdapterEnablement, Error> {
    let mut store = CredentialStore::new(control_data_dir);
    Ok(AdapterEnablement {
        firebase: command.firestore.then(FirebaseConfig::new),
        mongodb: resolve_mongodb(command, &env_lookup, &port_is_free, &mut store)?,
        dynamodb: resolve_dynamodb(command, &env_lookup, &port_is_free, &mut store)?,
    })
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
    // The server hard-guards the MongoDB listener to loopback
    // (`guard_listener_is_loopback_only`): SCRAM runs over a plaintext
    // channel, so the wire endpoint never binds a network-reachable
    // address. Validate here for a flag-shaped error instead of a late
    // bind failure.
    if !host_is_loopback_name(&command.mongodb_host) {
        return Err(Error::InvalidInput(format!(
            "--mongodb-host: the MongoDB listener is loopback-only (`{host}` refused); \
             front it with a TLS-terminating proxy for remote access",
            host = command.mongodb_host
        )));
    }
    let port = match command.mongodb_port {
        Some(port) => port,
        None => {
            if !port_is_free(MONGODB_CONVENTIONAL_PORT) {
                tracing::warn!(
                    "MongoDB conventional port {MONGODB_CONVENTIONAL_PORT} is busy; \
                     skipping the default MongoDB listener — pass --mongodb-port to \
                     serve on another port"
                );
                return Ok(None);
            }
            MONGODB_CONVENTIONAL_PORT
        }
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

        assert!(resolved.firebase.is_some(), "firestore routes default on");

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
        assert_eq!(mongodb.auth.username, store.mongodb_username);
        assert!(
            dynamodb
                .access_keys
                .binding(&store.dynamodb_access_key_id)
                .is_ok(),
            "the store access key must authenticate"
        );
    }

    #[test]
    fn adapter_opt_out_flags_disable_surfaces() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.firestore = false;
        command.mongodb = false;
        command.dynamodb = false;
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("a fully opted-out command should resolve");
        assert!(resolved.firebase.is_none());
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
        assert_eq!(mongodb.auth.username, store.mongodb_username);

        // Operator env credentials take precedence over the store.
        let resolved = resolve(&command, temp.path(), |name| match name {
            MONGODB_USERNAME_ENV => Some("ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("secret".to_string()),
            _ => None,
        })
        .expect("env credentials should enable the listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert_eq!(mongodb.bind_addr, "127.0.0.1:27017".parse().unwrap());
        assert_eq!(mongodb.auth.username, "ops");
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
        // file this test asserts stays absent.
        command.dynamodb = false;
        command.mongodb_username = Some("ops".to_string());
        let resolved = resolve(&command, temp.path(), |name| {
            (name == MONGODB_PASSWORD_ENV).then(|| "secret".to_string())
        })
        .expect("operator credentials should apply to the default listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert_eq!(mongodb.bind_addr.port(), MONGODB_CONVENTIONAL_PORT);
        assert_eq!(mongodb.auth.username, "ops");
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
        assert_eq!(mongodb.auth.username, store.mongodb_username);
        assert_ne!(mongodb.auth.username, "ambient-ops");
    }

    #[test]
    fn dynamodb_listener_parses_access_key_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
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

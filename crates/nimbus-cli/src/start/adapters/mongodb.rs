use std::net::IpAddr;

use nimbus::Error;
use nimbus_server::{MongoDbAuthConfig, MongoDbConfig, MongoDbCredentialRegistry};

use crate::start::StartCommand;
use crate::start::network_bind::ensure_host_opt_in;

use super::{CredentialStore, adapter_bind_addr};

pub(super) const MONGODB_USERNAME_ENV: &str = "NIMBUS_MONGODB_USERNAME";
pub(super) const MONGODB_PASSWORD_ENV: &str = "NIMBUS_MONGODB_PASSWORD";
/// Per-tenant MongoDB credential bindings (M9a). Comma-separated
/// `USERNAME:TENANT:PASSWORD` entries, mirroring the DynamoDB
/// `DYNAMODB_ACCESS_KEYS_ENV` convention. When set with at least one binding
/// the listener runs in bound mode (authentication decides the tenant), which a
/// non-loopback host requires; otherwise the listener stays in today's unbound,
/// loopback-only mode.
pub(super) const MONGODB_CREDENTIALS_ENV: &str = "NIMBUS_MONGODB_CREDENTIALS";

pub(crate) const MONGODB_CONVENTIONAL_PORT: u16 = 27017;

pub(super) fn resolve_mongodb(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
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
        let port = resolve_mongodb_port(command);
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
    let port = resolve_mongodb_port(command);
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
/// An explicit `--mongodb-port` is honored; otherwise desired state uses the
/// conventional port. The shared listener authority and real provider bind,
/// not configuration resolution, decide whether that address is available.
fn resolve_mongodb_port(command: &StartCommand) -> u16 {
    command.mongodb_port.unwrap_or(MONGODB_CONVENTIONAL_PORT)
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

fn host_is_loopback_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

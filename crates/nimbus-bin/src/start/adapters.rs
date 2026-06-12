//! LR6: CLI enablement for the protocol adapters that were previously
//! embedding-API-only. Resolves `nimbus start` flags (plus their env
//! fallbacks) into the `ServeOptions` adapter configs, applying the same
//! non-loopback opt-in gate as the main listener.

use std::net::{IpAddr, SocketAddr};

use nimbus::{Error, TenantId};
use nimbus_server::{DynamoDbConfig, FirebaseConfig, MongoDbAuthConfig, MongoDbConfig};

use super::StartCommand;
use super::network_bind::ensure_host_opt_in;

pub(super) const MONGODB_USERNAME_ENV: &str = "NIMBUS_MONGODB_USERNAME";
pub(super) const MONGODB_PASSWORD_ENV: &str = "NIMBUS_MONGODB_PASSWORD";
pub(super) const DYNAMODB_ACCESS_KEYS_ENV: &str = "NIMBUS_DYNAMODB_ACCESS_KEYS";

/// Adapter configs resolved from the start command. `None` means the
/// surface stays off — the default.
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

pub(super) fn resolve_adapter_enablement(
    command: &StartCommand,
) -> Result<AdapterEnablement, Error> {
    resolve_adapter_enablement_with_env(command, |name| std::env::var(name).ok())
}

pub(crate) fn resolve_adapter_enablement_with_env(
    command: &StartCommand,
    env_lookup: impl Fn(&str) -> Option<String>,
) -> Result<AdapterEnablement, Error> {
    Ok(AdapterEnablement {
        firebase: command.firestore.then(FirebaseConfig::new),
        mongodb: resolve_mongodb(command, &env_lookup)?,
        dynamodb: resolve_dynamodb(command, &env_lookup)?,
    })
}

fn resolve_mongodb(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<MongoDbConfig>, Error> {
    let Some(port) = command.mongodb_port else {
        if command.mongodb_username.is_some() {
            return Err(Error::InvalidInput(
                "--mongodb-username requires --mongodb-port to enable the MongoDB listener"
                    .to_string(),
            ));
        }
        return Ok(None);
    };
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
    let username = command
        .mongodb_username
        .clone()
        .or_else(|| env_lookup(MONGODB_USERNAME_ENV))
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "the MongoDB listener requires SCRAM credentials: pass --mongodb-username \
                 (or set {MONGODB_USERNAME_ENV}) and set {MONGODB_PASSWORD_ENV}"
            ))
        })?;
    let password = env_lookup(MONGODB_PASSWORD_ENV).ok_or_else(|| {
        Error::InvalidInput(format!(
            "the MongoDB listener requires the {MONGODB_PASSWORD_ENV} environment variable \
             (the password is env-only so it never appears in process listings)"
        ))
    })?;
    let auth = MongoDbAuthConfig::try_new(username, password)
        .map_err(|error| Error::Internal(error.to_string()))?;
    let bind_addr = adapter_bind_addr(&command.mongodb_host, port, "--mongodb-host")?;
    Ok(Some(MongoDbConfig::new(bind_addr, auth)))
}

fn resolve_dynamodb(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<DynamoDbConfig>, Error> {
    let Some(port) = command.dynamodb_port else {
        if !command.dynamodb_access_key.is_empty() {
            return Err(Error::InvalidInput(
                "--dynamodb-access-key requires --dynamodb-port to enable the DynamoDB listener"
                    .to_string(),
            ));
        }
        return Ok(None);
    };
    ensure_host_opt_in(&command.dynamodb_host, command.allow_network)
        .map_err(|error| Error::InvalidInput(format!("--dynamodb-host: {error}")))?;
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
    for binding in &raw_bindings {
        let (key_id, secret, tenant) = parse_access_key_binding(binding)?;
        config = config.with_signed_access_key(key_id, tenant, secret);
    }
    if raw_bindings.is_empty() {
        tracing::warn!(
            "DynamoDB listener enabled with no access-key bindings; every request will be \
             rejected — pass --dynamodb-access-key or set {DYNAMODB_ACCESS_KEYS_ENV}"
        );
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

    fn base_command() -> StartCommand {
        StartCommand::default()
    }

    #[test]
    fn adapters_stay_off_by_default() {
        let resolved = resolve_adapter_enablement_with_env(&base_command(), |_| None)
            .expect("default command should resolve");
        assert!(resolved.firebase.is_none());
        assert!(resolved.mongodb.is_none());
        assert!(resolved.dynamodb.is_none());
    }

    #[test]
    fn firestore_flag_mounts_firebase_config() {
        let mut command = base_command();
        command.firestore = true;
        let resolved = resolve_adapter_enablement_with_env(&command, |_| None)
            .expect("firestore-only command should resolve");
        assert!(resolved.firebase.is_some());
    }

    #[test]
    fn mongodb_listener_requires_scram_credentials() {
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let error = resolve_adapter_enablement_with_env(&command, |_| None)
            .expect_err("missing credentials must be rejected");
        let message = error.to_string();
        assert!(
            message.contains("--mongodb-username") && message.contains(MONGODB_PASSWORD_ENV),
            "error should name both credential sources, got: {message}"
        );

        let resolved = resolve_adapter_enablement_with_env(&command, |name| match name {
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
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_username = Some("ops".to_string());
        let error = resolve_adapter_enablement_with_env(&command, |_| None)
            .expect_err("missing env password must be rejected");
        assert!(error.to_string().contains(MONGODB_PASSWORD_ENV));
    }

    #[test]
    fn mongodb_username_without_port_is_rejected() {
        let mut command = base_command();
        command.mongodb_username = Some("ops".to_string());
        let error = resolve_adapter_enablement_with_env(&command, |_| None)
            .expect_err("username without port must be rejected");
        assert!(error.to_string().contains("--mongodb-port"));
    }

    #[test]
    fn dynamodb_listener_parses_access_key_bindings() {
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:sEcr3t/Key+=:demo".to_string()];
        let resolved = resolve_adapter_enablement_with_env(&command, |_| None)
            .expect("valid binding should resolve");
        let dynamodb = resolved.dynamodb.expect("dynamodb config should resolve");
        assert_eq!(dynamodb.bind_addr, "127.0.0.1:8000".parse().unwrap());
        assert!(dynamodb.access_keys.binding("AKIDEXAMPLE").is_ok());
    }

    #[test]
    fn dynamodb_env_bindings_apply_without_flags() {
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        let resolved = resolve_adapter_enablement_with_env(&command, |name| {
            (name == DYNAMODB_ACCESS_KEYS_ENV)
                .then(|| "AKIDONE:s1:alpha, AKIDTWO:s2:beta".to_string())
        })
        .expect("env bindings should resolve");
        let dynamodb = resolved.dynamodb.expect("dynamodb config should resolve");
        assert!(dynamodb.access_keys.binding("AKIDONE").is_ok());
        assert!(dynamodb.access_keys.binding("AKIDTWO").is_ok());
    }

    #[test]
    fn dynamodb_rejects_malformed_bindings_and_keys_without_port() {
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        command.dynamodb_access_key = vec!["only-two:parts".to_string()];
        let error = resolve_adapter_enablement_with_env(&command, |_| None)
            .expect_err("malformed binding must be rejected");
        assert!(error.to_string().contains("ACCESS_KEY_ID:SECRET:TENANT"));

        let mut command = base_command();
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let error = resolve_adapter_enablement_with_env(&command, |_| None)
            .expect_err("bindings without port must be rejected");
        assert!(error.to_string().contains("--dynamodb-port"));
    }

    #[test]
    fn mongodb_host_is_loopback_only_even_with_allow_network() {
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_host = "0.0.0.0".to_string();
        command.allow_network = true;
        let error = resolve_adapter_enablement_with_env(&command, |name| match name {
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
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        command.dynamodb_host = "0.0.0.0".to_string();
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let error = resolve_adapter_enablement_with_env(&command, |_| None)
            .expect_err("non-loopback dynamodb host without --allow-network must be refused");
        assert!(error.to_string().contains("--allow-network"));

        command.allow_network = true;
        let resolved = resolve_adapter_enablement_with_env(&command, |_| None)
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

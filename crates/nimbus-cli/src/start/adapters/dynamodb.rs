use nimbus::{Error, TenantId};
use nimbus_server::DynamoDbConfig;

use crate::start::StartCommand;
use crate::start::network_bind::ensure_host_opt_in;

use super::{CredentialStore, DEFAULT_WIRE_TENANT, adapter_bind_addr};

pub(super) const DYNAMODB_ACCESS_KEYS_ENV: &str = "NIMBUS_DYNAMODB_ACCESS_KEYS";

pub(crate) const DYNAMODB_CONVENTIONAL_PORT: u16 = 8000;

pub(super) fn resolve_dynamodb(
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
                return Err(Error::InvalidInput(format!(
                    "DynamoDB conventional port {DYNAMODB_CONVENTIONAL_PORT} is busy; \
                     pass --dynamodb-port to serve on another port or --no-dynamodb \
                     to disable the listener"
                )));
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
        let tenant = TenantId::new(DEFAULT_WIRE_TENANT)?;
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

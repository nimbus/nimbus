use std::path::Path;

use nimbus::{Error, TenantId};
use nimbus_server::CloudflareConfig;

use crate::start::StartCommand;
use crate::start::network_bind::ensure_host_opt_in;

use super::{CredentialStore, DEFAULT_WIRE_TENANT};

pub(super) fn resolve_cloudflare(
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
    let tenant = TenantId::new(DEFAULT_WIRE_TENANT)?;
    config = config.with_signed_access_key(
        credentials.dynamodb_access_key_id.clone(),
        tenant,
        credentials.dynamodb_secret_access_key.clone(),
    );
    Ok(Some(config))
}

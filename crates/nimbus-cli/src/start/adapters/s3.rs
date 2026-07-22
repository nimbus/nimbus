use nimbus::{Error, ObjectStorageConfig, ObjectStorageEnv, TenantId};
use nimbus_server::{S3AccessKeyRegistry, S3Config};

use crate::start::StartCommand;
use crate::start::network_bind::ensure_host_opt_in;

use super::{CredentialStore, DEFAULT_WIRE_TENANT, adapter_bind_addr};

pub(super) const S3_ACCESS_KEYS_ENV: &str = "NIMBUS_S3_ACCESS_KEYS";

pub(crate) const S3_CONVENTIONAL_PORT: u16 = 9000;

struct AdapterObjectStorageEnv<'a, F> {
    lookup: &'a F,
}

impl<F> ObjectStorageEnv for AdapterObjectStorageEnv<'_, F>
where
    F: Fn(&str) -> Option<String>,
{
    fn get(&self, key: &str) -> nimbus::Result<Option<String>> {
        match (self.lookup)(key) {
            Some(value) => Ok(Some(value)),
            // The start-command lookup closure is built over
            // `std::env::var(..).ok()`, which erases the set-but-non-UTF-8
            // case. Recover the distinction here: a variable that IS set in
            // the process environment but did not survive the lossy lookup
            // must fail configuration closed (a mangled
            // `..._LOCAL_LEG=erasure` would otherwise silently start
            // against the pack root).
            None => match std::env::var(key) {
                Err(std::env::VarError::NotUnicode(_)) => Err(nimbus::Error::InvalidInput(
                    format!("environment variable {key} is set but not valid UTF-8"),
                )),
                _ => Ok(None),
            },
        }
    }
}

pub(super) fn resolve_s3(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
    port_is_free: &impl Fn(u16) -> bool,
    store: &mut CredentialStore<'_>,
) -> Result<Option<S3Config>, Error> {
    if !command.s3 {
        if command.s3_port.is_some() || !command.s3_access_key.is_empty() {
            return Err(Error::InvalidInput(
                "--no-s3 conflicts with --s3-port/--s3-access-key; \
                 drop the configuration flags or re-enable the listener"
                    .to_string(),
            ));
        }
        return Ok(None);
    }
    ensure_host_opt_in(&command.s3_host, command.allow_network)
        .map_err(|error| Error::InvalidInput(format!("--s3-host: {error}")))?;
    let port = match command.s3_port {
        Some(port) => port,
        None => {
            if !port_is_free(S3_CONVENTIONAL_PORT) {
                return Err(Error::InvalidInput(format!(
                    "S3 conventional port {S3_CONVENTIONAL_PORT} is busy; \
                     pass --s3-port to serve on another port or --no-s3 \
                     to disable the listener"
                )));
            }
            S3_CONVENTIONAL_PORT
        }
    };
    let raw_bindings: Vec<String> = if command.s3_access_key.is_empty() {
        env_lookup(S3_ACCESS_KEYS_ENV)
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        command.s3_access_key.clone()
    };
    let mut convex_download_secret = None;
    let access_keys = if raw_bindings.is_empty() {
        let credentials = store.get()?;
        let tenant = TenantId::new(DEFAULT_WIRE_TENANT)?;
        convex_download_secret = Some(credentials.s3_secret_access_key.clone().into_bytes());
        S3AccessKeyRegistry::new().bind_signed(
            credentials.s3_access_key_id.clone(),
            tenant,
            credentials.s3_secret_access_key.clone(),
        )
    } else {
        S3AccessKeyRegistry::from_operator_spec(&raw_bindings.join(","))
            .map_err(|error| Error::InvalidInput(error.to_string()))?
    };
    let object_storage =
        ObjectStorageConfig::from_sources(None, &AdapterObjectStorageEnv { lookup: env_lookup })?;
    let mut config = S3Config::new(port)
        .with_bind_addr(adapter_bind_addr(&command.s3_host, port, "--s3-host")?)
        .with_access_keys(access_keys)
        .with_object_storage_config(object_storage);
    if let Some(secret) = convex_download_secret {
        config = config.with_convex_download_secret(secret);
    }
    Ok(Some(config))
}

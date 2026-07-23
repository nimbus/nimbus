use nimbus::{Error, TenantId};
use nimbus_server::{CloudFunctionsHttpTenantBinding, CloudFunctionsRegistry};

use crate::start::StartCommand;

/// Trusted deployment→tenant binding for Cloud Functions HTTP targets.
///
/// A request path may select a function but never a tenant. Plain `start`
/// therefore requires this explicit operator binding whenever the active
/// artifact exposes HTTP targets. `nimbus dev` binds the deployment to its
/// auto-provisioned tenant and ignores ambient production binding config.
pub(super) const CLOUD_FUNCTIONS_TENANT_ENV: &str = "NIMBUS_CLOUD_FUNCTIONS_TENANT";

pub(super) fn resolve_cloud_functions_http_tenant(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<CloudFunctionsHttpTenantBinding>, Error> {
    let raw_tenant = command
        .auto_tenant
        .as_deref()
        .map(str::to_owned)
        .or_else(|| env_lookup(CLOUD_FUNCTIONS_TENANT_ENV));
    raw_tenant
        .map(|raw| {
            let tenant_id = TenantId::new(raw.trim()).map_err(|error| {
                Error::InvalidInput(format!(
                    "invalid {CLOUD_FUNCTIONS_TENANT_ENV} value: {error}"
                ))
            })?;
            CloudFunctionsHttpTenantBinding::new(tenant_id)
        })
        .transpose()
}

pub(crate) fn ensure_http_targets_are_bound(
    registry: Option<&CloudFunctionsRegistry>,
    binding: Option<&CloudFunctionsHttpTenantBinding>,
) -> Result<(), Error> {
    if let Some(registry) = registry {
        registry.ensure_http_tenant_binding(binding).map_err(|_| {
            Error::InvalidInput(format!(
                "cloud functions HTTP targets require {CLOUD_FUNCTIONS_TENANT_ENV}=<TENANT>; \
                 the request URL is never used to select a tenant"
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_start_without_binding_stays_unbound() {
        let binding = resolve_cloud_functions_http_tenant(&StartCommand::default(), &|_| None)
            .expect("plain start should resolve");

        assert!(binding.is_none());
    }

    #[test]
    fn plain_start_reads_explicit_binding() {
        let binding = resolve_cloud_functions_http_tenant(&StartCommand::default(), &|name| {
            (name == CLOUD_FUNCTIONS_TENANT_ENV).then(|| "tenant-a".to_owned())
        })
        .expect("explicit binding should resolve")
        .expect("binding should be present");

        assert_eq!(binding.tenant_id().as_str(), "tenant-a");
    }

    #[test]
    fn dev_binding_uses_auto_tenant_instead_of_ambient_operator_value() {
        let command = StartCommand {
            auto_tenant: Some("dev-tenant".to_owned()),
            ..StartCommand::default()
        };
        let binding = resolve_cloud_functions_http_tenant(&command, &|name| {
            (name == CLOUD_FUNCTIONS_TENANT_ENV).then(|| "production-tenant".to_owned())
        })
        .expect("dev binding should resolve")
        .expect("binding should be present");

        assert_eq!(binding.tenant_id().as_str(), "dev-tenant");
    }

    #[test]
    fn reserved_binding_is_rejected() {
        let error = resolve_cloud_functions_http_tenant(&StartCommand::default(), &|name| {
            (name == CLOUD_FUNCTIONS_TENANT_ENV).then(|| "_nimbus".to_owned())
        })
        .expect_err("reserved binding must fail startup");

        assert!(error.to_string().contains("reserved Nimbus tenant"));
    }
}

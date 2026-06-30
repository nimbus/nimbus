use super::raw::RawComposeService;
use super::*;

pub(super) fn warnings_for_known_ignored_service_fields(
    service: &RawComposeService,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if service.networks.is_some() {
        warnings.push("networks: ignored (nimbus uses TSI networking)".to_owned());
    }
    if service.configs.is_some() {
        warnings.push("configs: ignored (not yet supported by nimbus compose config)".to_owned());
    }
    if service.secrets.is_some() {
        warnings.push("secrets: ignored (not yet supported by nimbus compose config)".to_owned());
    }
    if service.cap_add.is_some() {
        warnings.push("cap_add: ignored (VM isolation replaces container capabilities)".to_owned());
    }
    if service.cap_drop.is_some() {
        warnings
            .push("cap_drop: ignored (VM isolation replaces container capabilities)".to_owned());
    }
    if service.privileged.is_some() {
        warnings.push(
            "privileged: ignored (VM isolation replaces privileged container mode)".to_owned(),
        );
    }
    if service.logging.is_some() {
        warnings.push(
            "logging: ignored (conmon-backed logging is the current source of truth)".to_owned(),
        );
    }
    warnings
}

pub(super) fn warnings_for_unknown_fields(
    prefix: &str,
    fields: BTreeMap<String, Value>,
) -> Vec<String> {
    fields
        .into_keys()
        .map(|field| {
            if field.starts_with("x-") {
                format!("{prefix}.{field}: ignored extension field")
            } else {
                format!("{prefix}.{field}: ignored unknown field")
            }
        })
        .collect()
}

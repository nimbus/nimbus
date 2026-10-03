use super::*;

#[cfg(test)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(super) struct BuildahInspectPayload {
    #[serde(default, rename = "OCIv1")]
    pub(super) oci_v1: Option<BuildahImageEnvelope>,
    #[serde(default, rename = "Docker")]
    pub(super) docker: Option<BuildahImageEnvelope>,
}

#[cfg(test)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(super) struct BuildahImageEnvelope {
    #[serde(default, alias = "Config", alias = "config")]
    pub(super) config: BuildahImageFields,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct ImageConfigBlobPayload {
    #[serde(default, alias = "Config", alias = "config")]
    config: BuildahImageFields,
    #[serde(
        default,
        alias = "ContainerConfig",
        alias = "containerConfig",
        alias = "container_config"
    )]
    container_config: BuildahImageFields,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(super) struct BuildahImageFields {
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Entrypoint",
        alias = "entrypoint"
    )]
    pub(super) entrypoint: Vec<String>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Cmd",
        alias = "cmd"
    )]
    pub(super) cmd: Vec<String>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Env",
        alias = "env"
    )]
    pub(super) env: Vec<String>,
    #[serde(default, alias = "WorkingDir", alias = "working_dir")]
    pub(super) working_dir: Option<String>,
    #[serde(default, alias = "User", alias = "user")]
    pub(super) user: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "ExposedPorts",
        alias = "exposed_ports"
    )]
    pub(super) exposed_ports: BTreeMap<String, Value>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Volumes",
        alias = "volumes"
    )]
    pub(super) volumes: BTreeMap<String, Value>,
    #[serde(default, alias = "StopSignal", alias = "stop_signal")]
    pub(super) stop_signal: Option<String>,
    #[serde(default, alias = "Healthcheck", alias = "healthcheck")]
    pub(super) healthcheck: Option<ImageHealthcheck>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        alias = "Labels",
        alias = "labels"
    )]
    pub(super) labels: BTreeMap<String, String>,
}

/// Deserialize a field that may be `null` in JSON as the type's `Default` value.
/// This handles the common OCI case where buildah/Docker write `"Entrypoint": null`
/// instead of omitting the field entirely.
fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(|opt| opt.unwrap_or_default())
}

#[cfg(test)]
pub(super) fn parse_inspect_output(stdout: &[u8]) -> Result<OciImageConfig> {
    let value: Value =
        serde_json::from_slice(stdout).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to parse buildah inspect JSON: {error}"),
        })?;

    let payload = match value {
        Value::Array(entries) => {
            let first =
                entries
                    .into_iter()
                    .next()
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: "buildah inspect returned an empty JSON array".to_owned(),
                    })?;
            serde_json::from_value::<BuildahInspectPayload>(first).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to decode buildah inspect payload: {error}"),
                }
            })?
        }
        Value::Object(_) => {
            serde_json::from_value::<BuildahInspectPayload>(value).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to decode buildah inspect payload: {error}"),
                }
            })?
        }
        _ => {
            return Err(SandboxError::OperationFailed {
                message: "buildah inspect JSON was neither an object nor an array".to_owned(),
            });
        }
    };

    Ok(OciImageConfig::from_payload(payload))
}

pub(crate) fn parse_image_config_blob(blob: &[u8]) -> Result<OciImageConfig> {
    let value: Value =
        serde_json::from_slice(blob).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to parse OCI image config JSON: {error}"),
        })?;

    match value {
        Value::Object(map)
            if map.contains_key("config")
                || map.contains_key("Config")
                || map.contains_key("container_config")
                || map.contains_key("containerConfig")
                || map.contains_key("ContainerConfig") =>
        {
            let payload = serde_json::from_value::<ImageConfigBlobPayload>(Value::Object(map))
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to decode OCI image config payload: {error}"),
                })?;
            Ok(OciImageConfig::from_fields(
                payload.config,
                payload.container_config,
            ))
        }
        Value::Object(map) => {
            let fields = serde_json::from_value::<BuildahImageFields>(Value::Object(map)).map_err(
                |error| SandboxError::OperationFailed {
                    message: format!("failed to decode OCI image config fields: {error}"),
                },
            )?;
            Ok(OciImageConfig::from_fields(
                fields,
                BuildahImageFields::default(),
            ))
        }
        _ => Err(SandboxError::OperationFailed {
            message: "OCI image config JSON was not an object".to_owned(),
        }),
    }
}

use super::raw::*;
use super::*;

pub(super) fn resolve_ports(
    service_name: &str,
    ports: Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<Vec<ComposePortBindingPlan>, Error> {
    ports
        .into_iter()
        .enumerate()
        .map(|(index, port)| parse_port_binding(service_name, &port, index, warnings))
        .collect()
}

pub(super) fn parse_port_binding(
    service_name: &str,
    raw: &str,
    index: usize,
    warnings: &mut Vec<String>,
) -> Result<ComposePortBindingPlan, Error> {
    let (port_part, protocol) = match raw.rsplit_once('/') {
        Some((port_part, "tcp")) => (port_part, PublishedEndpointProtocol::Tcp),
        Some((_, other)) => {
            return Err(Error::InvalidInput(format!(
                "services.{service_name}.ports: unsupported protocol {other:?}; nimbus currently supports tcp only"
            )));
        }
        None => (raw, PublishedEndpointProtocol::Tcp),
    };

    let segments = port_part.split(':').collect::<Vec<_>>();
    let (host_address, host_port, guest_port) = match segments.as_slice() {
        [host_port, guest_port] => (
            "127.0.0.1".parse::<IpAddr>().expect("localhost ip parses"),
            parse_u16_field(
                &format!("services.{service_name}.ports host port"),
                host_port,
            )?,
            parse_u16_field(
                &format!("services.{service_name}.ports guest port"),
                guest_port,
            )?,
        ),
        [host_address, host_port, guest_port] => (
            host_address.parse::<IpAddr>().map_err(|error| {
                Error::InvalidInput(format!(
                    "services.{service_name}.ports: invalid host address {host_address:?}: {error}"
                ))
            })?,
            parse_u16_field(
                &format!("services.{service_name}.ports host port"),
                host_port,
            )?,
            parse_u16_field(
                &format!("services.{service_name}.ports guest port"),
                guest_port,
            )?,
        ),
        _ => {
            return Err(Error::InvalidInput(format!(
                "services.{service_name}.ports: unsupported port mapping {raw:?}; expected HOST:CONTAINER or HOST_IP:HOST:CONTAINER"
            )));
        }
    };

    if index > 0 {
        warnings.push(format!(
            "ports[{index}]: additional exposed port {guest_port} will be available through ctx.services.<name>.endpoints"
        ));
    }

    Ok(ComposePortBindingPlan {
        name: if index == 0 {
            "default".to_owned()
        } else {
            format!("tcp-{guest_port}")
        },
        protocol,
        host_address,
        host_port,
        guest_port,
    })
}

pub(super) fn parse_u16_field(label: &str, value: &str) -> Result<u16, Error> {
    value
        .trim()
        .parse::<u16>()
        .map_err(|error| Error::InvalidInput(format!("{label} {value:?} is invalid: {error}")))
}

pub(super) fn parse_cpu_count(
    service_name: &str,
    value: &str,
    warnings: &mut Vec<String>,
) -> Result<u8, Error> {
    let parsed = value.trim().parse::<f64>().map_err(|error| {
        Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.cpus: invalid value {value:?}: {error}"
        ))
    })?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.cpus: expected a positive CPU value, got {value:?}"
        )));
    }

    let rounded = parsed.ceil();
    if rounded > u8::MAX as f64 {
        return Err(Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.cpus: value {value:?} exceeds the current krun vCPU limit of {}",
            u8::MAX
        )));
    }

    if (rounded - parsed).abs() > f64::EPSILON {
        warnings.push(format!(
            "deploy.resources.limits.cpus: rounded {value} up to {} vCPU(s) because the krun backend currently requires whole guest CPU counts",
            rounded as u8
        ));
    }

    Ok(rounded as u8)
}

pub(super) fn parse_memory_limit(service_name: &str, value: &str) -> Result<u64, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.memory: expected a byte value like 256M or 1G"
        )));
    }

    let digits_len = trimmed
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .count();
    if digits_len == 0 {
        return Err(Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.memory: invalid value {value:?}. Expected format: 256M, 1G, etc."
        )));
    }

    let amount = trimmed[..digits_len].parse::<u64>().map_err(|error| {
        Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.memory: invalid numeric value {value:?}: {error}"
        ))
    })?;
    let unit = trimmed[digits_len..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        "g" | "gb" => 1024 * 1024 * 1024,
        "t" | "tb" => 1024_u64.pow(4),
        other => {
            return Err(Error::InvalidInput(format!(
                "services.{service_name}.deploy.resources.limits.memory: unsupported unit {other:?}. Expected format: 256M, 1G, etc."
            )));
        }
    };
    amount.checked_mul(multiplier).ok_or_else(|| {
        Error::InvalidInput(format!(
            "services.{service_name}.deploy.resources.limits.memory: value {value:?} overflowed u64 bytes"
        ))
    })
}

pub(super) fn compose_lifecycle_spec(
    restart: &ComposeRestartPlan,
    stop_grace_period: Option<&str>,
    stop_grace_period_label: &str,
) -> Result<SandboxLifecycleSpec, Error> {
    let mut lifecycle = SandboxLifecycleSpec::default().with_restart_policy(restart.policy);
    if let Some(stop_grace_period) = stop_grace_period {
        lifecycle = lifecycle.with_stop_timeout(parse_compose_duration(
            stop_grace_period_label,
            stop_grace_period,
        )?);
    }
    Ok(lifecycle)
}

pub(super) fn parse_compose_duration(label: &str, value: &str) -> Result<Duration, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput(format!(
            "{label}: expected a duration like 30s or 1m30s"
        )));
    }

    let mut total_nanos = 0_f64;
    let mut offset = 0;
    while offset < trimmed.len() {
        let remaining = &trimmed[offset..];
        let skipped = remaining
            .chars()
            .take_while(|character| character.is_ascii_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        offset += skipped;
        if offset >= trimmed.len() {
            break;
        }

        let number_start = offset;
        let mut seen_digit = false;
        let mut seen_decimal = false;
        while offset < trimmed.len() {
            let character = trimmed[offset..]
                .chars()
                .next()
                .expect("slice should contain a character");
            if character.is_ascii_digit() {
                seen_digit = true;
                offset += character.len_utf8();
                continue;
            }
            if character == '.' && !seen_decimal {
                seen_decimal = true;
                offset += character.len_utf8();
                continue;
            }
            break;
        }
        if !seen_digit {
            return Err(Error::InvalidInput(format!(
                "{label}: invalid duration {value:?}. Expected a duration like 30s or 1m30s"
            )));
        }

        let amount = trimmed[number_start..offset]
            .parse::<f64>()
            .map_err(|error| {
                Error::InvalidInput(format!("{label}: invalid duration {value:?}: {error}"))
            })?;
        if !amount.is_finite() || amount < 0.0 {
            return Err(Error::InvalidInput(format!(
                "{label}: invalid duration {value:?}. Expected a positive duration"
            )));
        }

        let remaining = &trimmed[offset..];
        let (unit, unit_nanos) = ["ns", "us", "µs", "μs", "ms", "s", "m", "h"]
            .into_iter()
            .find_map(|unit| {
                remaining
                    .strip_prefix(unit)
                    .map(|_| (unit, duration_unit_nanos(unit)))
            })
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "{label}: invalid duration {value:?}. Supported units are ns, us, ms, s, m, h"
                ))
            })?;
        offset += unit.len();
        total_nanos += amount * unit_nanos;
    }

    if !total_nanos.is_finite() || total_nanos < 0.0 || total_nanos > u64::MAX as f64 {
        return Err(Error::InvalidInput(format!(
            "{label}: duration {value:?} exceeds the supported range"
        )));
    }

    Ok(Duration::from_nanos(total_nanos.round() as u64))
}

pub(super) fn duration_unit_nanos(unit: &str) -> f64 {
    match unit {
        "ns" => 1.0,
        "us" | "µs" | "μs" => 1_000.0,
        "ms" => 1_000_000.0,
        "s" => 1_000_000_000.0,
        "m" => 60.0 * 1_000_000_000.0,
        "h" => 60.0 * 60.0 * 1_000_000_000.0,
        _ => unreachable!("unsupported duration unit {unit}"),
    }
}

pub(super) fn resolve_depends_on(
    depends_on: Option<RawComposeDependsOn>,
) -> Result<BTreeMap<String, ComposeDependencyCondition>, Error> {
    let Some(depends_on) = depends_on else {
        return Ok(BTreeMap::new());
    };

    match depends_on {
        RawComposeDependsOn::List(list) => Ok(list
            .into_iter()
            .map(|name| (name, ComposeDependencyCondition::ServiceStarted))
            .collect()),
        RawComposeDependsOn::Map(map) => map
            .into_iter()
            .map(|(name, detail)| {
                let condition = match detail.condition.as_deref().unwrap_or("service_started") {
                    "service_started" => ComposeDependencyCondition::ServiceStarted,
                    "service_healthy" => ComposeDependencyCondition::ServiceHealthy,
                    other => {
                        return Err(Error::InvalidInput(format!(
                            "depends_on.{name}.condition: unsupported condition {other:?}; expected service_started or service_healthy"
                        )));
                    }
                };
                Ok((name, condition))
            })
            .collect(),
    }
}

pub(super) fn resolve_volume_mounts(
    volumes: Vec<RawComposeVolumeMount>,
) -> Vec<ComposeVolumeMountPlan> {
    volumes
        .into_iter()
        .filter_map(|volume| match volume {
            RawComposeVolumeMount::Short(raw) => parse_short_volume_mount(&raw),
            RawComposeVolumeMount::Long(detail) => Some(ComposeVolumeMountPlan {
                source: detail.source,
                target: detail.target,
                kind: detail.kind.unwrap_or_else(|| "volume".to_owned()),
                read_only: detail.read_only.unwrap_or(false),
            }),
        })
        .collect()
}

pub(super) fn parse_short_volume_mount(raw: &str) -> Option<ComposeVolumeMountPlan> {
    let parts = raw.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [target] => Some(ComposeVolumeMountPlan {
            source: None,
            target: (*target).to_owned(),
            kind: "anonymous".to_owned(),
            read_only: false,
        }),
        [source, target] => Some(ComposeVolumeMountPlan {
            source: Some((*source).to_owned()),
            target: (*target).to_owned(),
            kind: classify_volume_source(source),
            read_only: false,
        }),
        [source, target, mode] => Some(ComposeVolumeMountPlan {
            source: Some((*source).to_owned()),
            target: (*target).to_owned(),
            kind: classify_volume_source(source),
            read_only: mode.split(',').any(|flag| flag.trim() == "ro"),
        }),
        _ => None,
    }
}

pub(super) fn classify_volume_source(source: &str) -> String {
    if source.starts_with('/')
        || source.starts_with("./")
        || source.starts_with("../")
        || source.starts_with('~')
    {
        "bind".to_owned()
    } else {
        "volume".to_owned()
    }
}

pub(super) fn parse_environment_map(
    environment: Option<RawComposeStringMap>,
    field_label: &str,
) -> Result<BTreeMap<String, String>, Error> {
    match environment {
        None => Ok(BTreeMap::new()),
        Some(RawComposeStringMap::List(entries)) => entries
            .into_iter()
            .filter_map(|entry| parse_inline_key_value_entry(&entry))
            .map(|(key, value)| Ok((key, value)))
            .collect(),
        Some(RawComposeStringMap::Map(entries)) => {
            let mut resolved = BTreeMap::new();
            for (key, value) in entries {
                if let Some(value) = scalar_value_to_string(field_label, value)? {
                    resolved.insert(key, value);
                }
            }
            Ok(resolved)
        }
    }
}

pub(super) fn parse_string_map(
    values: Option<RawComposeStringMap>,
    field_label: &str,
) -> Result<BTreeMap<String, String>, Error> {
    parse_environment_map(values, field_label)
}

pub(super) fn parse_inline_key_value_entry(entry: &str) -> Option<(String, String)> {
    let (key, value) = entry.split_once('=')?;
    Some((key.trim().to_owned(), value.trim().to_owned()))
}

pub(super) fn scalar_value_to_string(
    field_label: &str,
    value: Option<Value>,
) -> Result<Option<String>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };

    match value {
        Value::Null => Ok(None),
        Value::Bool(value) => Ok(Some(value.to_string())),
        Value::Number(value) => Ok(Some(value.to_string())),
        Value::String(value) => Ok(Some(value)),
        other => Err(Error::InvalidInput(format!(
            "{field_label}: expected a scalar string/number/bool value, got {other:?}"
        ))),
    }
}

pub(super) fn load_env_files(
    compose_dir: &Path,
    env_file: Option<RawComposeEnvFile>,
) -> Result<BTreeMap<String, String>, Error> {
    let mut environment = BTreeMap::new();
    let entries = match env_file {
        None => return Ok(environment),
        Some(RawComposeEnvFile::Single(path)) => vec![RawComposeEnvFileEntry::Path(path)],
        Some(RawComposeEnvFile::List(entries)) => entries,
    };

    for entry in entries {
        let detail = match entry {
            RawComposeEnvFileEntry::Path(path) => RawComposeEnvFileDetail {
                path,
                required: None,
            },
            RawComposeEnvFileEntry::Detail(detail) => detail,
        };
        let path = compose_dir.join(&detail.path);
        let bytes = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && detail.required == Some(false) =>
            {
                continue;
            }
            Err(error) => {
                return Err(Error::InvalidInput(format!(
                    "failed to read env_file {}: {error}",
                    path.display()
                )));
            }
        };
        for line in bytes.lines() {
            if let Some((key, value)) = parse_env_file_line(line) {
                environment.insert(key, value);
            }
        }
    }

    Ok(environment)
}

pub(super) fn parse_env_file_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let (key, value) = trimmed
        .split_once('=')
        .or_else(|| trimmed.split_once(':'))
        .map(|(key, value)| (key.trim(), value.trim()))?;

    if key.is_empty() {
        return None;
    }

    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
        .to_owned();
    Some((key.to_owned(), value))
}

pub(super) fn default_project_name(path: &Path) -> &str {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .or_else(|| path.file_stem().and_then(|name| name.to_str()))
        .unwrap_or("nimbus")
}

pub(super) fn sanitize_project_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

pub(super) fn default_build_image_name(project_name: &str, service_name: &str) -> String {
    format!(
        "nimbus-{}-{}",
        sanitize_project_name(project_name),
        sanitize_project_name(service_name)
    )
}

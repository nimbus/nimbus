use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::buildah::{ImageHealthcheck, OciImageConfig, resolve_image_user_from_rootfs};
use super::materializer::{OciImageMaterializer, PreparedMaterializedImageLaunch};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxImageProcessOverrides;

const SCRATCH_IMAGE_REFERENCE: &str = "scratch";
const DEFAULT_SHELL: &[&str] = &["/bin/sh", "-c"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciDockerfileBuilder {
    materializer: OciImageMaterializer,
}

impl OciDockerfileBuilder {
    pub(crate) fn under_state_root(state_root: impl Into<PathBuf>) -> Self {
        Self {
            materializer: OciImageMaterializer::under_state_root(state_root),
        }
    }

    pub(crate) fn prepare_built_image_launch(
        &self,
        sandbox_id: &SandboxId,
        image_name: &str,
        dockerfile_path: &Path,
        context_path: &Path,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMaterializedImageLaunch> {
        let dockerfile = DockerfileRecipe::load(dockerfile_path)?;
        let (artifact, mut image_config) = if dockerfile.base_image == SCRATCH_IMAGE_REFERENCE {
            (
                self.materializer.prepare_scratch_rootfs(
                    sandbox_id,
                    &synthetic_built_image_reference(image_name),
                )?,
                OciImageConfig {
                    entrypoint: Vec::new(),
                    cmd: Vec::new(),
                    env: Vec::new(),
                    working_dir: None,
                    user: None,
                    exposed_ports: Vec::new(),
                    volumes: Vec::new(),
                    stop_signal: None,
                    healthcheck: None,
                    labels: BTreeMap::new(),
                },
            )
        } else {
            let prepared = self
                .materializer
                .prepare_image_rootfs_with_config(sandbox_id, &dockerfile.base_image)?;
            (prepared.artifact, prepared.image_config)
        };

        dockerfile.apply(context_path, &artifact.rootfs_path, &mut image_config)?;

        let resolved_user = resolve_image_user_from_rootfs(
            &artifact.rootfs_path,
            overrides.user.as_deref().or(image_config.user.as_deref()),
        )?;
        image_config.user = resolved_user;

        let mut process_overrides = overrides.clone();
        process_overrides.user = None;

        Ok(PreparedMaterializedImageLaunch {
            launch_defaults: image_config
                .resolve_launch_defaults(&artifact.rootfs_path, &process_overrides)?,
            artifact,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DockerfileRecipe {
    base_image: String,
    instructions: Vec<DockerfileInstruction>,
}

impl DockerfileRecipe {
    fn load(dockerfile_path: &Path) -> Result<Self> {
        let raw =
            fs::read_to_string(dockerfile_path).map_err(|error| SandboxError::InvalidSpec {
                message: format!(
                    "failed to read Dockerfile {}: {error}",
                    dockerfile_path.display()
                ),
            })?;

        let mut base_image = None;
        let mut instructions = Vec::new();
        for line in logical_dockerfile_lines(&raw)? {
            let (keyword, body) = split_instruction(&line)?;
            match keyword.as_str() {
                "FROM" => {
                    if base_image.is_some() {
                        return Err(unsupported_instruction_error(
                            "multi-stage FROM",
                            "use a single-stage Dockerfile or a prebuilt image for now",
                        ));
                    }
                    base_image = Some(parse_from_instruction(&body)?);
                }
                "ADD" | "COPY" => {
                    instructions.push(DockerfileInstruction::Copy(parse_copy_instruction(
                        &body, &keyword,
                    )?));
                }
                "CMD" => instructions.push(DockerfileInstruction::Cmd(parse_command_instruction(
                    &body, &keyword,
                )?)),
                "ENTRYPOINT" => instructions.push(DockerfileInstruction::Entrypoint(
                    parse_command_instruction(&body, &keyword)?,
                )),
                "ENV" => {
                    instructions.push(DockerfileInstruction::Env(parse_key_value_instruction(
                        &body, &keyword,
                    )?));
                }
                "EXPOSE" => instructions.push(DockerfileInstruction::Expose(
                    parse_expose_instruction(&body)?,
                )),
                "HEALTHCHECK" => instructions.push(DockerfileInstruction::Healthcheck(
                    parse_healthcheck_instruction(&body)?,
                )),
                "LABEL" => {
                    instructions.push(DockerfileInstruction::Label(parse_key_value_instruction(
                        &body, &keyword,
                    )?));
                }
                "RUN" => {
                    return Err(unsupported_instruction_error(
                        "RUN",
                        "use a prebuilt image or a Dockerfile that only rewrites runtime metadata and local COPY steps for now",
                    ));
                }
                "STOPSIGNAL" => {
                    instructions.push(DockerfileInstruction::StopSignal(body.trim().to_owned()));
                }
                "USER" => instructions.push(DockerfileInstruction::User(body.trim().to_owned())),
                "VOLUME" => instructions.push(DockerfileInstruction::Volume(
                    parse_volume_instruction(&body)?,
                )),
                "WORKDIR" => {
                    instructions.push(DockerfileInstruction::Workdir(body.trim().to_owned()));
                }
                "ARG" | "ONBUILD" | "SHELL" => {
                    return Err(unsupported_instruction_error(
                        &keyword,
                        "use a prebuilt image or simplify the Dockerfile for the current macOS guest builder",
                    ));
                }
                other => {
                    return Err(SandboxError::InvalidSpec {
                        message: format!(
                            "Dockerfile uses unsupported instruction {other:?}; use a prebuilt image or a supported Dockerfile subset"
                        ),
                    });
                }
            }
        }

        Ok(Self {
            base_image: base_image.ok_or_else(|| SandboxError::InvalidSpec {
                message: "Dockerfile must start with a FROM instruction".to_owned(),
            })?,
            instructions,
        })
    }

    fn apply(
        &self,
        context_path: &Path,
        rootfs_path: &Path,
        image_config: &mut OciImageConfig,
    ) -> Result<()> {
        for instruction in &self.instructions {
            instruction.apply(context_path, rootfs_path, image_config)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DockerfileInstruction {
    Copy(CopyInstruction),
    Cmd(Vec<String>),
    Entrypoint(Vec<String>),
    Env(Vec<(String, String)>),
    Expose(Vec<String>),
    Healthcheck(Option<ImageHealthcheck>),
    Label(Vec<(String, String)>),
    StopSignal(String),
    User(String),
    Volume(Vec<String>),
    Workdir(String),
}

impl DockerfileInstruction {
    fn apply(
        &self,
        context_path: &Path,
        rootfs_path: &Path,
        image_config: &mut OciImageConfig,
    ) -> Result<()> {
        match self {
            Self::Copy(copy) => {
                apply_copy_instruction(copy, context_path, rootfs_path, image_config)
            }
            Self::Cmd(args) => {
                image_config.cmd = args.clone();
                Ok(())
            }
            Self::Entrypoint(args) => {
                image_config.entrypoint = args.clone();
                Ok(())
            }
            Self::Env(entries) => {
                merge_env_entries(&mut image_config.env, entries);
                Ok(())
            }
            Self::Expose(ports) => {
                for port in ports {
                    if !image_config
                        .exposed_ports
                        .iter()
                        .any(|existing| existing == port)
                    {
                        image_config.exposed_ports.push(port.clone());
                    }
                }
                Ok(())
            }
            Self::Healthcheck(healthcheck) => {
                image_config.healthcheck = healthcheck.clone();
                Ok(())
            }
            Self::Label(entries) => {
                for (key, value) in entries {
                    image_config.labels.insert(key.clone(), value.clone());
                }
                Ok(())
            }
            Self::StopSignal(signal) => {
                image_config.stop_signal = Some(signal.clone());
                Ok(())
            }
            Self::User(user) => {
                image_config.user = Some(user.clone());
                Ok(())
            }
            Self::Volume(volumes) => {
                for volume in volumes {
                    if !image_config
                        .volumes
                        .iter()
                        .any(|existing| existing == volume)
                    {
                        image_config.volumes.push(volume.clone());
                    }
                }
                Ok(())
            }
            Self::Workdir(workdir) => {
                image_config.working_dir = Some(resolve_container_path(
                    image_config.working_dir.as_deref(),
                    workdir,
                )?);
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopyInstruction {
    sources: Vec<String>,
    destination: String,
}

fn logical_dockerfile_lines(raw: &str) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for physical_line in raw.lines() {
        let trimmed_start = physical_line.trim_start();
        if current.is_empty() && (trimmed_start.is_empty() || trimmed_start.starts_with('#')) {
            continue;
        }

        let continued = physical_line.trim_end().ends_with('\\');
        let segment = if continued {
            physical_line.trim_end().trim_end_matches('\\').trim_end()
        } else {
            physical_line
        };

        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(segment.trim());

        if !continued {
            if !current.trim().is_empty() {
                lines.push(current.trim().to_owned());
            }
            current.clear();
        }
    }

    if !current.trim().is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "Dockerfile ended with an unterminated line continuation".to_owned(),
        });
    }

    Ok(lines)
}

fn split_instruction(line: &str) -> Result<(String, String)> {
    let mut parts = line.splitn(2, char::is_whitespace);
    let keyword = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!("Dockerfile instruction line {line:?} is empty"),
        })?;
    let body = parts.next().unwrap_or_default().trim().to_owned();
    Ok((keyword.to_ascii_uppercase(), body))
}

fn parse_from_instruction(body: &str) -> Result<String> {
    let tokens = shell_words::split(body).map_err(|error| SandboxError::InvalidSpec {
        message: format!("failed to parse FROM instruction {body:?}: {error}"),
    })?;
    if tokens.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "FROM instruction must specify a base image".to_owned(),
        });
    }
    if tokens.len() > 3 || (tokens.len() == 3 && !tokens[1].eq_ignore_ascii_case("AS")) {
        return Err(unsupported_instruction_error(
            "multi-stage FROM",
            "use a single-stage Dockerfile or a prebuilt image for now",
        ));
    }
    Ok(tokens[0].clone())
}

fn parse_copy_instruction(body: &str, keyword: &str) -> Result<CopyInstruction> {
    let tokens = if body.trim_start().starts_with('[') {
        parse_json_array(body, keyword)?
    } else {
        shell_words::split(body).map_err(|error| SandboxError::InvalidSpec {
            message: format!("failed to parse {keyword} instruction {body:?}: {error}"),
        })?
    };
    if tokens.iter().any(|token| token.starts_with("--")) {
        return Err(unsupported_instruction_error(
            keyword,
            "instruction flags are not supported by the current internal guest builder",
        ));
    }
    if tokens.len() < 2 {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "{keyword} instruction must include at least one source and a destination"
            ),
        });
    }
    let destination = tokens.last().cloned().expect("destination should exist");
    Ok(CopyInstruction {
        sources: tokens[..tokens.len() - 1].to_vec(),
        destination,
    })
}

fn parse_command_instruction(body: &str, keyword: &str) -> Result<Vec<String>> {
    if body.trim_start().starts_with('[') {
        return parse_json_array(body, keyword);
    }
    if body.trim().is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: format!("{keyword} instruction cannot be empty"),
        });
    }
    Ok(DEFAULT_SHELL
        .iter()
        .map(|value| value.to_string())
        .chain(std::iter::once(body.trim().to_owned()))
        .collect())
}

fn parse_key_value_instruction(body: &str, keyword: &str) -> Result<Vec<(String, String)>> {
    let tokens = shell_words::split(body).map_err(|error| SandboxError::InvalidSpec {
        message: format!("failed to parse {keyword} instruction {body:?}: {error}"),
    })?;
    if tokens.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: format!("{keyword} instruction cannot be empty"),
        });
    }
    if tokens[0].contains('=') {
        let mut pairs = Vec::with_capacity(tokens.len());
        for token in tokens {
            let (key, value) = token
                .split_once('=')
                .ok_or_else(|| SandboxError::InvalidSpec {
                    message: format!("{keyword} token {token:?} must be in KEY=VALUE form"),
                })?;
            if key.is_empty() {
                return Err(SandboxError::InvalidSpec {
                    message: format!("{keyword} token {token:?} must not have an empty key"),
                });
            }
            pairs.push((key.to_owned(), value.to_owned()));
        }
        return Ok(pairs);
    }
    if tokens.len() < 2 {
        return Err(SandboxError::InvalidSpec {
            message: format!("{keyword} instruction must include both a key and a value"),
        });
    }
    Ok(vec![(tokens[0].clone(), tokens[1..].join(" "))])
}

fn parse_expose_instruction(body: &str) -> Result<Vec<String>> {
    let tokens = shell_words::split(body).map_err(|error| SandboxError::InvalidSpec {
        message: format!("failed to parse EXPOSE instruction {body:?}: {error}"),
    })?;
    if tokens.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "EXPOSE instruction cannot be empty".to_owned(),
        });
    }
    Ok(tokens
        .into_iter()
        .map(|token| {
            if token.contains('/') {
                token
            } else {
                format!("{token}/tcp")
            }
        })
        .collect())
}

fn parse_volume_instruction(body: &str) -> Result<Vec<String>> {
    if body.trim_start().starts_with('[') {
        return parse_json_array(body, "VOLUME");
    }
    let tokens = shell_words::split(body).map_err(|error| SandboxError::InvalidSpec {
        message: format!("failed to parse VOLUME instruction {body:?}: {error}"),
    })?;
    if tokens.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "VOLUME instruction cannot be empty".to_owned(),
        });
    }
    tokens
        .into_iter()
        .map(|token| resolve_container_path(None, &token))
        .collect()
}

fn parse_healthcheck_instruction(body: &str) -> Result<Option<ImageHealthcheck>> {
    let mut remainder = body.trim();
    if remainder.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "HEALTHCHECK instruction cannot be empty".to_owned(),
        });
    }
    if remainder.eq_ignore_ascii_case("NONE") {
        return Ok(None);
    }

    let mut interval = None;
    let mut timeout = None;
    let mut start_period = None;
    let mut retries = None;
    while remainder.starts_with("--") {
        let (flag_token, rest) =
            split_first_word(remainder).ok_or_else(|| SandboxError::InvalidSpec {
                message: "HEALTHCHECK instruction must specify CMD or CMD-SHELL".to_owned(),
            })?;
        let (flag, value) =
            flag_token
                .split_once('=')
                .ok_or_else(|| SandboxError::InvalidSpec {
                    message: format!(
                        "HEALTHCHECK flag {:?} must be in --key=value form",
                        flag_token
                    ),
                })?;
        match flag {
            "--interval" => interval = Some(parse_healthcheck_duration(value)?),
            "--timeout" => timeout = Some(parse_healthcheck_duration(value)?),
            "--start-period" => start_period = Some(parse_healthcheck_duration(value)?),
            "--retries" => {
                retries = Some(
                    value
                        .parse::<u32>()
                        .map_err(|error| SandboxError::InvalidSpec {
                            message: format!(
                                "HEALTHCHECK retries value {value:?} is invalid: {error}"
                            ),
                        })?,
                )
            }
            _ => {
                return Err(unsupported_instruction_error(
                    "HEALTHCHECK",
                    &format!("unsupported flag {flag:?}"),
                ));
            }
        }
        remainder = rest.trim_start();
    }
    if remainder.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "HEALTHCHECK instruction must specify CMD or CMD-SHELL".to_owned(),
        });
    }
    let (mode, command_body) =
        split_first_word(remainder).ok_or_else(|| SandboxError::InvalidSpec {
            message: "HEALTHCHECK instruction must include a command".to_owned(),
        })?;
    let command_body = command_body.trim();
    if command_body.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: "HEALTHCHECK instruction must include a command".to_owned(),
        });
    }
    let test = if mode.eq_ignore_ascii_case("CMD") {
        let command = if command_body.starts_with('[') {
            parse_json_array(command_body, "HEALTHCHECK CMD")?
        } else {
            shell_words::split(command_body).map_err(|error| SandboxError::InvalidSpec {
                message: format!(
                    "failed to parse HEALTHCHECK CMD instruction {command_body:?}: {error}"
                ),
            })?
        };
        std::iter::once("CMD".to_owned()).chain(command).collect()
    } else if mode.eq_ignore_ascii_case("CMD-SHELL") {
        vec!["CMD-SHELL".to_owned(), command_body.to_owned()]
    } else {
        return Err(SandboxError::InvalidSpec {
            message: format!("HEALTHCHECK instruction must use CMD or CMD-SHELL, got {mode:?}"),
        });
    };

    Ok(Some(ImageHealthcheck {
        test,
        interval,
        timeout,
        start_period,
        retries,
    }))
}

fn parse_healthcheck_duration(raw: &str) -> Result<u64> {
    let split_at = raw
        .find(|character: char| !character.is_ascii_digit())
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!(
                "HEALTHCHECK duration {raw:?} must include a unit such as ms, s, m, or h"
            ),
        })?;
    let value = raw[..split_at]
        .parse::<u64>()
        .map_err(|error| SandboxError::InvalidSpec {
            message: format!("HEALTHCHECK duration {raw:?} has invalid digits: {error}"),
        })?;
    let nanos_per_unit = match &raw[split_at..] {
        "ns" => 1,
        "us" => 1_000,
        "ms" => 1_000_000,
        "s" => 1_000_000_000,
        "m" => 60 * 1_000_000_000,
        "h" => 60 * 60 * 1_000_000_000,
        unit => {
            return Err(SandboxError::InvalidSpec {
                message: format!("HEALTHCHECK duration unit {unit:?} is unsupported"),
            });
        }
    };
    value
        .checked_mul(nanos_per_unit)
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!("HEALTHCHECK duration {raw:?} overflowed"),
        })
}

fn split_first_word(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let split_at = trimmed
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index));
    match split_at {
        Some(index) => Some((&trimmed[..index], &trimmed[index..])),
        None => Some((trimmed, "")),
    }
}

fn parse_json_array(body: &str, keyword: &str) -> Result<Vec<String>> {
    serde_json::from_str::<Vec<String>>(body).map_err(|error| SandboxError::InvalidSpec {
        message: format!("failed to parse {keyword} JSON array {body:?}: {error}"),
    })
}

fn apply_copy_instruction(
    copy: &CopyInstruction,
    context_path: &Path,
    rootfs_path: &Path,
    image_config: &mut OciImageConfig,
) -> Result<()> {
    let destination_container_path =
        resolve_container_path(image_config.working_dir.as_deref(), &copy.destination)?;
    let destination_host_path = rootfs_path.join(
        destination_container_path
            .strip_prefix('/')
            .expect("container paths should stay absolute"),
    );
    let multiple_sources = copy.sources.len() > 1;
    let destination_is_dir_hint = copy.destination.ends_with('/');

    for source in &copy.sources {
        let source_path = resolve_context_source_path(context_path, source)?;
        let metadata = fs::metadata(&source_path).map_err(|error| SandboxError::InvalidSpec {
            message: format!(
                "failed to stat build context source {}: {error}",
                source_path.display()
            ),
        })?;
        if metadata.is_dir() {
            fs::create_dir_all(&destination_host_path).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to create COPY destination {}: {error}",
                        destination_host_path.display()
                    ),
                }
            })?;
            copy_directory_contents(&source_path, &destination_host_path)?;
            continue;
        }

        let final_destination =
            if multiple_sources || destination_is_dir_hint || destination_host_path.is_dir() {
                destination_host_path.join(source_path.file_name().ok_or_else(|| {
                    SandboxError::InvalidSpec {
                        message: format!(
                            "build context source {} does not have a file name",
                            source_path.display()
                        ),
                    }
                })?)
            } else {
                destination_host_path.clone()
            };
        copy_file_to_path(&source_path, &final_destination)?;
    }

    Ok(())
}

fn resolve_context_source_path(context_path: &Path, source: &str) -> Result<PathBuf> {
    if source.starts_with("http://") || source.starts_with("https://") {
        return Err(unsupported_instruction_error(
            "ADD",
            "remote URLs are not supported by the current internal guest builder",
        ));
    }
    if source.starts_with('/') {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "build context source {source:?} must be relative to the declared build context"
            ),
        });
    }
    if source.contains('*') || source.contains('?') || source.contains('[') {
        return Err(unsupported_instruction_error(
            "COPY",
            "globs are not supported by the current internal guest builder",
        ));
    }
    let relative = sanitize_relative_path(Path::new(source))?;
    let resolved = context_path.join(relative);
    if !resolved.exists() {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "build context source {} does not exist under {}",
                source,
                context_path.display()
            ),
        });
    }
    Ok(resolved)
}

fn sanitize_relative_path(path: &Path) -> Result<PathBuf> {
    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => sanitized.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SandboxError::InvalidSpec {
                    message: format!(
                        "build context path {:?} escapes the declared build context",
                        path
                    ),
                });
            }
        }
    }
    Ok(sanitized)
}

fn resolve_container_path(current_workdir: Option<&str>, raw: &str) -> Result<String> {
    let path = if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        Path::new(current_workdir.unwrap_or("/")).join(raw)
    };
    normalize_container_path(&path)
}

fn normalize_container_path(path: &Path) -> Result<String> {
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(SandboxError::InvalidSpec {
                        message: format!("container path {:?} escapes the rootfs", path),
                    });
                }
            }
            Component::Prefix(_) => {
                return Err(SandboxError::InvalidSpec {
                    message: format!("container path {:?} is invalid", path),
                });
            }
        }
    }
    Ok(normalized.to_string_lossy().into_owned())
}

fn copy_directory_contents(source_dir: &Path, destination_dir: &Path) -> Result<()> {
    for entry in fs::read_dir(source_dir).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read build context directory {}: {error}",
            source_dir.display()
        ),
    })? {
        let entry = entry.map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read build context entry in {}: {error}",
                source_dir.display()
            ),
        })?;
        copy_entry(&entry.path(), &destination_dir.join(entry.file_name()))?;
    }
    Ok(())
}

fn copy_entry(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::metadata(source).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to stat build context path {}: {error}",
            source.display()
        ),
    })?;
    if metadata.is_dir() {
        fs::create_dir_all(destination).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create copied directory {}: {error}",
                destination.display()
            ),
        })?;
        for entry in fs::read_dir(source).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read source directory {}: {error}",
                source.display()
            ),
        })? {
            let entry = entry.map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to iterate source directory {}: {error}",
                    source.display()
                ),
            })?;
            copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    copy_file_to_path(source, destination)
}

fn copy_file_to_path(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create copied file parent directory {}: {error}",
                parent.display()
            ),
        })?;
    }
    fs::copy(source, destination).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to copy build context file {} to {}: {error}",
            source.display(),
            destination.display()
        ),
    })?;
    let permissions = fs::metadata(source)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to stat copied file {}: {error}", source.display()),
        })?
        .permissions();
    fs::set_permissions(destination, permissions).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to preserve permissions on copied file {}: {error}",
            destination.display()
        ),
    })
}

fn merge_env_entries(env: &mut Vec<String>, entries: &[(String, String)]) {
    for (key, value) in entries {
        let entry = format!("{key}={value}");
        if let Some(index) = env.iter().position(|existing| {
            existing
                .split_once('=')
                .is_some_and(|(existing_key, _)| existing_key == key)
        }) {
            env[index] = entry;
        } else {
            env.push(entry);
        }
    }
}

fn synthetic_built_image_reference(image_name: &str) -> String {
    format!("localhost/{image_name}")
}

fn unsupported_instruction_error(instruction: &str, detail: &str) -> SandboxError {
    SandboxError::InvalidSpec {
        message: format!(
            "Dockerfile instruction {instruction:?} is not supported by the current internal guest builder: {detail}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Cursor, Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::thread;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    use super::OciDockerfileBuilder;
    use crate::backends::oci::buildah::OciExposedPortProtocol;
    use crate::instance::SandboxId;
    use crate::spec::SandboxImageProcessOverrides;

    #[test]
    fn builder_builds_from_scratch_with_copy_and_runtime_metadata() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(context_dir.join("bin")).expect("context dir should build");
        fs::write(context_dir.join("bin/server"), b"#!/bin/sh\nexit 0\n")
            .expect("server fixture should write");
        let dockerfile_path = context_dir.join("Dockerfile");
        fs::write(
            &dockerfile_path,
            r#"
FROM scratch
WORKDIR /app
ENV APP_ENV=dev LOG_LEVEL=info
COPY ./bin/server ./server
ENTRYPOINT ["/app/server"]
EXPOSE 8080
USER 1000:1000
STOPSIGNAL SIGQUIT
LABEL com.example.role=edge
HEALTHCHECK CMD ["/app/server", "--healthcheck"]
"#,
        )
        .expect("dockerfile should write");

        let builder = OciDockerfileBuilder::under_state_root(temp_dir.path());
        let prepared = builder
            .prepare_built_image_launch(
                &SandboxId::new("build-01"),
                "demo-build",
                &dockerfile_path,
                &context_dir,
                &SandboxImageProcessOverrides::default(),
            )
            .expect("scratch build should succeed");

        assert!(prepared.artifact.rootfs_path.join("app/server").is_file());
        assert_eq!(
            prepared.launch_defaults.process.args,
            vec!["/app/server".to_owned()]
        );
        assert_eq!(prepared.launch_defaults.process.cwd, PathBuf::from("/app"));
        assert_eq!(
            prepared.launch_defaults.process.env,
            vec!["APP_ENV=dev".to_owned(), "LOG_LEVEL=info".to_owned()]
        );
        assert_eq!(prepared.launch_defaults.user.as_deref(), Some("1000:1000"));
        assert_eq!(
            prepared.launch_defaults.stop_signal.as_deref(),
            Some("SIGQUIT")
        );
        assert_eq!(
            prepared
                .launch_defaults
                .labels
                .get("com.example.role")
                .map(String::as_str),
            Some("edge")
        );
        assert_eq!(prepared.launch_defaults.exposed_ports.len(), 1);
        assert_eq!(prepared.launch_defaults.exposed_ports[0].raw, "8080/tcp");
        assert_eq!(
            prepared.launch_defaults.exposed_ports[0].protocol,
            OciExposedPortProtocol::Tcp
        );
        assert_eq!(
            prepared
                .launch_defaults
                .healthcheck
                .as_ref()
                .expect("healthcheck should exist")
                .test,
            vec![
                "CMD".to_owned(),
                "/app/server".to_owned(),
                "--healthcheck".to_owned(),
            ]
        );
    }

    #[test]
    fn builder_layers_runtime_metadata_over_a_registry_base_image() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).expect("context dir should build");
        let registry = serve_fake_oci_registry(build_layer_archive());
        let dockerfile_path = context_dir.join("Dockerfile");
        fs::write(
            &dockerfile_path,
            format!(
                "FROM {registry}\nENV PORT=9090 APP_MODE=dev\nCMD [\"--custom\"]\nEXPOSE 9090\n"
            ),
        )
        .expect("dockerfile should write");

        let builder = OciDockerfileBuilder::under_state_root(temp_dir.path());
        let prepared = builder
            .prepare_built_image_launch(
                &SandboxId::new("build-02"),
                "demo-base",
                &dockerfile_path,
                &context_dir,
                &SandboxImageProcessOverrides::default(),
            )
            .expect("registry-backed build should succeed");

        assert!(prepared.artifact.rootfs_path.join("usr/bin/demo").is_file());
        assert_eq!(
            prepared.launch_defaults.process.args,
            vec!["/usr/bin/demo".to_owned(), "--custom".to_owned()]
        );
        assert_eq!(
            prepared.launch_defaults.process.env,
            vec![
                "PATH=/usr/bin".to_owned(),
                "PORT=9090".to_owned(),
                "APP_MODE=dev".to_owned(),
            ]
        );
        assert_eq!(
            prepared.launch_defaults.process.cwd,
            PathBuf::from("/workspace")
        );
        assert_eq!(prepared.launch_defaults.user.as_deref(), Some("1000:1000"));
        assert_eq!(
            prepared
                .launch_defaults
                .labels
                .get("app")
                .map(String::as_str),
            Some("demo")
        );
        assert_eq!(
            prepared
                .launch_defaults
                .exposed_ports
                .iter()
                .map(|port| port.raw.as_str())
                .collect::<Vec<_>>(),
            vec!["8080/tcp", "9090/tcp"]
        );
    }

    #[test]
    fn builder_rejects_run_instructions_cleanly() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).expect("context dir should build");
        let dockerfile_path = context_dir.join("Dockerfile");
        fs::write(
            &dockerfile_path,
            "FROM scratch\nRUN echo nope\nCMD [\"/bin/true\"]\n",
        )
        .expect("dockerfile should write");

        let builder = OciDockerfileBuilder::under_state_root(temp_dir.path());
        let error = builder
            .prepare_built_image_launch(
                &SandboxId::new("build-03"),
                "demo-run",
                &dockerfile_path,
                &context_dir,
                &SandboxImageProcessOverrides::default(),
            )
            .expect_err("RUN should be rejected");
        assert!(
            error
                .to_string()
                .contains("Dockerfile instruction \"RUN\" is not supported"),
            "{error}"
        );
    }

    fn build_layer_archive() -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(encoder);

        write_tar_file(
            &mut builder,
            "etc/passwd",
            b"demo:x:1000:1000:demo:/home/demo:/bin/sh\n",
            0o644,
        );
        write_tar_file(&mut builder, "etc/group", b"demo:x:1000:\n", 0o644);
        write_tar_file(
            &mut builder,
            "usr/bin/demo",
            b"#!/bin/sh\nexec sleep 60\n",
            0o755,
        );

        let encoder = builder.into_inner().expect("tar encoder should finish");
        encoder.finish().expect("gzip layer should finish")
    }

    fn write_tar_file(
        builder: &mut tar::Builder<GzEncoder<Vec<u8>>>,
        path: &str,
        body: &[u8],
        mode: u32,
    ) {
        let mut header = tar::Header::new_gnu();
        header.set_mode(mode);
        header.set_size(body.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, path, Cursor::new(body))
            .expect("layer entry should append");
    }

    fn serve_fake_oci_registry(layer_body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("registry listener should bind");
        let address = listener
            .local_addr()
            .expect("registry listener should report local addr");

        let config = serde_json::json!({
            "config": {
                "Entrypoint": ["/usr/bin/demo"],
                "Cmd": ["--serve"],
                "Env": ["PATH=/usr/bin", "PORT=8080"],
                "User": "demo",
                "WorkingDir": "/workspace",
                "ExposedPorts": {
                    "8080/tcp": {}
                },
                "Labels": {
                    "app": "demo"
                }
            }
        });
        let config_bytes = serde_json::to_vec(&config).expect("config should serialize");
        let config_digest = format!("sha256:{:x}", Sha256::digest(&config_bytes));
        let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_body));
        let child_manifest = serde_json::json!({
            "schemaVersion": 2,
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "size": config_bytes.len(),
                "digest": config_digest
            },
            "layers": [{
                "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                "size": layer_body.len(),
                "digest": layer_digest
            }]
        });
        let child_manifest_bytes =
            serde_json::to_vec(&child_manifest).expect("child manifest should serialize");
        let child_manifest_digest = format!("sha256:{:x}", Sha256::digest(&child_manifest_bytes));
        let index = serde_json::json!({
            "schemaVersion": 2,
            "manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": child_manifest_digest,
                "platform": {
                    "os": "linux",
                    "architecture": std::env::consts::ARCH
                }
            }]
        });
        let index_bytes = serde_json::to_vec(&index).expect("index should serialize");

        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.expect("registry connection should succeed");
                let mut request = [0_u8; 2048];
                let bytes_read = stream
                    .read(&mut request)
                    .expect("registry request should read");
                let request = String::from_utf8_lossy(&request[..bytes_read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");

                let (status, body) = match path {
                    "/v2/" => (200, Vec::new()),
                    "/v2/library/demo/manifests/latest" => (200, index_bytes.clone()),
                    path if path
                        == format!("/v2/library/demo/manifests/{child_manifest_digest}") =>
                    {
                        (200, child_manifest_bytes.clone())
                    }
                    path if path == format!("/v2/library/demo/blobs/{config_digest}") => {
                        (200, config_bytes.clone())
                    }
                    path if path == format!("/v2/library/demo/blobs/{layer_digest}") => {
                        (200, layer_body.clone())
                    }
                    _ => (404, Vec::new()),
                };

                let response = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    if status == 200 { "OK" } else { "Not Found" },
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("fake OCI registry response head should write");
                stream
                    .write_all(&body)
                    .expect("fake OCI registry response body should write");
            }
        });

        format!("docker://localhost:{}/library/demo:latest", address.port())
    }
}

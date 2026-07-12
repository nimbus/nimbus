use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use nimbus::Error;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::dirs::global_config_dir;
use crate::target_context::{TargetContext, TargetContextKind, TargetContextSource};

const TARGETS_FILE_NAME: &str = "targets";

/// On-disk shape of `~/.config/nimbus/targets`.
///
/// TOML, keyed by target name, each mapping to a Nimbus server URL. Mirrors the
/// credentials-file precedent (`credentials.rs`, DEP2): a name resolves to its
/// URL here, then credential lookup keys by that URL exactly as today. This is
/// the registry that backs `NamedTarget`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TargetsFile {
    #[serde(default)]
    pub(crate) target: BTreeMap<String, TargetEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TargetEntry {
    pub(crate) url: String,
}

/// Resolve `~/.config/nimbus/targets`, honoring `XDG_CONFIG_HOME`.
pub(crate) fn default_targets_path() -> Result<PathBuf, Error> {
    Ok(global_config_dir()?.join(TARGETS_FILE_NAME))
}

/// Human-readable spelling of the targets file for banners and errors. Falls
/// back to the canonical `~/.config/nimbus/targets` when the home directory
/// cannot be resolved (the error path prints the same words the docs use).
pub(crate) fn targets_path_display() -> String {
    default_targets_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "~/.config/nimbus/targets".to_owned())
}

pub(crate) fn read_targets_file(path: &Path) -> Result<TargetsFile, Error> {
    let bytes = match fs::read_to_string(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TargetsFile::default());
        }
        Err(error) => {
            return Err(Error::Internal(format!(
                "failed to read targets file {}: {error}",
                path.display()
            )));
        }
    };
    toml::from_str(&bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "targets file {} is not valid TOML: {error}",
            path.display()
        ))
    })
}

pub(crate) fn write_targets_file(path: &Path, file: &TargetsFile) -> Result<(), Error> {
    let parent = path.parent().ok_or_else(|| {
        Error::InvalidInput(format!(
            "targets path {} does not have a parent directory",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create config directory {}: {error}",
            parent.display()
        ))
    })?;
    let bytes = toml::to_string_pretty(file).map_err(|error| {
        Error::Internal(format!(
            "failed to serialize targets file {}: {error}",
            path.display()
        ))
    })?;
    let mut temp_file = NamedTempFile::new_in(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to open a temp file next to {}: {error}",
            path.display()
        ))
    })?;
    temp_file
        .write_all(bytes.as_bytes())
        .and_then(|()| temp_file.flush())
        .and_then(|()| temp_file.as_file().sync_all())
        .map_err(|error| Error::Internal(format!("failed to write targets file bytes: {error}")))?;
    temp_file.into_temp_path().persist(path).map_err(|error| {
        Error::Internal(format!(
            "failed to atomically replace {}: {}",
            path.display(),
            error.error
        ))
    })?;
    Ok(())
}

/// Validate a target name: non-empty, no whitespace, and not URL-shaped (so a
/// name can never be confused with the positional URL form).
pub(crate) fn validate_target_name(name: &str) -> Result<String, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::InvalidInput(
            "target name cannot be empty".to_owned(),
        ));
    }
    if name.contains(char::is_whitespace) {
        return Err(Error::InvalidInput(format!(
            "target name {name:?} cannot contain whitespace"
        )));
    }
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Err(Error::InvalidInput(format!(
            "target name {name:?} must not look like a URL; a name maps to a URL"
        )));
    }
    Ok(name.to_owned())
}

/// Validate and normalize a target URL: `http`/`https` only, trailing slash
/// trimmed so it matches the credential lookup key exactly.
pub(crate) fn validate_target_url(url: &str) -> Result<String, Error> {
    let url = url.trim();
    let parsed = reqwest::Url::parse(url)
        .map_err(|error| Error::InvalidInput(format!("target URL {url:?} is invalid: {error}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(Error::InvalidInput(format!(
                "target URL scheme {scheme:?} is unsupported; use http or https"
            )));
        }
    }
    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

pub(crate) fn upsert_target(file: &mut TargetsFile, name: &str, url: &str) -> Result<(), Error> {
    let name = validate_target_name(name)?;
    let url = validate_target_url(url)?;
    file.target.insert(name, TargetEntry { url });
    Ok(())
}

pub(crate) fn remove_target(file: &mut TargetsFile, name: &str) -> bool {
    file.target.remove(name.trim()).is_some()
}

pub(crate) fn find_target<'a>(file: &'a TargetsFile, name: &str) -> Option<&'a TargetEntry> {
    file.target.get(name.trim())
}

/// Resolve a `NamedTarget` to its configured URL from the default targets file.
/// A missing name fails with an error that names the targets file and the
/// `nimbus target add` command, so an unconfigured bare-word target is never a
/// silent dead end.
pub(crate) fn resolve_named_target_url(name: &str) -> Result<String, Error> {
    resolve_named_target_url_at(&default_targets_path()?, name)
}

pub(crate) fn resolve_named_target_url_at(path: &Path, name: &str) -> Result<String, Error> {
    let file = read_targets_file(path)?;
    match find_target(&file, name) {
        Some(entry) => Ok(entry.url.trim_end_matches('/').to_owned()),
        None => Err(Error::InvalidInput(format!(
            "target `{name}` is not configured in {}; add it with `nimbus target add {name} <url>` or pass a TARGET URL",
            path.display()
        ))),
    }
}

/// The prominent line every target-taking command prints before acting, so the
/// destination is explicit in the output even when it was implicit in the
/// invocation (XD4 mitigation). `verb` is the command's phrasing, e.g.
/// `"Deploying to"` or `"Running against"`; `resolved_url` is the concrete
/// endpoint the command will hit.
pub(crate) fn resolved_target_banner(
    verb: &str,
    context: &TargetContext,
    resolved_url: &str,
) -> String {
    match (&context.kind, context.source) {
        (TargetContextKind::LocalDiscovery, _) => {
            format!("{verb} LOCAL {resolved_url} (no TARGET given)")
        }
        (TargetContextKind::NamedTarget(name), TargetContextSource::EnvironmentTarget) => {
            format!("{verb} {name} ({resolved_url}, from NIMBUS_TARGET)")
        }
        (TargetContextKind::NamedTarget(name), _) => {
            format!(
                "{verb} {name} ({resolved_url}, from {})",
                targets_path_display()
            )
        }
        (TargetContextKind::RemoteUrl(_), TargetContextSource::EnvironmentTargetUrl) => {
            format!("{verb} {resolved_url} (from NIMBUS_TARGET_URL)")
        }
        (TargetContextKind::RemoteUrl(_), _) => {
            format!("{verb} {resolved_url} (from TARGET)")
        }
    }
}

/// Manage the named-target registry in `~/.config/nimbus/targets`.
#[derive(Debug, Subcommand)]
pub(crate) enum TargetCommand {
    /// Map a name to a Nimbus server URL (overwrites an existing name).
    Add(TargetAddCommand),
    /// List configured targets.
    List(TargetListCommand),
    /// Remove a configured target by name.
    Remove(TargetRemoveCommand),
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct TargetAddCommand {
    /// Target name used as the positional TARGET on other commands.
    #[arg(value_name = "NAME")]
    pub(crate) name: String,
    /// Nimbus server URL the name resolves to (http or https).
    #[arg(value_name = "URL")]
    pub(crate) url: String,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct TargetListCommand {}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct TargetRemoveCommand {
    /// Target name to remove.
    #[arg(value_name = "NAME")]
    pub(crate) name: String,
}

pub(crate) fn run_target_command(command: TargetCommand) -> Result<(), Error> {
    let path = default_targets_path()?;
    let mut out = std::io::stdout().lock();
    run_target_command_at(command, &path, &mut out)
}

/// Execute a `target` subcommand against an explicit registry path, writing all
/// human output to `out`. The path and sink are injected so tests exercise the
/// real registry read/write and assert stdout without touching the machine's
/// `~/.config/nimbus/targets`.
fn run_target_command_at(
    command: TargetCommand,
    path: &Path,
    out: &mut impl Write,
) -> Result<(), Error> {
    let emit = |out: &mut dyn Write, line: &str| -> Result<(), Error> {
        writeln!(out, "{line}")
            .map_err(|error| Error::Internal(format!("failed to write target output: {error}")))
    };
    match command {
        TargetCommand::Add(add) => {
            let mut file = read_targets_file(path)?;
            upsert_target(&mut file, &add.name, &add.url)?;
            write_targets_file(path, &file)?;
            let name = validate_target_name(&add.name)?;
            let url = validate_target_url(&add.url)?;
            emit(
                out,
                &format!("Added target {name} -> {url} in {}", path.display()),
            )
        }
        TargetCommand::List(_) => {
            let file = read_targets_file(path)?;
            if file.target.is_empty() {
                return emit(
                    out,
                    &format!(
                        "No targets configured in {}. Add one with `nimbus target add <name> <url>`.",
                        path.display()
                    ),
                );
            }
            let width = file.target.keys().map(String::len).max().unwrap_or(0);
            for (name, entry) in &file.target {
                emit(
                    out,
                    &format!("{name:<width$}  {}", entry.url, width = width),
                )?;
            }
            Ok(())
        }
        TargetCommand::Remove(remove) => {
            let name = remove.name.trim().to_owned();
            let mut file = read_targets_file(path)?;
            if remove_target(&mut file, &name) {
                write_targets_file(path, &file)?;
                emit(
                    out,
                    &format!("Removed target {name} from {}", path.display()),
                )
            } else {
                Err(Error::InvalidInput(format!(
                    "target `{name}` is not configured in {}",
                    path.display()
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target_context::{TargetContext, TargetContextKind, TargetContextSource};

    fn temp_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("targets");
        (dir, path)
    }

    #[test]
    fn add_list_remove_round_trips_through_disk() {
        let (_dir, path) = temp_path();
        let mut file = read_targets_file(&path).expect("missing file reads as empty");
        assert!(file.target.is_empty());

        upsert_target(&mut file, "prod", "https://nimbus.example.com/").expect("add should work");
        upsert_target(&mut file, "staging", "http://localhost:3210").expect("add should work");
        write_targets_file(&path, &file).expect("write should work");

        let loaded = read_targets_file(&path).expect("read should work");
        assert_eq!(loaded.target.len(), 2);
        assert_eq!(
            find_target(&loaded, "prod").map(|entry| entry.url.as_str()),
            Some("https://nimbus.example.com"),
            "trailing slash should be normalized away to match the credential key"
        );
        assert_eq!(
            find_target(&loaded, "staging").map(|entry| entry.url.as_str()),
            Some("http://localhost:3210")
        );

        let mut loaded = loaded;
        assert!(remove_target(&mut loaded, "prod"));
        assert!(!remove_target(&mut loaded, "prod"));
        assert!(find_target(&loaded, "prod").is_none());
        assert!(find_target(&loaded, "staging").is_some());
    }

    #[test]
    fn upsert_overwrites_existing_name() {
        let mut file = TargetsFile::default();
        upsert_target(&mut file, "prod", "https://one.example.com").unwrap();
        upsert_target(&mut file, "prod", "https://two.example.com").unwrap();
        assert_eq!(file.target.len(), 1);
        assert_eq!(
            find_target(&file, "prod").map(|entry| entry.url.as_str()),
            Some("https://two.example.com")
        );
    }

    #[test]
    fn add_rejects_non_http_url() {
        let mut file = TargetsFile::default();
        let error = upsert_target(&mut file, "prod", "ftp://nimbus.example.com")
            .expect_err("non-http scheme should reject");
        assert!(error.to_string().contains("unsupported"), "{error}");
    }

    #[test]
    fn add_rejects_url_shaped_name() {
        let mut file = TargetsFile::default();
        let error = upsert_target(&mut file, "https://oops", "https://nimbus.example.com")
            .expect_err("a URL-shaped name should reject");
        assert!(
            error.to_string().contains("must not look like a URL"),
            "{error}"
        );
    }

    #[test]
    fn add_rejects_whitespace_name() {
        let mut file = TargetsFile::default();
        let error = upsert_target(&mut file, "has space", "https://nimbus.example.com")
            .expect_err("a whitespace name should reject");
        assert!(error.to_string().contains("whitespace"), "{error}");
    }

    #[test]
    fn resolve_reads_configured_url_from_disk() {
        let (_dir, path) = temp_path();
        let mut file = TargetsFile::default();
        upsert_target(&mut file, "prod", "https://nimbus.example.com/").unwrap();
        write_targets_file(&path, &file).expect("write file");

        let url =
            resolve_named_target_url_at(&path, "prod").expect("configured name should resolve");
        assert_eq!(url, "https://nimbus.example.com");
    }

    #[test]
    fn resolve_missing_name_names_file_and_add_command() {
        let (_dir, path) = temp_path();
        write_targets_file(&path, &TargetsFile::default()).expect("write empty file");

        let error = resolve_named_target_url_at(&path, "ghost")
            .expect_err("an unconfigured name must fail");
        let message = error.to_string();
        assert!(
            message.contains("nimbus target add ghost"),
            "error should name the add command: {message}"
        );
        assert!(
            message.contains("targets"),
            "error should name the targets file: {message}"
        );
    }

    #[test]
    fn target_command_writes_toml_shape_and_stdout() {
        let (_dir, path) = temp_path();

        // list on an empty registry names the file and the add command.
        let mut empty = Vec::new();
        run_target_command_at(TargetCommand::List(TargetListCommand {}), &path, &mut empty)
            .expect("list of an empty registry should succeed");
        let empty = String::from_utf8(empty).unwrap();
        assert_eq!(
            empty.trim_end(),
            format!(
                "No targets configured in {}. Add one with `nimbus target add <name> <url>`.",
                path.display()
            )
        );

        // add writes the on-disk TOML shape and confirms on stdout.
        let mut added = Vec::new();
        run_target_command_at(
            TargetCommand::Add(TargetAddCommand {
                name: "prod".to_owned(),
                url: "https://nimbus.example.com/".to_owned(),
            }),
            &path,
            &mut added,
        )
        .expect("add should succeed");
        let added = String::from_utf8(added).unwrap();
        assert_eq!(
            added.trim_end(),
            format!(
                "Added target prod -> https://nimbus.example.com in {}",
                path.display()
            )
        );
        let toml = fs::read_to_string(&path).expect("targets file should exist after add");
        assert!(
            toml.contains("[target.prod]"),
            "registry TOML must carry a [target.<name>] table: {toml}"
        );
        assert!(
            toml.contains("url = \"https://nimbus.example.com\""),
            "registry TOML must store the normalized url: {toml}"
        );

        // list prints the name/url row.
        let mut listed = Vec::new();
        run_target_command_at(
            TargetCommand::List(TargetListCommand {}),
            &path,
            &mut listed,
        )
        .expect("list should succeed");
        let listed = String::from_utf8(listed).unwrap();
        assert_eq!(listed.trim_end(), "prod  https://nimbus.example.com");

        // remove deletes the entry and confirms on stdout.
        let mut removed = Vec::new();
        run_target_command_at(
            TargetCommand::Remove(TargetRemoveCommand {
                name: "prod".to_owned(),
            }),
            &path,
            &mut removed,
        )
        .expect("remove should succeed");
        let removed = String::from_utf8(removed).unwrap();
        assert_eq!(
            removed.trim_end(),
            format!("Removed target prod from {}", path.display())
        );
        assert!(
            read_targets_file(&path).unwrap().target.is_empty(),
            "registry should be empty after removing the only target"
        );

        // removing a missing name is an error that names the file.
        let mut missing = Vec::new();
        let error = run_target_command_at(
            TargetCommand::Remove(TargetRemoveCommand {
                name: "ghost".to_owned(),
            }),
            &path,
            &mut missing,
        )
        .expect_err("removing an unconfigured name must fail");
        assert!(
            error.to_string().contains("is not configured in"),
            "{error}"
        );
    }

    #[test]
    fn target_subcommands_parse() {
        use clap::Parser;

        use crate::{Cli, Command};

        let add = Cli::parse_from([
            "nimbus",
            "target",
            "add",
            "prod",
            "https://nimbus.example.com",
        ]);
        let Command::Target(TargetCommand::Add(add)) = add.command else {
            panic!("target add should parse");
        };
        assert_eq!(add.name, "prod");
        assert_eq!(add.url, "https://nimbus.example.com");

        let list = Cli::parse_from(["nimbus", "target", "list"]);
        assert!(matches!(
            list.command,
            Command::Target(TargetCommand::List(_))
        ));

        let remove = Cli::parse_from(["nimbus", "target", "remove", "prod"]);
        let Command::Target(TargetCommand::Remove(remove)) = remove.command else {
            panic!("target remove should parse");
        };
        assert_eq!(remove.name, "prod");
    }

    #[test]
    fn banner_names_local_named_and_url_sources() {
        let local = TargetContext {
            kind: TargetContextKind::LocalDiscovery,
            source: TargetContextSource::ImplicitLocalDefault,
        };
        assert_eq!(
            resolved_target_banner("Deploying to", &local, "http://127.0.0.1:3210"),
            "Deploying to LOCAL http://127.0.0.1:3210 (no TARGET given)"
        );

        let named = TargetContext {
            kind: TargetContextKind::NamedTarget("prod".to_owned()),
            source: TargetContextSource::PositionalName,
        };
        let line = resolved_target_banner("Deploying to", &named, "https://nimbus.example.com");
        assert!(
            line.starts_with("Deploying to prod (https://nimbus.example.com, from "),
            "{line}"
        );
        assert!(line.contains("targets)"), "{line}");

        let env_named = TargetContext {
            kind: TargetContextKind::NamedTarget("prod".to_owned()),
            source: TargetContextSource::EnvironmentTarget,
        };
        assert_eq!(
            resolved_target_banner("Running against", &env_named, "https://nimbus.example.com"),
            "Running against prod (https://nimbus.example.com, from NIMBUS_TARGET)"
        );

        let url = TargetContext {
            kind: TargetContextKind::RemoteUrl("https://nimbus.example.com".to_owned()),
            source: TargetContextSource::PositionalUrl,
        };
        assert_eq!(
            resolved_target_banner("Deploying to", &url, "https://nimbus.example.com"),
            "Deploying to https://nimbus.example.com (from TARGET)"
        );

        let env_url = TargetContext {
            kind: TargetContextKind::RemoteUrl("https://nimbus.example.com".to_owned()),
            source: TargetContextSource::EnvironmentTargetUrl,
        };
        assert_eq!(
            resolved_target_banner("Deploying to", &env_url, "https://nimbus.example.com"),
            "Deploying to https://nimbus.example.com (from NIMBUS_TARGET_URL)"
        );
    }
}

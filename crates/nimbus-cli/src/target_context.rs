use clap::Args;
use nimbus::Error;

pub(crate) const TARGET_ENV: &str = "NIMBUS_TARGET";
pub(crate) const DEPLOY_URL_ENV: &str = "NIMBUS_DEPLOY_URL";

/// One shared vocabulary for every target-taking command's positional help.
pub(crate) const TARGET_ARG_HELP: &str =
    "TARGET is a URL or a configured target name; omitted = local";

/// One optional positional target shared by every command that acts on a
/// Nimbus resource. A `http`/`https` value resolves to a remote URL; any other
/// value resolves to a configured target name; omitting it resolves to the
/// local server, exactly like `nimbus dev`. Whether the resolved resource is a
/// single node or a cluster is invisible here — it is just a Nimbus resource.
#[derive(Debug, Clone, Args)]
pub(crate) struct TargetSelector {
    #[arg(value_name = "TARGET", help = TARGET_ARG_HELP)]
    pub(crate) target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TargetContext {
    pub(crate) kind: TargetContextKind,
    pub(crate) source: TargetContextSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TargetContextKind {
    LocalDiscovery,
    NamedTarget(String),
    RemoteUrl(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetContextSource {
    ImplicitLocalDefault,
    PositionalName,
    PositionalUrl,
    EnvironmentTarget,
    EnvironmentDeployUrl,
}

impl TargetSelector {
    /// Resolve the positional target to a concrete [`TargetContext`].
    ///
    /// Precedence: an explicit positional wins outright; otherwise the
    /// `NIMBUS_TARGET` / `NIMBUS_DEPLOY_URL` env fallbacks apply (exactly one,
    /// or an ambiguity error if both are set); otherwise the command targets
    /// the local server.
    pub(crate) fn resolve(
        &self,
        command_name: &str,
        env_lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<TargetContext, Error> {
        if let Some(raw) = self.target.as_deref() {
            return TargetCandidate::from_positional(raw).map(|candidate| candidate.context);
        }

        let env_target = env_lookup(TARGET_ENV);
        let env_url = env_lookup(DEPLOY_URL_ENV);
        match (env_target, env_url) {
            (Some(_), Some(_)) => Err(Error::InvalidInput(format!(
                "nimbus {command_name} found both {TARGET_ENV} and {DEPLOY_URL_ENV}; set exactly one, or pass TARGET explicitly"
            ))),
            (Some(name), None) => {
                TargetCandidate::named(&name, TargetContextSource::EnvironmentTarget)
                    .map(|candidate| candidate.context)
            }
            (None, Some(url)) => {
                TargetCandidate::url(&url, TargetContextSource::EnvironmentDeployUrl)
                    .map(|candidate| candidate.context)
            }
            (None, None) => Ok(TargetContext {
                kind: TargetContextKind::LocalDiscovery,
                source: TargetContextSource::ImplicitLocalDefault,
            }),
        }
    }
}

/// A positional target is URL-shaped when it carries an explicit `http`/`https`
/// scheme; every other value is a configured target name.
fn looks_like_url(raw: &str) -> bool {
    let lower = raw.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

#[derive(Debug, Clone)]
struct TargetCandidate {
    context: TargetContext,
}

impl TargetCandidate {
    fn from_positional(raw: &str) -> Result<Self, Error> {
        if looks_like_url(raw) {
            Self::url(raw, TargetContextSource::PositionalUrl)
        } else {
            Self::named(raw, TargetContextSource::PositionalName)
        }
    }

    fn named(target: &str, source: TargetContextSource) -> Result<Self, Error> {
        let target = target.trim();
        if target.is_empty() {
            return Err(Error::InvalidInput(
                "target name cannot be empty".to_owned(),
            ));
        }
        if target.contains(char::is_whitespace) {
            return Err(Error::InvalidInput(format!(
                "target name {target:?} cannot contain whitespace"
            )));
        }
        Ok(Self {
            context: TargetContext {
                kind: TargetContextKind::NamedTarget(target.to_owned()),
                source,
            },
        })
    }

    fn url(url: &str, source: TargetContextSource) -> Result<Self, Error> {
        let url = url.trim();
        let parsed = reqwest::Url::parse(url).map_err(|error| {
            Error::InvalidInput(format!("target URL {url:?} is invalid: {error}"))
        })?;
        match parsed.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(Error::InvalidInput(format!(
                    "target URL scheme {scheme:?} is unsupported; use http or https"
                )));
            }
        }
        Ok(Self {
            context: TargetContext {
                kind: TargetContextKind::RemoteUrl(parsed.to_string()),
                source,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(target: Option<&str>) -> TargetSelector {
        TargetSelector {
            target: target.map(str::to_owned),
        }
    }

    #[test]
    fn absent_target_resolves_local_discovery() {
        let context = selector(None)
            .resolve("run", |_| None)
            .expect("absent target should resolve to local discovery");

        assert_eq!(context.kind, TargetContextKind::LocalDiscovery);
        assert_eq!(context.source, TargetContextSource::ImplicitLocalDefault);
    }

    #[test]
    fn url_shaped_positional_resolves_remote_url() {
        let context = selector(Some("https://nimbus.example.test"))
            .resolve("deploy", |_| None)
            .expect("url-shaped target should resolve to a remote url");

        assert_eq!(
            context.kind,
            TargetContextKind::RemoteUrl("https://nimbus.example.test/".to_owned())
        );
        assert_eq!(context.source, TargetContextSource::PositionalUrl);
    }

    #[test]
    fn non_url_positional_resolves_named_target() {
        let context = selector(Some("prod"))
            .resolve("deploy", |_| None)
            .expect("non-url target should resolve to a named target");

        assert_eq!(
            context.kind,
            TargetContextKind::NamedTarget("prod".to_owned())
        );
        assert_eq!(context.source, TargetContextSource::PositionalName);
    }

    #[test]
    fn named_target_rejects_whitespace() {
        let error = selector(Some("has space"))
            .resolve("run", |_| None)
            .expect_err("whitespace in a target name should fail");

        assert!(
            error.to_string().contains("cannot contain whitespace"),
            "error should explain the whitespace rule: {error}"
        );
    }

    #[test]
    fn empty_positional_rejects() {
        let error = selector(Some("   "))
            .resolve("run", |_| None)
            .expect_err("an all-whitespace target should fail");

        assert!(
            error.to_string().contains("cannot be empty"),
            "error should explain the empty rule: {error}"
        );
    }

    #[test]
    fn env_target_resolves_named_target_when_positional_absent() {
        let context = selector(None)
            .resolve("sandbox", |name| {
                (name == TARGET_ENV).then(|| "developer-machine".to_owned())
            })
            .expect("env target should resolve");

        assert_eq!(
            context.kind,
            TargetContextKind::NamedTarget("developer-machine".to_owned())
        );
        assert_eq!(context.source, TargetContextSource::EnvironmentTarget);
    }

    #[test]
    fn env_deploy_url_resolves_remote_url_when_positional_absent() {
        let context = selector(None)
            .resolve("deploy", |name| {
                (name == DEPLOY_URL_ENV).then(|| "http://localhost:3210".to_owned())
            })
            .expect("env deploy url should resolve");

        assert_eq!(
            context.kind,
            TargetContextKind::RemoteUrl("http://localhost:3210/".to_owned())
        );
        assert_eq!(context.source, TargetContextSource::EnvironmentDeployUrl);
    }

    #[test]
    fn positional_wins_over_env_fallbacks() {
        let context = selector(Some("prod"))
            .resolve("deploy", |name| {
                (name == DEPLOY_URL_ENV).then(|| "http://localhost:3210".to_owned())
            })
            .expect("explicit positional should override env");

        assert_eq!(
            context.kind,
            TargetContextKind::NamedTarget("prod".to_owned())
        );
        assert_eq!(context.source, TargetContextSource::PositionalName);
    }

    #[test]
    fn ambiguous_env_sources_reject() {
        let error = selector(None)
            .resolve("run", |name| match name {
                TARGET_ENV => Some("prod".to_owned()),
                DEPLOY_URL_ENV => Some("http://localhost:3210".to_owned()),
                _ => None,
            })
            .expect_err("both env sources set should fail");

        assert!(
            error.to_string().contains("set exactly one"),
            "error should explain the ambiguity: {error}"
        );
    }
}

use clap::Args;
use nimbus::Error;

pub(crate) const TARGET_ENV: &str = "NIMBUS_TARGET";
pub(crate) const DEPLOY_URL_ENV: &str = "NIMBUS_DEPLOY_URL";

#[derive(Debug, Clone, Args)]
pub(crate) struct TargetSelector {
    /// Resolve the command against the currently running local Nimbus server.
    #[arg(long)]
    pub(crate) local: bool,

    /// Named Nimbus target configured for this machine.
    #[arg(long, value_name = "TARGET")]
    pub(crate) target: Option<String>,

    /// Explicit Nimbus server URL for this command.
    #[arg(long, value_name = "URL")]
    pub(crate) url: Option<String>,
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
    ExplicitLocalFlag,
    ExplicitTargetFlag,
    ExplicitUrlFlag,
    EnvironmentTarget,
    EnvironmentDeployUrl,
}

impl TargetSelector {
    pub(crate) fn resolve(
        &self,
        command_name: &str,
        env_lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<TargetContext, Error> {
        let mut sources = Vec::new();
        if self.local {
            sources.push(TargetCandidate::local());
        }
        if let Some(target) = self.target.as_deref() {
            sources.push(TargetCandidate::named(
                target,
                TargetContextSource::ExplicitTargetFlag,
            )?);
        }
        if let Some(url) = self.url.as_deref() {
            sources.push(TargetCandidate::url(
                url,
                TargetContextSource::ExplicitUrlFlag,
            )?);
        }
        if sources.is_empty() {
            if let Some(target) = env_lookup(TARGET_ENV) {
                sources.push(TargetCandidate::named(
                    &target,
                    TargetContextSource::EnvironmentTarget,
                )?);
            }
        }
        if sources.is_empty() {
            if let Some(url) = env_lookup(DEPLOY_URL_ENV) {
                sources.push(TargetCandidate::url(
                    &url,
                    TargetContextSource::EnvironmentDeployUrl,
                )?);
            }
        }
        match sources.as_slice() {
            [candidate] => Ok(candidate.context.clone()),
            [] => Err(Error::InvalidInput(format!(
                "nimbus {command_name} requires --local, --target, --url, {TARGET_ENV}, or {DEPLOY_URL_ENV}"
            ))),
            _ => Err(Error::InvalidInput(format!(
                "nimbus {command_name} accepts exactly one target source; use one of --local, --target, or --url"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
struct TargetCandidate {
    context: TargetContext,
}

impl TargetCandidate {
    fn local() -> Self {
        Self {
            context: TargetContext {
                kind: TargetContextKind::LocalDiscovery,
                source: TargetContextSource::ExplicitLocalFlag,
            },
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

    #[test]
    fn target_context_resolves_local_discovery() {
        let selector = TargetSelector {
            local: true,
            target: None,
            url: None,
        };

        let context = selector
            .resolve("run", |_| None)
            .expect("local target should resolve");

        assert_eq!(context.kind, TargetContextKind::LocalDiscovery);
        assert_eq!(context.source, TargetContextSource::ExplicitLocalFlag);
    }

    #[test]
    fn target_context_resolves_named_target_from_env() {
        let selector = TargetSelector {
            local: false,
            target: None,
            url: None,
        };

        let context = selector
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
    fn target_context_rejects_ambiguous_sources() {
        let selector = TargetSelector {
            local: true,
            target: Some("dev".to_owned()),
            url: None,
        };

        let error = selector
            .resolve("run", |_| None)
            .expect_err("multiple explicit target sources should fail");

        assert!(
            error.to_string().contains("exactly one target source"),
            "error should explain the ambiguity: {error}"
        );
    }
}

use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawComposeDocument {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) services: BTreeMap<String, RawComposeService>,
    #[serde(default)]
    pub(super) volumes: BTreeMap<String, Value>,
    #[serde(default)]
    pub(super) networks: BTreeMap<String, Value>,
    #[serde(default)]
    pub(super) configs: BTreeMap<String, Value>,
    #[serde(default)]
    pub(super) secrets: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub(super) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeService {
    #[serde(default)]
    pub(super) image: Option<String>,
    #[serde(default)]
    pub(super) build: Option<RawComposeBuild>,
    #[serde(default)]
    pub(super) environment: Option<RawComposeStringMap>,
    #[serde(default)]
    pub(super) env_file: Option<RawComposeEnvFile>,
    #[serde(default)]
    pub(super) ports: Vec<String>,
    #[serde(default)]
    pub(super) volumes: Vec<RawComposeVolumeMount>,
    #[serde(default)]
    pub(super) deploy: Option<RawComposeDeploy>,
    #[serde(default)]
    pub(super) restart: Option<String>,
    #[serde(default)]
    pub(super) depends_on: Option<RawComposeDependsOn>,
    #[serde(default)]
    pub(super) stop_grace_period: Option<String>,
    #[serde(default)]
    pub(super) command: Option<ComposeCommandPlan>,
    #[serde(default)]
    pub(super) entrypoint: Option<ComposeCommandPlan>,
    #[serde(default)]
    pub(super) user: Option<String>,
    #[serde(default)]
    pub(super) working_dir: Option<String>,
    #[serde(default)]
    pub(super) labels: Option<RawComposeStringMap>,
    #[serde(default)]
    pub(super) healthcheck: Option<RawComposeHealthcheck>,
    #[serde(default)]
    pub(super) x_neovex: Option<ComposeNeovexPlan>,
    #[serde(default)]
    pub(super) networks: Option<Value>,
    #[serde(default)]
    pub(super) configs: Option<Value>,
    #[serde(default)]
    pub(super) secrets: Option<Value>,
    #[serde(default)]
    pub(super) cap_add: Option<Value>,
    #[serde(default)]
    pub(super) cap_drop: Option<Value>,
    #[serde(default)]
    pub(super) privileged: Option<bool>,
    #[serde(default)]
    pub(super) logging: Option<Value>,
    #[serde(default, flatten)]
    pub(super) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum RawComposeBuild {
    Context(String),
    Detail(RawComposeBuildDetail),
}

impl RawComposeBuild {
    pub(super) fn resolve_paths(&self, compose_dir: &Path) -> Result<(PathBuf, PathBuf), Error> {
        let (context, dockerfile) = match self {
            Self::Context(context) => (PathBuf::from(context), PathBuf::from("Dockerfile")),
            Self::Detail(detail) => (
                PathBuf::from(detail.context.clone().unwrap_or_else(|| ".".to_owned())),
                PathBuf::from(
                    detail
                        .dockerfile
                        .clone()
                        .unwrap_or_else(|| "Dockerfile".to_owned()),
                ),
            ),
        };

        let context_path = compose_dir.join(context);
        let dockerfile_path = if dockerfile.is_absolute() {
            dockerfile
        } else {
            context_path.join(dockerfile)
        };
        Ok((context_path, dockerfile_path))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeBuildDetail {
    #[serde(default)]
    pub(super) context: Option<String>,
    #[serde(default)]
    pub(super) dockerfile: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum RawComposeStringMap {
    List(Vec<String>),
    Map(BTreeMap<String, Option<Value>>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum RawComposeEnvFile {
    Single(String),
    List(Vec<RawComposeEnvFileEntry>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum RawComposeEnvFileEntry {
    Path(String),
    Detail(RawComposeEnvFileDetail),
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeEnvFileDetail {
    pub(super) path: String,
    #[serde(default)]
    pub(super) required: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum RawComposeDependsOn {
    List(Vec<String>),
    Map(BTreeMap<String, RawComposeDependencyDetail>),
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeDependencyDetail {
    #[serde(default)]
    pub(super) condition: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeDeploy {
    #[serde(default)]
    pub(super) resources: Option<RawComposeResources>,
    #[serde(default)]
    pub(super) replicas: Option<Value>,
    #[serde(default)]
    pub(super) placement: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeResources {
    #[serde(default)]
    pub(super) limits: Option<RawComposeResourceLimits>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeResourceLimits {
    #[serde(default)]
    pub(super) cpus: Option<String>,
    #[serde(default)]
    pub(super) memory: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeHealthcheck {
    #[serde(default)]
    pub(super) test: Option<ComposeCommandPlan>,
    #[serde(default)]
    pub(super) interval: Option<String>,
    #[serde(default)]
    pub(super) timeout: Option<String>,
    #[serde(default)]
    pub(super) retries: Option<u32>,
    #[serde(default)]
    pub(super) start_period: Option<String>,
    #[serde(default)]
    pub(super) disable: Option<bool>,
    #[serde(default, flatten)]
    pub(super) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum RawComposeVolumeMount {
    Short(String),
    Long(RawComposeVolumeMountDetail),
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeVolumeMountDetail {
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
    #[serde(default)]
    pub(super) source: Option<String>,
    pub(super) target: String,
    #[serde(default)]
    pub(super) read_only: Option<bool>,
}

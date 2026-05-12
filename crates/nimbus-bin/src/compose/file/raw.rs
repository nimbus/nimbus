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

impl RawComposeDocument {
    pub(super) fn merge_from(&mut self, overlay: Self) {
        if overlay.name.is_some() {
            self.name = overlay.name;
        }
        merge_named_entries(&mut self.services, overlay.services, |base, overlay| {
            base.merge_from(overlay);
        });
        self.volumes.extend(overlay.volumes);
        self.networks.extend(overlay.networks);
        self.configs.extend(overlay.configs);
        self.secrets.extend(overlay.secrets);
        self.extra.extend(overlay.extra);
    }
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
    pub(super) x_nimbus: Option<ComposeNimbusPlan>,
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

impl RawComposeService {
    fn merge_from(&mut self, overlay: Self) {
        replace_if_some(&mut self.image, overlay.image);
        replace_if_some(&mut self.build, overlay.build);
        merge_optional_string_map(&mut self.environment, overlay.environment);
        merge_optional_env_file(&mut self.env_file, overlay.env_file);
        self.ports.extend(overlay.ports);
        self.volumes.extend(overlay.volumes);
        merge_optional_deploy(&mut self.deploy, overlay.deploy);
        replace_if_some(&mut self.restart, overlay.restart);
        merge_optional_depends_on(&mut self.depends_on, overlay.depends_on);
        replace_if_some(&mut self.stop_grace_period, overlay.stop_grace_period);
        replace_if_some(&mut self.command, overlay.command);
        replace_if_some(&mut self.entrypoint, overlay.entrypoint);
        replace_if_some(&mut self.user, overlay.user);
        replace_if_some(&mut self.working_dir, overlay.working_dir);
        merge_optional_string_map(&mut self.labels, overlay.labels);
        merge_optional_healthcheck(&mut self.healthcheck, overlay.healthcheck);
        merge_optional_nimbus_plan(&mut self.x_nimbus, overlay.x_nimbus);
        replace_if_some(&mut self.networks, overlay.networks);
        replace_if_some(&mut self.configs, overlay.configs);
        replace_if_some(&mut self.secrets, overlay.secrets);
        replace_if_some(&mut self.cap_add, overlay.cap_add);
        replace_if_some(&mut self.cap_drop, overlay.cap_drop);
        replace_if_some(&mut self.privileged, overlay.privileged);
        replace_if_some(&mut self.logging, overlay.logging);
        self.extra.extend(overlay.extra);
    }
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

impl RawComposeStringMap {
    fn merge_from(&mut self, overlay: Self) {
        match (self, overlay) {
            (Self::Map(base), Self::Map(overlay)) => base.extend(overlay),
            (Self::List(base), Self::List(overlay)) => base.extend(overlay),
            (base, overlay) => *base = overlay,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum RawComposeEnvFile {
    Single(String),
    List(Vec<RawComposeEnvFileEntry>),
}

impl RawComposeEnvFile {
    fn into_entries(self) -> Vec<RawComposeEnvFileEntry> {
        match self {
            Self::Single(path) => vec![RawComposeEnvFileEntry::Path(path)],
            Self::List(entries) => entries,
        }
    }

    fn from_entries(entries: Vec<RawComposeEnvFileEntry>) -> Option<Self> {
        if entries.is_empty() {
            None
        } else {
            Some(Self::List(entries))
        }
    }
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

impl RawComposeDependsOn {
    fn merge_from(&mut self, overlay: Self) {
        match (self, overlay) {
            (Self::Map(base), Self::Map(overlay)) => base.extend(overlay),
            (Self::List(base), Self::List(overlay)) => {
                for item in overlay {
                    if !base.contains(&item) {
                        base.push(item);
                    }
                }
            }
            (base, overlay) => *base = overlay,
        }
    }
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

impl RawComposeDeploy {
    fn merge_from(&mut self, overlay: Self) {
        merge_optional_resources(&mut self.resources, overlay.resources);
        replace_if_some(&mut self.replicas, overlay.replicas);
        replace_if_some(&mut self.placement, overlay.placement);
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeResources {
    #[serde(default)]
    pub(super) limits: Option<RawComposeResourceLimits>,
}

impl RawComposeResources {
    fn merge_from(&mut self, overlay: Self) {
        merge_optional_resource_limits(&mut self.limits, overlay.limits);
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawComposeResourceLimits {
    #[serde(default)]
    pub(super) cpus: Option<String>,
    #[serde(default)]
    pub(super) memory: Option<String>,
}

impl RawComposeResourceLimits {
    fn merge_from(&mut self, overlay: Self) {
        replace_if_some(&mut self.cpus, overlay.cpus);
        replace_if_some(&mut self.memory, overlay.memory);
    }
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

impl RawComposeHealthcheck {
    fn merge_from(&mut self, overlay: Self) {
        replace_if_some(&mut self.test, overlay.test);
        replace_if_some(&mut self.interval, overlay.interval);
        replace_if_some(&mut self.timeout, overlay.timeout);
        replace_if_some(&mut self.retries, overlay.retries);
        replace_if_some(&mut self.start_period, overlay.start_period);
        replace_if_some(&mut self.disable, overlay.disable);
        self.extra.extend(overlay.extra);
    }
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

fn merge_named_entries<T>(
    base: &mut BTreeMap<String, T>,
    overlay: BTreeMap<String, T>,
    merge_existing: impl Fn(&mut T, T),
) {
    for (key, value) in overlay {
        if let Some(existing) = base.get_mut(&key) {
            merge_existing(existing, value);
        } else {
            base.insert(key, value);
        }
    }
}

fn replace_if_some<T>(base: &mut Option<T>, overlay: Option<T>) {
    if let Some(overlay) = overlay {
        *base = Some(overlay);
    }
}

fn merge_optional_string_map(
    base: &mut Option<RawComposeStringMap>,
    overlay: Option<RawComposeStringMap>,
) {
    match (base.as_mut(), overlay) {
        (Some(base), Some(overlay)) => base.merge_from(overlay),
        (None, Some(overlay)) => *base = Some(overlay),
        _ => {}
    }
}

fn merge_optional_env_file(
    base: &mut Option<RawComposeEnvFile>,
    overlay: Option<RawComposeEnvFile>,
) {
    match (base.take(), overlay) {
        (Some(base_entries), Some(overlay_entries)) => {
            let mut entries = base_entries.into_entries();
            entries.extend(overlay_entries.into_entries());
            *base = RawComposeEnvFile::from_entries(entries);
        }
        (Some(base_entries), None) => *base = Some(base_entries),
        (None, Some(overlay_entries)) => *base = Some(overlay_entries),
        (None, None) => {}
    }
}

fn merge_optional_depends_on(
    base: &mut Option<RawComposeDependsOn>,
    overlay: Option<RawComposeDependsOn>,
) {
    match (base.as_mut(), overlay) {
        (Some(base), Some(overlay)) => base.merge_from(overlay),
        (None, Some(overlay)) => *base = Some(overlay),
        _ => {}
    }
}

fn merge_optional_deploy(base: &mut Option<RawComposeDeploy>, overlay: Option<RawComposeDeploy>) {
    match (base.as_mut(), overlay) {
        (Some(base), Some(overlay)) => base.merge_from(overlay),
        (None, Some(overlay)) => *base = Some(overlay),
        _ => {}
    }
}

fn merge_optional_resources(
    base: &mut Option<RawComposeResources>,
    overlay: Option<RawComposeResources>,
) {
    match (base.as_mut(), overlay) {
        (Some(base), Some(overlay)) => base.merge_from(overlay),
        (None, Some(overlay)) => *base = Some(overlay),
        _ => {}
    }
}

fn merge_optional_resource_limits(
    base: &mut Option<RawComposeResourceLimits>,
    overlay: Option<RawComposeResourceLimits>,
) {
    match (base.as_mut(), overlay) {
        (Some(base), Some(overlay)) => base.merge_from(overlay),
        (None, Some(overlay)) => *base = Some(overlay),
        _ => {}
    }
}

fn merge_optional_healthcheck(
    base: &mut Option<RawComposeHealthcheck>,
    overlay: Option<RawComposeHealthcheck>,
) {
    match (base.as_mut(), overlay) {
        (Some(base), Some(overlay)) => base.merge_from(overlay),
        (None, Some(overlay)) => *base = Some(overlay),
        _ => {}
    }
}

fn merge_optional_nimbus_plan(
    base: &mut Option<ComposeNimbusPlan>,
    overlay: Option<ComposeNimbusPlan>,
) {
    match (base.as_mut(), overlay) {
        (Some(base), Some(overlay)) => {
            replace_if_some(&mut base.backend, overlay.backend);
            replace_if_some(&mut base.idle_timeout, overlay.idle_timeout);
            replace_if_some(&mut base.snapshot, overlay.snapshot);
        }
        (None, Some(overlay)) => *base = Some(overlay),
        _ => {}
    }
}

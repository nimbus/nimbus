use std::collections::{BTreeMap, BTreeSet};

use nimbus_core::{Error, Result, TenantId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployMode {
    LocalDevelopment,
    Production,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimbusDeployPackage {
    app_intent_path: String,
    compose_sources: Vec<String>,
    bundled_policy_path: Option<String>,
}

impl NimbusDeployPackage {
    pub fn new(
        app_intent_path: impl Into<String>,
        compose_sources: impl IntoIterator<Item = impl Into<String>>,
        bundled_policy_path: Option<impl Into<String>>,
        mode: DeployMode,
    ) -> Result<Self> {
        let app_intent_path = non_empty(app_intent_path, "app intent path")?;
        if app_intent_path != "nimbus.yaml" {
            return Err(Error::InvalidInput(
                "Nimbus app intent must be packaged as nimbus.yaml".to_owned(),
            ));
        }
        let bundled_policy_path = bundled_policy_path
            .map(|path| non_empty(path, "bundled policy path"))
            .transpose()?;
        if matches!(mode, DeployMode::Production) && bundled_policy_path.is_some() {
            return Err(Error::PermissionDenied(
                "production deploy must not package app-bundled nimbus.policy.yaml authority"
                    .to_owned(),
            ));
        }
        Ok(Self {
            app_intent_path,
            compose_sources: compose_sources
                .into_iter()
                .map(|source| non_empty(source, "compose source"))
                .collect::<Result<Vec<_>>>()?,
            bundled_policy_path,
        })
    }

    pub fn app_intent_path(&self) -> &str {
        &self.app_intent_path
    }

    pub fn compose_sources(&self) -> &[String] {
        &self.compose_sources
    }

    pub fn bundled_policy_path(&self) -> Option<&str> {
        self.bundled_policy_path.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeSandboxTemplateService {
    service_name: String,
    image: String,
    profiles: BTreeSet<String>,
    ports: Vec<u16>,
    exposed_ports: Vec<u16>,
    stdin_open: bool,
    tty: bool,
    internal_networks: Vec<String>,
}

impl ComposeSandboxTemplateService {
    pub fn new(service_name: impl Into<String>, image: impl Into<String>) -> Result<Self> {
        Ok(Self {
            service_name: non_empty(service_name, "compose service name")?,
            image: non_empty(image, "compose service image")?,
            profiles: BTreeSet::new(),
            ports: Vec::new(),
            exposed_ports: Vec::new(),
            stdin_open: false,
            tty: false,
            internal_networks: Vec::new(),
        })
    }

    pub fn with_profile(mut self, profile: impl Into<String>) -> Result<Self> {
        self.profiles.insert(non_empty(profile, "compose profile")?);
        Ok(self)
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.ports.push(port);
        self
    }

    pub fn with_exposed_port(mut self, port: u16) -> Self {
        self.exposed_ports.push(port);
        self
    }

    pub fn with_stdin_open(mut self, stdin_open: bool) -> Self {
        self.stdin_open = stdin_open;
        self
    }

    pub fn with_tty(mut self, tty: bool) -> Self {
        self.tty = tty;
        self
    }

    pub fn with_internal_network(mut self, network: impl Into<String>) -> Result<Self> {
        self.internal_networks
            .push(non_empty(network, "internal compose network")?);
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTemplateProvenance {
    compose_service: String,
    app_intent_path: String,
}

impl SandboxTemplateProvenance {
    fn new(compose_service: impl Into<String>, app_intent_path: impl Into<String>) -> Result<Self> {
        Ok(Self {
            compose_service: non_empty(compose_service, "compose service name")?,
            app_intent_path: non_empty(app_intent_path, "app intent path")?,
        })
    }

    pub fn compose_service(&self) -> &str {
        &self.compose_service
    }

    pub fn app_intent_path(&self) -> &str {
        &self.app_intent_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTemplateChannelEndpoint {
    channel: String,
    container_port: u16,
}

impl SandboxTemplateChannelEndpoint {
    fn new(channel: impl Into<String>, container_port: u16) -> Result<Self> {
        Ok(Self {
            channel: non_empty(channel, "template channel")?,
            container_port,
        })
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn container_port(&self) -> u16 {
        self.container_port
    }

    pub fn published_host_port(&self) -> Option<u16> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTemplate {
    name: String,
    image: String,
    provenance: SandboxTemplateProvenance,
    channels: Vec<SandboxTemplateChannelEndpoint>,
    default_ttl_millis: u64,
    max_ttl_millis: u64,
    max_instances_per_tenant: u32,
    stdin_open: bool,
    tty: bool,
    internal_networks: Vec<String>,
}

impl SandboxTemplate {
    pub fn from_compose(
        template_name: impl Into<String>,
        service: ComposeSandboxTemplateService,
    ) -> Result<Self> {
        let template_name = non_empty(template_name, "sandbox template name")?;
        if !service.profiles.contains("nimbus-template") {
            return Err(Error::InvalidInput(format!(
                "sandbox template `{template_name}` must use Compose profile `nimbus-template`"
            )));
        }
        let mut channels = Vec::new();
        for port in service.ports.iter().chain(service.exposed_ports.iter()) {
            channels.push(SandboxTemplateChannelEndpoint::new(
                format!("tcp:{port}"),
                *port,
            )?);
        }
        Ok(Self {
            name: template_name,
            image: service.image,
            provenance: SandboxTemplateProvenance::new(service.service_name, "nimbus.yaml")?,
            channels,
            default_ttl_millis: 15 * 60 * 1000,
            max_ttl_millis: 2 * 60 * 60 * 1000,
            max_instances_per_tenant: 1,
            stdin_open: service.stdin_open,
            tty: service.tty,
            internal_networks: service.internal_networks,
        })
    }

    pub fn with_ttl_limits(mut self, default_ttl_millis: u64, max_ttl_millis: u64) -> Result<Self> {
        if default_ttl_millis == 0 || max_ttl_millis == 0 || default_ttl_millis > max_ttl_millis {
            return Err(Error::InvalidInput(
                "template TTL limits require 0 < default <= max".to_owned(),
            ));
        }
        self.default_ttl_millis = default_ttl_millis;
        self.max_ttl_millis = max_ttl_millis;
        Ok(self)
    }

    pub fn with_max_instances_per_tenant(mut self, max_instances: u32) -> Result<Self> {
        if max_instances == 0 {
            return Err(Error::InvalidInput(
                "template quota must allow at least one active lease".to_owned(),
            ));
        }
        self.max_instances_per_tenant = max_instances;
        Ok(self)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn image(&self) -> &str {
        &self.image
    }

    pub fn provenance(&self) -> &SandboxTemplateProvenance {
        &self.provenance
    }

    pub fn channels(&self) -> &[SandboxTemplateChannelEndpoint] {
        &self.channels
    }

    pub fn stdin_open(&self) -> bool {
        self.stdin_open
    }

    pub fn tty(&self) -> bool {
        self.tty
    }

    pub fn internal_networks(&self) -> &[String] {
        &self.internal_networks
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NimbusAppIntent {
    templates: Vec<SandboxTemplate>,
}

impl NimbusAppIntent {
    pub fn new(templates: impl IntoIterator<Item = SandboxTemplate>) -> Self {
        Self {
            templates: templates.into_iter().collect(),
        }
    }

    pub fn templates(&self) -> &[SandboxTemplate] {
        &self.templates
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSandboxTemplatePolicy {
    admitted_templates: BTreeMap<String, SandboxTemplate>,
    create_grants: BTreeSet<(String, String)>,
}

impl EffectiveSandboxTemplatePolicy {
    pub fn new(templates: impl IntoIterator<Item = SandboxTemplate>) -> Self {
        Self {
            admitted_templates: templates
                .into_iter()
                .map(|template| (template.name.clone(), template))
                .collect(),
            create_grants: BTreeSet::new(),
        }
    }

    pub fn with_create_from_template_grant(
        mut self,
        principal: impl Into<String>,
        template_name: impl Into<String>,
    ) -> Result<Self> {
        self.create_grants.insert((
            non_empty(principal, "template principal")?,
            non_empty(template_name, "sandbox template name")?,
        ));
        Ok(self)
    }

    pub fn admit_app_intent(&self, intent: &NimbusAppIntent) -> Result<Vec<SandboxTemplate>> {
        intent
            .templates()
            .iter()
            .map(|requested| {
                self.admitted_templates
                    .get(requested.name())
                    .cloned()
                    .ok_or_else(|| {
                        Error::PermissionDenied(format!(
                            "effective policy did not admit sandbox template `{}`",
                            requested.name()
                        ))
                    })
            })
            .collect()
    }

    fn template(&self, template_name: &str) -> Result<&SandboxTemplate> {
        self.admitted_templates
            .get(template_name)
            .ok_or_else(|| Error::NotFound(format!("sandbox template `{template_name}` not found")))
    }

    fn ensure_exact_grant(&self, principal: &str, template_name: &str) -> Result<()> {
        if self
            .create_grants
            .contains(&(principal.to_owned(), template_name.to_owned()))
        {
            return Ok(());
        }
        Err(Error::PermissionDenied(format!(
            "principal `{principal}` lacks sandboxes.createFromTemplate grant for `{template_name}`"
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTemplateLeaseRequest {
    tenant_id: TenantId,
    principal: String,
    template_name: String,
    ttl_millis: Option<u64>,
}

impl SandboxTemplateLeaseRequest {
    pub fn new(
        tenant_id: TenantId,
        principal: impl Into<String>,
        template_name: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            tenant_id,
            principal: non_empty(principal, "lease principal")?,
            template_name: non_empty(template_name, "sandbox template name")?,
            ttl_millis: None,
        })
    }

    pub fn with_ttl_millis(mut self, ttl_millis: u64) -> Self {
        self.ttl_millis = Some(ttl_millis);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasedSandbox {
    id: String,
    tenant_id: TenantId,
    principal: String,
    template_name: String,
    expires_at_millis: u64,
}

impl LeasedSandbox {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn principal(&self) -> &str {
        &self.principal
    }

    pub fn expires_at_millis(&self) -> u64 {
        self.expires_at_millis
    }

    pub fn template_name(&self) -> &str {
        &self.template_name
    }
}

#[derive(Debug, Default)]
pub struct SandboxTemplateLeaseController {
    active: BTreeMap<(TenantId, String), Vec<LeasedSandbox>>,
    next_id: u64,
}

impl SandboxTemplateLeaseController {
    pub fn lease(
        &mut self,
        policy: &EffectiveSandboxTemplatePolicy,
        request: SandboxTemplateLeaseRequest,
        now_millis: u64,
    ) -> Result<LeasedSandbox> {
        policy.ensure_exact_grant(&request.principal, &request.template_name)?;
        let template = policy.template(&request.template_name)?;
        let ttl = request.ttl_millis.unwrap_or(template.default_ttl_millis);
        if ttl == 0 || ttl > template.max_ttl_millis {
            return Err(Error::InvalidInput(format!(
                "requested TTL {ttl}ms exceeds max TTL {}ms for sandbox template `{}`",
                template.max_ttl_millis,
                template.name()
            )));
        }
        let key = (request.tenant_id.clone(), request.template_name.clone());
        let active = self.active.entry(key).or_default();
        if active.len() as u32 >= template.max_instances_per_tenant {
            return Err(Error::Conflict(format!(
                "sandbox template `{}` quota exceeded for tenant {}",
                template.name(),
                request.tenant_id
            )));
        }
        self.next_id += 1;
        let lease = LeasedSandbox {
            id: format!("lease-{}", self.next_id),
            tenant_id: request.tenant_id,
            principal: request.principal,
            template_name: request.template_name,
            expires_at_millis: now_millis + ttl,
        };
        active.push(lease.clone());
        Ok(lease)
    }

    pub fn reconcile_expired(&mut self, now_millis: u64) -> Vec<LeasedSandbox> {
        let mut expired = Vec::new();
        for leases in self.active.values_mut() {
            let mut retained = Vec::new();
            for lease in leases.drain(..) {
                if lease.expires_at_millis <= now_millis {
                    expired.push(lease);
                } else {
                    retained.push(lease);
                }
            }
            *leases = retained;
        }
        self.active.retain(|_, leases| !leases.is_empty());
        expired
    }

    pub fn active_count(&self, tenant_id: &TenantId, template_name: &str) -> usize {
        self.active
            .get(&(tenant_id.clone(), template_name.to_owned()))
            .map_or(0, Vec::len)
    }
}

fn non_empty(value: impl Into<String>, label: &str) -> Result<String> {
    let value = value.into();
    if value.trim() != value || value.is_empty() {
        return Err(Error::InvalidInput(format!(
            "{label} must be non-empty and must not have leading or trailing whitespace"
        )));
    }
    if value.contains('\0') || value.contains('\n') {
        return Err(Error::InvalidInput(format!(
            "{label} must not contain control characters"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant_id() -> TenantId {
        TenantId::new("tenant-a").expect("tenant id should parse")
    }

    fn compose_template_service() -> ComposeSandboxTemplateService {
        ComposeSandboxTemplateService::new("browser", "ghcr.io/acme/browser@sha256:abc")
            .expect("compose service should parse")
            .with_profile("nimbus-template")
            .expect("profile should parse")
            .with_port(9222)
            .with_exposed_port(5900)
            .with_stdin_open(true)
            .with_tty(true)
            .with_internal_network("agent-internal")
            .expect("internal network should parse")
    }

    fn template() -> SandboxTemplate {
        SandboxTemplate::from_compose("agentBrowser", compose_template_service())
            .expect("template import should succeed")
            .with_ttl_limits(1_000, 10_000)
            .expect("ttl limits should parse")
            .with_max_instances_per_tenant(1)
            .expect("quota should parse")
    }

    fn policy() -> EffectiveSandboxTemplatePolicy {
        EffectiveSandboxTemplatePolicy::new([template()])
            .with_create_from_template_grant("runtime:actions", "agentBrowser")
            .expect("grant should parse")
    }

    #[test]
    fn compose_imports_sandbox_template() {
        let template = SandboxTemplate::from_compose("agentBrowser", compose_template_service())
            .expect("Compose service should import as sandbox template");

        assert_eq!(template.name(), "agentBrowser");
        assert_eq!(template.image(), "ghcr.io/acme/browser@sha256:abc");
        assert_eq!(template.provenance().compose_service(), "browser");
        assert_eq!(template.provenance().app_intent_path(), "nimbus.yaml");
        assert!(template.stdin_open());
        assert!(template.tty());
        assert_eq!(template.internal_networks(), &["agent-internal".to_owned()]);
    }

    #[test]
    fn deploy_packages_nimbus_yaml_app_intent() {
        let package = NimbusDeployPackage::new(
            "nimbus.yaml",
            ["compose.yaml", "compose.browser.yaml"],
            Option::<String>::None,
            DeployMode::Production,
        )
        .expect("production package should include app intent");

        assert_eq!(package.app_intent_path(), "nimbus.yaml");
        assert_eq!(
            package.compose_sources(),
            &["compose.yaml".to_owned(), "compose.browser.yaml".to_owned()]
        );
        assert!(package.bundled_policy_path().is_none());
    }

    #[test]
    fn prod_deploy_rejects_app_bundled_policy_authority() {
        let error = NimbusDeployPackage::new(
            "nimbus.yaml",
            ["compose.yaml"],
            Some("nimbus.policy.yaml"),
            DeployMode::Production,
        )
        .expect_err("production deploy must reject app-bundled policy authority");

        assert!(
            error.to_string().contains("nimbus.policy.yaml"),
            "policy-authority rejection should name the file: {error}"
        );
    }

    #[test]
    fn app_intent_is_admitted_against_effective_policy() {
        let intent = NimbusAppIntent::new([template()]);
        let admitted = policy()
            .admit_app_intent(&intent)
            .expect("effective policy should admit matching template intent");

        assert_eq!(admitted.len(), 1);
        assert_eq!(admitted[0].name(), "agentBrowser");

        let denied_intent = NimbusAppIntent::new([SandboxTemplate::from_compose(
            "unapproved",
            compose_template_service(),
        )
        .expect("unapproved template request should parse")]);
        let error = policy()
            .admit_app_intent(&denied_intent)
            .expect_err("effective policy must reject unadmitted app intent");
        assert!(
            error.to_string().contains("unapproved"),
            "rejection should name the unadmitted template: {error}"
        );
    }

    #[test]
    fn create_from_template_requires_exact_grant() {
        let mut controller = SandboxTemplateLeaseController::default();
        let denied = SandboxTemplateLeaseRequest::new(tenant_id(), "runtime:other", "agentBrowser")
            .expect("lease request should parse");
        let error = controller
            .lease(&policy(), denied, 0)
            .expect_err("lease must require an exact createFromTemplate grant");

        assert!(
            error.to_string().contains("createFromTemplate"),
            "grant rejection should name the exact capability: {error}"
        );
    }

    #[test]
    fn sandbox_template_ports_are_channel_only() {
        let template = template();

        assert_eq!(template.channels().len(), 2);
        assert!(template.channels().iter().all(|channel| {
            channel.published_host_port().is_none() && channel.channel().starts_with("tcp:")
        }));
        assert_eq!(template.channels()[0].container_port(), 9222);
    }

    #[test]
    fn leased_sandbox_ttl_reconciles_deadline() {
        let mut controller = SandboxTemplateLeaseController::default();
        let request =
            SandboxTemplateLeaseRequest::new(tenant_id(), "runtime:actions", "agentBrowser")
                .expect("lease request should parse")
                .with_ttl_millis(1_000);

        let lease = controller
            .lease(&policy(), request, 4_000)
            .expect("exact-granted template lease should succeed");
        assert_eq!(lease.tenant_id(), &tenant_id());
        assert_eq!(lease.principal(), "runtime:actions");
        assert_eq!(lease.expires_at_millis(), 5_000);
        assert!(controller.reconcile_expired(4_999).is_empty());
        let expired = controller.reconcile_expired(5_000);
        assert_eq!(expired, vec![lease]);
    }

    #[test]
    fn leased_sandbox_quota_is_enforced() {
        let mut controller = SandboxTemplateLeaseController::default();
        let first =
            SandboxTemplateLeaseRequest::new(tenant_id(), "runtime:actions", "agentBrowser")
                .expect("first lease request should parse");
        let second =
            SandboxTemplateLeaseRequest::new(tenant_id(), "runtime:actions", "agentBrowser")
                .expect("second lease request should parse");

        controller
            .lease(&policy(), first, 0)
            .expect("first template lease should fit quota");
        let error = controller
            .lease(&policy(), second, 0)
            .expect_err("second lease must exceed one-instance quota");

        assert!(
            error.to_string().contains("quota"),
            "quota rejection should be visible: {error}"
        );
        assert_eq!(controller.active_count(&tenant_id(), "agentBrowser"), 1);
    }
}

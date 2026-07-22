//! CLI enablement for the protocol adapter surfaces. Under D7 every
//! adapter serves by default: Firestore routes mount on the main HTTP
//! listener, MongoDB serves on its conventional port (27017), and DynamoDB
//! serves on its conventional port (8000), each with a `--no-*` opt-out.
//! A busy conventional port fails startup with guidance to choose another port
//! or disable the listener. Explicit ports fail at bind time in the same
//! fail-loud posture.
//! Operator flags/env provide credentials when present; otherwise the
//! listeners use the generated wire-credential store under the control
//! data dir (D5), shared with `nimbus dev`. The same non-loopback opt-in
//! gate as the main listener applies.
//!
//! Each protocol surface resolves in its own module (`cloudflare`,
//! `convex_tenancy`, `dynamodb`, `firebase`, `mongodb`, `s3`). This module is
//! the composition root: it owns [`AdapterEnablement`], the credential-store
//! and bind-address helpers the port-owning resolvers share, and the
//! dispatcher that calls each resolver once per boot.

mod cloudflare;
mod convex_tenancy;
mod dynamodb;
mod firebase;
mod mongodb;
mod s3;

pub(crate) use dynamodb::DYNAMODB_CONVENTIONAL_PORT;
pub(crate) use mongodb::MONGODB_CONVENTIONAL_PORT;
pub(crate) use s3::S3_CONVENTIONAL_PORT;

use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use nimbus::Error;
use nimbus_server::{
    CloudflareConfig, ConvexTenancyConfig, DynamoDbConfig, FirebaseConfig, MongoDbConfig, S3Config,
};

use crate::wire_credentials::{WireCredentials, load_or_generate};

use super::StartCommand;

/// Tenant the generated wire credentials bind to when the operator provides no
/// surface-specific bindings. `nimbus dev` overrides this with its auto-tenant
/// by passing explicit bindings.
const DEFAULT_WIRE_TENANT: &str = "default";

/// Adapter configs resolved from the start command. `None` means the
/// surface does not serve this boot — opted out, or its conventional
/// port was busy with no explicit port requested.
#[derive(Debug)]
pub(crate) struct AdapterEnablement {
    pub(crate) firebase: Option<FirebaseConfig>,
    pub(crate) cloudflare: Option<CloudflareConfig>,
    pub(crate) convex_tenancy: Option<ConvexTenancyConfig>,
    /// Loud boot notice when [`convex_tenancy`] admits anonymous requests —
    /// either `nimbus dev`'s auto-provisioned dev team (EX3.7) or an operator's
    /// explicit `NIMBUS_CONVEX_ANONYMOUS_TEAM`. `None` whenever anonymous
    /// application-Convex access stays refused, which keeps
    /// [`status_lines`](Self::status_lines) unchanged for every boot that
    /// doesn't touch this path.
    pub(crate) convex_tenancy_notice: Option<String>,
    pub(crate) mongodb: Option<MongoDbConfig>,
    pub(crate) dynamodb: Option<DynamoDbConfig>,
    pub(crate) s3: Option<S3Config>,
}

impl AdapterEnablement {
    /// Startup-summary lines describing which adapter surfaces serve this
    /// boot. Every surface reports — `off` is as informative as an address
    /// — and the lines carry bind addresses only, never credentials.
    pub(super) fn status_lines(&self) -> Vec<String> {
        let firestore = match &self.firebase {
            Some(_) => "mounted on the main listener".to_string(),
            None => "off".to_string(),
        };
        let cloudflare = match &self.cloudflare {
            Some(config) if config.bindings().is_empty() => {
                "mounted on the main listener".to_string()
            }
            Some(config) => format!(
                "mounted on the main listener ({} KV, {} DO, {} D1, {} R2 bindings)",
                config.bindings().kv_namespaces().len(),
                config.bindings().durable_objects().len(),
                config.bindings().d1_databases().len(),
                config.bindings().r2_buckets().len()
            ),
            None => "off".to_string(),
        };
        let mongodb = self
            .mongodb
            .as_ref()
            .map_or_else(|| "off".to_string(), |config| config.bind_addr.to_string());
        let dynamodb = self
            .dynamodb
            .as_ref()
            .map_or_else(|| "off".to_string(), |config| config.bind_addr.to_string());
        let s3 = self
            .s3
            .as_ref()
            .map_or_else(|| "off".to_string(), |config| config.bind_addr.to_string());
        let mut lines = vec![
            format!("firestore routes:\t{firestore}"),
            format!("cloudflare routes:\t{cloudflare}"),
            format!("mongodb listener:\t{mongodb}"),
            format!("dynamodb listener:\t{dynamodb}"),
            format!("s3 listener:\t{s3}"),
        ];
        if let Some(notice) = &self.convex_tenancy_notice {
            lines.push(notice.clone());
        }
        lines
    }

    /// Mounts every resolved adapter surface onto the serve options.
    pub(crate) fn apply_to(
        self,
        mut options: nimbus_server::ServeOptions,
    ) -> nimbus_server::ServeOptions {
        if let Some(firebase) = self.firebase {
            options = options.with_firebase_config(firebase);
        }
        if let Some(cloudflare) = self.cloudflare {
            options = options.with_cloudflare(cloudflare);
        }
        if let Some(convex_tenancy) = self.convex_tenancy {
            options = options.with_convex_tenancy(convex_tenancy);
        }
        if let Some(mongodb) = self.mongodb {
            options = options.with_mongodb(mongodb);
        }
        if let Some(dynamodb) = self.dynamodb {
            options = options.with_dynamodb(dynamodb);
        }
        if let Some(s3) = self.s3 {
            options = options.with_s3(s3);
        }
        options
    }
}

/// Lazy handle on the shared persisted wire-credential store (D5). Disk is
/// touched only when a default-on listener actually needs generated
/// credentials, so opted-out and operator-credentialed boots never create
/// the file.
struct CredentialStore<'a> {
    data_dir: &'a Path,
    cached: Option<WireCredentials>,
}

impl<'a> CredentialStore<'a> {
    fn new(data_dir: &'a Path) -> Self {
        Self {
            data_dir,
            cached: None,
        }
    }

    fn get(&mut self) -> Result<&WireCredentials, Error> {
        if self.cached.is_none() {
            self.cached = Some(load_or_generate(self.data_dir).map_err(store_error)?);
        }
        Ok(self
            .cached
            .as_ref()
            .expect("credentials cached by the branch above"))
    }
}

/// A malformed store is operator-fixable (the underlying error carries the
/// "delete it to regenerate" hint), so it maps to `InvalidInput`; anything
/// else is an environment failure.
fn store_error(error: std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::InvalidData {
        Error::InvalidInput(error.to_string())
    } else {
        Error::Internal(error.to_string())
    }
}

pub(super) fn resolve_adapter_enablement(
    command: &StartCommand,
    control_data_dir: &Path,
    app_dir: Option<&Path>,
) -> Result<AdapterEnablement, Error> {
    resolve_adapter_enablement_with_env_and_app_dir(
        command,
        control_data_dir,
        app_dir,
        |name| std::env::var(name).ok(),
        |port| std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
    )
}

#[cfg(test)]
pub(crate) fn resolve_adapter_enablement_with_env(
    command: &StartCommand,
    control_data_dir: &Path,
    env_lookup: impl Fn(&str) -> Option<String>,
    port_is_free: impl Fn(u16) -> bool,
) -> Result<AdapterEnablement, Error> {
    resolve_adapter_enablement_with_env_and_app_dir(
        command,
        control_data_dir,
        None,
        env_lookup,
        port_is_free,
    )
}

pub(crate) fn resolve_adapter_enablement_with_env_and_app_dir(
    command: &StartCommand,
    control_data_dir: &Path,
    app_dir: Option<&Path>,
    env_lookup: impl Fn(&str) -> Option<String>,
    port_is_free: impl Fn(u16) -> bool,
) -> Result<AdapterEnablement, Error> {
    let mut store = CredentialStore::new(control_data_dir);
    let mongodb = mongodb::resolve_mongodb(command, &env_lookup, &port_is_free, &mut store)?;
    let dynamodb = dynamodb::resolve_dynamodb(command, &env_lookup, &port_is_free, &mut store)?;
    let s3 = s3::resolve_s3(command, &env_lookup, &port_is_free, &mut store)?;
    let cloudflare = cloudflare::resolve_cloudflare(command, app_dir, &mut store)?;
    let (convex_tenancy, convex_tenancy_notice) =
        convex_tenancy::resolve_convex_tenancy(command, &env_lookup)?;
    Ok(AdapterEnablement {
        firebase: firebase::resolve_firebase(command, &env_lookup)?,
        cloudflare,
        convex_tenancy,
        convex_tenancy_notice,
        mongodb,
        dynamodb,
        s3,
    })
}

fn adapter_bind_addr(host: &str, port: u16, flag: &str) -> Result<SocketAddr, Error> {
    let ip: IpAddr = if host.eq_ignore_ascii_case("localhost") {
        IpAddr::from([127, 0, 0, 1])
    } else {
        host.parse().map_err(|_| {
            Error::InvalidInput(format!("{flag}: `{host}` is not a valid IP address"))
        })?
    };
    Ok(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dynamodb::DYNAMODB_ACCESS_KEYS_ENV;
    use firebase::FIREBASE_PROJECTS_ENV;
    use mongodb::{MONGODB_CREDENTIALS_ENV, MONGODB_PASSWORD_ENV, MONGODB_USERNAME_ENV};
    use nimbus::TenantId;
    use s3::S3_ACCESS_KEYS_ENV;

    use crate::wire_credentials::wire_credentials_path;

    fn base_command() -> StartCommand {
        StartCommand::default()
    }

    /// Resolve with an always-free port probe so default-on tests stay
    /// deterministic on machines running a real `mongod` or DynamoDB Local.
    fn resolve(
        command: &StartCommand,
        data_dir: &Path,
        env_lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<AdapterEnablement, Error> {
        resolve_adapter_enablement_with_env(command, data_dir, env_lookup, |_| true)
    }

    #[test]
    fn start_serves_all_adapters_by_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve(&base_command(), temp.path(), |_| None)
            .expect("the default command must resolve every surface");

        let firebase = resolved
            .firebase
            .as_ref()
            .expect("firestore routes default on");
        assert!(
            !firebase.allows_emulator_token_verification_bypass(),
            "plain start must not enable the dev-only Firebase emulator bypass"
        );
        assert!(
            firebase.project_registry().resolve("demo").is_err(),
            "plain start must keep Firestore strict until projects are explicitly bound"
        );
        assert!(
            resolved.cloudflare.is_some(),
            "cloudflare routes default on"
        );

        let mongodb = resolved.mongodb.expect("mongodb listener defaults on");
        assert_eq!(
            mongodb.bind_addr,
            format!("127.0.0.1:{MONGODB_CONVENTIONAL_PORT}")
                .parse()
                .unwrap()
        );

        let dynamodb = resolved.dynamodb.expect("dynamodb listener defaults on");
        assert_eq!(
            dynamodb.bind_addr,
            format!("127.0.0.1:{DYNAMODB_CONVENTIONAL_PORT}")
                .parse()
                .unwrap()
        );
        let s3 = resolved.s3.expect("s3 listener defaults on");
        assert_eq!(
            s3.bind_addr,
            format!("127.0.0.1:{S3_CONVENTIONAL_PORT}").parse().unwrap()
        );

        // Credentials came from the persisted store: the file now exists,
        // its MongoDB user backs the listener auth, and the DynamoDB/S3 keys
        // are bound separately (to the `default` tenant).
        assert!(
            wire_credentials_path(temp.path()).exists(),
            "a default-on boot must persist the generated credentials"
        );
        let store = load_or_generate(temp.path()).expect("reload the persisted store");
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            store.mongodb_username
        );
        assert!(
            dynamodb
                .access_keys
                .binding(&store.dynamodb_access_key_id)
                .is_ok(),
            "the store access key must authenticate"
        );
        assert!(
            s3.access_keys.binding(&store.s3_access_key_id).is_ok(),
            "the store S3 access key must authenticate"
        );
        assert_eq!(
            s3.convex_download_secret.as_deref(),
            Some(store.s3_secret_access_key.as_bytes()),
            "generated S3 credentials seed the local Convex storage download signer"
        );
        assert!(
            s3.access_keys
                .binding(&store.dynamodb_access_key_id)
                .is_err(),
            "S3 must not accept the DynamoDB access key"
        );
        assert!(
            resolved
                .cloudflare
                .as_ref()
                .expect("cloudflare should resolve")
                .access_keys()
                .binding(&store.dynamodb_access_key_id)
                .is_ok(),
            "the store access key must authenticate the Cloudflare surface"
        );
    }

    #[test]
    fn dev_auto_tenant_enables_loopback_firebase_project_binding() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.auto_tenant = Some("dev-project".to_string());
        let resolved = resolve(&command, temp.path(), |name| {
            (name == FIREBASE_PROJECTS_ENV).then(|| "other:other".to_string())
        })
        .expect("dev auto-tenant Firestore config should resolve");
        let firebase = resolved.firebase.expect("firestore routes stay mounted");

        assert!(
            firebase.allows_emulator_token_verification_bypass(),
            "dev auto-tenant mode must accept Firebase emulator mock tokens on loopback"
        );
        assert_eq!(
            firebase
                .project_registry()
                .resolve("dev-project")
                .expect("dev project should resolve"),
            TenantId::new("dev-project").expect("tenant id should parse")
        );
        assert!(
            firebase.project_registry().resolve("other").is_err(),
            "ambient project bindings must not desync nimbus dev from its auto tenant"
        );
    }

    #[test]
    fn dev_auto_tenant_enables_anonymous_convex_access() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.auto_tenant = Some("dev-project".to_string());
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("dev auto-tenant Convex tenancy config should resolve");
        let convex_tenancy = resolved
            .convex_tenancy
            .as_ref()
            .expect("nimbus dev must auto-provision a convex tenancy config");
        let tenant = TenantId::new("dev-project").expect("tenant id should parse");
        convex_tenancy
            .authorize_silo_selection(&tenant, &nimbus_core::PrincipalContext::anonymous())
            .expect("anonymous access to the auto-tenant's silo must be admitted");
        assert!(
            resolved
                .convex_tenancy_notice
                .as_ref()
                .expect("dev auto-provisioning must print a loud boot notice")
                .contains("dev-project"),
            "the notice should name the auto-provisioned tenant"
        );
    }

    #[test]
    fn start_with_no_convex_envs_keeps_convex_refused() {
        let temp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve(&base_command(), temp.path(), |_| None)
            .expect("the default command must resolve every surface");
        assert!(
            resolved.convex_tenancy.is_none(),
            "plain start with no envs must not plumb a convex tenancy config"
        );
        assert!(
            resolved.convex_tenancy_notice.is_none(),
            "plain start with no envs must never print an anonymous-access notice"
        );
    }

    #[test]
    fn status_lines_report_every_surface_without_credentials() {
        let temp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve(&base_command(), temp.path(), |_| None)
            .expect("the default command must resolve every surface");
        let lines = resolved.status_lines();
        assert_eq!(
            lines,
            vec![
                "firestore routes:\tmounted on the main listener".to_string(),
                "cloudflare routes:\tmounted on the main listener".to_string(),
                format!("mongodb listener:\t127.0.0.1:{MONGODB_CONVENTIONAL_PORT}"),
                format!("dynamodb listener:\t127.0.0.1:{DYNAMODB_CONVENTIONAL_PORT}"),
                format!("s3 listener:\t127.0.0.1:{S3_CONVENTIONAL_PORT}"),
            ]
        );
        let store = load_or_generate(temp.path()).expect("reload the persisted store");
        for line in &lines {
            assert!(
                !line.contains(&store.mongodb_password)
                    && !line.contains(&store.dynamodb_access_key_id)
                    && !line.contains(&store.dynamodb_secret_access_key)
                    && !line.contains(&store.s3_access_key_id)
                    && !line.contains(&store.s3_secret_access_key),
                "status lines must never carry credential material: {line}"
            );
        }

        let mut command = base_command();
        command.firestore = false;
        command.cloudflare = false;
        command.mongodb = false;
        command.dynamodb = false;
        command.s3 = false;
        let opted_out = resolve(&command, temp.path(), |_| None)
            .expect("a fully opted-out command should resolve");
        assert_eq!(
            opted_out.status_lines(),
            vec![
                "firestore routes:\toff".to_string(),
                "cloudflare routes:\toff".to_string(),
                "mongodb listener:\toff".to_string(),
                "dynamodb listener:\toff".to_string(),
                "s3 listener:\toff".to_string(),
            ]
        );
    }

    #[test]
    fn adapter_opt_out_flags_disable_surfaces() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.firestore = false;
        command.cloudflare = false;
        command.mongodb = false;
        command.dynamodb = false;
        command.s3 = false;
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("a fully opted-out command should resolve");
        assert!(resolved.firebase.is_none());
        assert!(resolved.cloudflare.is_none());
        assert!(resolved.mongodb.is_none());
        assert!(resolved.dynamodb.is_none());
        assert!(resolved.s3.is_none());
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "an opted-out boot must never touch the credential store"
        );

        // Opting out while also configuring the surface is a conflict, not
        // a silent ignore.
        let mut command = base_command();
        command.mongodb = false;
        command.mongodb_port = Some(27017);
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("--no-mongodb with --mongodb-port must conflict");
        assert!(error.to_string().contains("--no-mongodb"));

        let mut command = base_command();
        command.dynamodb = false;
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("--no-dynamodb with --dynamodb-access-key must conflict");
        assert!(error.to_string().contains("--no-dynamodb"));

        let mut command = base_command();
        command.s3 = false;
        command.s3_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("--no-s3 with --s3-access-key must conflict");
        assert!(error.to_string().contains("--no-s3"));
    }

    #[test]
    fn cloudflare_reads_wrangler_bindings_from_app_dir() {
        let data_dir = tempfile::tempdir().expect("data tempdir");
        let app_dir = tempfile::tempdir().expect("app tempdir");
        std::fs::write(
            app_dir.path().join("wrangler.jsonc"),
            r#"
            {
              "kv_namespaces": [
                { "binding": "CACHE", "id": "kv-prod", },
              ],
              "durable_objects": {
                "bindings": [
                  { "name": "COUNTERS", "class_name": "Counter", },
                ],
              },
            }
            "#,
        )
        .expect("wrangler config should write");

        let resolved = resolve_adapter_enablement_with_env_and_app_dir(
            &base_command(),
            data_dir.path(),
            Some(app_dir.path()),
            |_| None,
            |_| true,
        )
        .expect("wrangler-backed Cloudflare config should resolve");
        let cloudflare = resolved
            .cloudflare
            .as_ref()
            .expect("cloudflare should be enabled");

        assert_eq!(cloudflare.bindings().kv_namespaces()[0].binding, "CACHE");
        assert_eq!(cloudflare.bindings().durable_objects()[0].name, "COUNTERS");
        assert_eq!(
            resolved.status_lines()[1],
            "cloudflare routes:\tmounted on the main listener (1 KV, 1 DO, 0 D1, 0 R2 bindings)"
        );
    }

    #[test]
    fn busy_conventional_ports_fail_startup() {
        let temp = tempfile::tempdir().expect("tempdir");
        for (command, expected) in [
            (base_command(), "MongoDB conventional port 27017 is busy"),
            (
                {
                    let mut command = base_command();
                    command.mongodb = false;
                    command
                },
                "DynamoDB conventional port 8000 is busy",
            ),
            (
                {
                    let mut command = base_command();
                    command.mongodb = false;
                    command.dynamodb = false;
                    command
                },
                "S3 conventional port 9000 is busy",
            ),
        ] {
            let error =
                resolve_adapter_enablement_with_env(&command, temp.path(), |_| None, |_| false)
                    .expect_err("a busy default listener port must fail boot");
            assert!(
                error.to_string().contains(expected),
                "unexpected error: {error}"
            );
        }

        // An explicit port never probes: the operator asked for it, so a
        // conflict surfaces as a loud bind failure at serve time instead.
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.dynamodb_port = Some(8000);
        command.s3_port = Some(9000);
        let resolved =
            resolve_adapter_enablement_with_env(&command, temp.path(), |_| None, |_| false)
                .expect("explicit ports must resolve regardless of the probe");
        assert!(resolved.mongodb.is_some());
        assert!(resolved.dynamodb.is_some());
        assert!(resolved.s3.is_some());
    }

    #[test]
    fn mongodb_listener_requires_scram_credentials() {
        // Even with nothing configured, the listener never serves
        // unauthenticated: the generated store provides the SCRAM user.
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("store credentials should back the listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        let store = load_or_generate(temp.path()).expect("reload the persisted store");
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            store.mongodb_username
        );

        // Operator env credentials take precedence over the store.
        let resolved = resolve(&command, temp.path(), |name| match name {
            MONGODB_USERNAME_ENV => Some("ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("secret".to_string()),
            _ => None,
        })
        .expect("env credentials should enable the listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert_eq!(mongodb.bind_addr, "127.0.0.1:27017".parse().unwrap());
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            "ops"
        );
    }

    #[test]
    fn mongodb_password_is_env_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_username = Some("ops".to_string());
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("a username without the env password must be rejected");
        assert!(error.to_string().contains(MONGODB_PASSWORD_ENV));

        // The reverse half-pair is rejected too: a password with no
        // username is ambiguous between operator intent and leftovers.
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_username = None;
        let error = resolve(&command, temp.path(), |name| {
            (name == MONGODB_PASSWORD_ENV).then(|| "secret".to_string())
        })
        .expect_err("a password without a username must be rejected");
        assert!(error.to_string().contains("without a username"));
    }

    #[test]
    fn mongodb_credentials_without_port_apply_to_the_default_listener() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        // Opt DynamoDB out so its store-backed default key can't create the
        // file this test asserts stays absent. Cloudflare also uses the
        // generated store credential by default, so it is out of this
        // MongoDB-only assertion too.
        command.dynamodb = false;
        command.cloudflare = false;
        command.s3 = false;
        command.mongodb_username = Some("ops".to_string());
        let resolved = resolve(&command, temp.path(), |name| {
            (name == MONGODB_PASSWORD_ENV).then(|| "secret".to_string())
        })
        .expect("operator credentials should apply to the default listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert_eq!(mongodb.bind_addr.port(), MONGODB_CONVENTIONAL_PORT);
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            "ops"
        );
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "operator credentials must not touch the store"
        );
    }

    #[test]
    fn dev_store_only_credentials_ignore_ambient_operator_env() {
        // `nimbus dev` pins the listener to the store credentials its
        // `.env.local` advertises; ambient NIMBUS_MONGODB_* in the
        // developer's shell must not desync the two.
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_credentials_from_store = true;
        let resolved = resolve(&command, temp.path(), |name| match name {
            MONGODB_USERNAME_ENV => Some("ambient-ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("ambient-secret".to_string()),
            _ => None,
        })
        .expect("store-only mode should resolve");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        let store = load_or_generate(temp.path()).expect("reload the persisted store");
        assert_eq!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            store.mongodb_username
        );
        assert_ne!(
            mongodb
                .auth_config()
                .expect("an unbound listener exposes its credential")
                .username,
            "ambient-ops"
        );
    }

    #[test]
    fn dynamodb_listener_parses_access_key_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.cloudflare = false;
        command.mongodb = false;
        command.s3 = false;
        command.dynamodb_port = Some(8000);
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:sEcr3t/Key+=:demo".to_string()];
        let resolved =
            resolve(&command, temp.path(), |_| None).expect("valid binding should resolve");
        let dynamodb = resolved.dynamodb.expect("dynamodb config should resolve");
        assert_eq!(dynamodb.bind_addr, "127.0.0.1:8000".parse().unwrap());
        assert!(dynamodb.access_keys.binding("AKIDEXAMPLE").is_ok());
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "operator bindings must not touch the store"
        );
    }

    #[test]
    fn dynamodb_env_bindings_apply_without_flags() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        let resolved = resolve(&command, temp.path(), |name| {
            (name == DYNAMODB_ACCESS_KEYS_ENV)
                .then(|| "AKIDONE:s1:alpha, AKIDTWO:s2:beta".to_string())
        })
        .expect("env bindings should resolve");
        let dynamodb = resolved.dynamodb.expect("dynamodb config should resolve");
        assert!(dynamodb.access_keys.binding("AKIDONE").is_ok());
        assert!(dynamodb.access_keys.binding("AKIDTWO").is_ok());
    }

    #[test]
    fn dynamodb_bindings_without_port_apply_to_the_default_listener() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("bindings should apply to the default listener");
        let dynamodb = resolved.dynamodb.expect("dynamodb config should resolve");
        assert_eq!(dynamodb.bind_addr.port(), DYNAMODB_CONVENTIONAL_PORT);
        assert!(dynamodb.access_keys.binding("AKIDEXAMPLE").is_ok());
    }

    #[test]
    fn dynamodb_rejects_malformed_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        command.dynamodb_access_key = vec!["only-two:parts".to_string()];
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("malformed binding must be rejected");
        assert!(error.to_string().contains("ACCESS_KEY_ID:SECRET:TENANT"));
    }

    #[test]
    fn s3_listener_parses_access_key_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.cloudflare = false;
        command.mongodb = false;
        command.dynamodb = false;
        command.s3_port = Some(9000);
        command.s3_access_key = vec!["AKIAS3EXAMPLE:s3-secret:demo".to_string()];
        let resolved =
            resolve(&command, temp.path(), |_| None).expect("valid binding should resolve");
        let s3 = resolved.s3.expect("s3 config should resolve");
        assert_eq!(s3.bind_addr, "127.0.0.1:9000".parse().unwrap());
        assert!(s3.access_keys.binding("AKIAS3EXAMPLE").is_ok());
        assert!(
            s3.convex_download_secret.is_none(),
            "operator S3 bindings should not implicitly mint a Convex download signer"
        );
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "operator bindings must not touch the store"
        );
    }

    #[test]
    fn s3_env_bindings_apply_without_flags() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.s3_port = Some(9000);
        let resolved = resolve(&command, temp.path(), |name| {
            (name == S3_ACCESS_KEYS_ENV)
                .then(|| "AKIAS3ONE:s1:alpha, AKIAS3TWO:s2:beta".to_string())
        })
        .expect("env bindings should resolve");
        let s3 = resolved.s3.expect("s3 config should resolve");
        assert!(s3.access_keys.binding("AKIAS3ONE").is_ok());
        assert!(s3.access_keys.binding("AKIAS3TWO").is_ok());
        assert!(
            s3.convex_download_secret.is_none(),
            "env S3 bindings should not implicitly mint a Convex download signer"
        );
    }

    #[test]
    fn s3_listener_consumes_object_storage_env_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.s3_port = Some(9000);
        let resolved = resolve(&command, temp.path(), |name| match name {
            S3_ACCESS_KEYS_ENV => Some("AKIAS3ONE:s1:alpha".to_string()),
            "NIMBUS_OBJECT_STORAGE_MODE" => Some("mirror".to_string()),
            "NIMBUS_OBJECT_STORAGE_PROVIDER" => Some("memory".to_string()),
            "NIMBUS_OBJECT_STORAGE_BUCKET" => Some("tenant-mirror".to_string()),
            "NIMBUS_OBJECT_STORAGE_CREDENTIALS" => Some("anonymous".to_string()),
            "NIMBUS_OBJECT_STORAGE_REQUIRE_ACK" => Some("true".to_string()),
            _ => None,
        })
        .expect("object-storage env should resolve into S3 listener config");
        let s3 = resolved.s3.expect("s3 config should resolve");
        match s3.object_storage.default_policy() {
            nimbus::PlacementPolicy::Mirror {
                target,
                require_ack,
            } => {
                assert!(*require_ack);
                assert_eq!(target.bucket, "tenant-mirror");
                assert_eq!(target.provider, nimbus::ObjectStoreProviderKind::Memory);
                assert_eq!(
                    target.credentials,
                    nimbus::ObjectStoreProviderCredentials::Anonymous
                );
            }
            other => panic!("expected mirror default placement, got {other:?}"),
        }
    }

    #[test]
    fn s3_bindings_without_port_apply_to_the_default_listener() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.s3_access_key = vec!["AKIAS3EXAMPLE:secret:demo".to_string()];
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("bindings should apply to the default listener");
        let s3 = resolved.s3.expect("s3 config should resolve");
        assert_eq!(s3.bind_addr.port(), S3_CONVENTIONAL_PORT);
        assert!(s3.access_keys.binding("AKIAS3EXAMPLE").is_ok());
    }

    #[test]
    fn s3_rejects_malformed_bindings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.s3_port = Some(9000);
        command.s3_access_key = vec!["only-two:parts".to_string()];
        let error =
            resolve(&command, temp.path(), |_| None).expect_err("malformed binding must fail");
        assert!(error.to_string().contains("ACCESS_KEY_ID:SECRET:TENANT"));
    }

    #[test]
    fn mongodb_host_is_loopback_only_even_with_allow_network() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_host = "0.0.0.0".to_string();
        command.allow_network = true;
        let error = resolve(&command, temp.path(), |name| match name {
            MONGODB_USERNAME_ENV => Some("ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("secret".to_string()),
            _ => None,
        })
        .expect_err("the MongoDB listener must refuse non-loopback hosts outright");
        let message = error.to_string();
        assert!(
            message.contains("loopback-only") && message.contains("proxy"),
            "refusal should explain the posture, got: {message}"
        );
    }

    #[test]
    fn mongodb_bound_credentials_enable_a_tenant_bound_listener() {
        // M9a: NIMBUS_MONGODB_CREDENTIALS ingests per-tenant bindings, so the
        // listener runs in bound mode (authentication decides the tenant).
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        // Opt DynamoDB out so its store-backed default key can't create the
        // file this test asserts stays absent. Cloudflare also uses the
        // generated store credential by default, so it is out of this
        // MongoDB-only assertion too.
        command.dynamodb = false;
        command.cloudflare = false;
        command.s3 = false;
        let resolved = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV)
                .then(|| "user-a:tenant-a:secret-a,user-b:tenant-b:secret-b".to_string())
        })
        .expect("bound credentials should enable the listener");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert!(
            mongodb.is_tenant_bound(),
            "operator credentials must produce a bound listener"
        );
        assert!(
            mongodb.auth_config().is_none(),
            "bound mode exposes no single tenant-agnostic credential"
        );
        assert!(
            !wire_credentials_path(temp.path()).exists(),
            "operator credentials must not touch the store"
        );
    }

    #[test]
    fn mongodb_bound_credentials_admit_a_non_loopback_host_with_opt_in() {
        // The loopback-only check is relaxed in bound mode, gated by the same
        // --allow-network opt-in as the main and DynamoDB listeners.
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        command.mongodb_host = "0.0.0.0".to_string();

        // Without --allow-network the non-loopback bound host is still refused.
        let error = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV).then(|| "user-a:tenant-a:secret-a".to_string())
        })
        .expect_err("a non-loopback bound host without --allow-network must be refused");
        assert!(
            error.to_string().contains("--allow-network"),
            "refusal must name the opt-in flag, got: {error}"
        );

        // With --allow-network it binds the non-loopback address.
        command.allow_network = true;
        let resolved = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV).then(|| "user-a:tenant-a:secret-a".to_string())
        })
        .expect("--allow-network should admit the non-loopback bound host");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert!(mongodb.is_tenant_bound());
        assert_eq!(mongodb.bind_addr, "0.0.0.0:27017".parse().unwrap());
    }

    #[test]
    fn mongodb_malformed_bound_credentials_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let error = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV).then(|| "only-two:parts".to_string())
        })
        .expect_err("a malformed credential binding must be rejected");
        assert!(
            error.to_string().contains("USERNAME:TENANT:PASSWORD"),
            "refusal must name the expected format, got: {error}"
        );
    }

    #[test]
    fn mongodb_reserved_tenant_bound_credentials_are_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let error = resolve(&command, temp.path(), |name| {
            (name == MONGODB_CREDENTIALS_ENV)
                .then(|| "user-evil:_nimbus_internal:secret".to_string())
        })
        .expect_err("binding a reserved Nimbus-internal tenant must be refused");
        assert!(
            error.to_string().contains("reserved"),
            "refusal must explain the reserved-tenant rejection, got: {error}"
        );
    }

    #[test]
    fn mongodb_empty_credentials_env_keeps_the_unbound_loopback_path() {
        // An env present but empty (no bindings) is not "bound mode": today's
        // unbound, loopback-only behavior holds and a non-loopback host is
        // refused with the loopback-only message.
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.mongodb_port = Some(27017);
        let resolved = resolve(&command, temp.path(), |name| match name {
            MONGODB_CREDENTIALS_ENV => Some("  ,  ".to_string()),
            _ => None,
        })
        .expect("an empty credentials env should fall back to the unbound path");
        let mongodb = resolved.mongodb.expect("mongodb config should resolve");
        assert!(
            !mongodb.is_tenant_bound(),
            "an empty credentials env must not enable bound mode"
        );

        command.mongodb_host = "0.0.0.0".to_string();
        command.allow_network = true;
        let error = resolve(&command, temp.path(), |name| match name {
            MONGODB_CREDENTIALS_ENV => Some("".to_string()),
            MONGODB_USERNAME_ENV => Some("ops".to_string()),
            MONGODB_PASSWORD_ENV => Some("secret".to_string()),
            _ => None,
        })
        .expect_err("the unbound path must still refuse a non-loopback host");
        assert!(error.to_string().contains("loopback-only"));
    }

    #[test]
    fn dynamodb_listener_respects_the_network_opt_in_gate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.dynamodb_port = Some(8000);
        command.dynamodb_host = "0.0.0.0".to_string();
        command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("non-loopback dynamodb host without --allow-network must be refused");
        assert!(error.to_string().contains("--allow-network"));

        command.allow_network = true;
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("--allow-network should admit the non-loopback dynamodb host");
        assert_eq!(
            resolved
                .dynamodb
                .expect("dynamodb should resolve")
                .bind_addr,
            "0.0.0.0:8000".parse().unwrap()
        );
    }

    #[test]
    fn s3_listener_respects_the_network_opt_in_gate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut command = base_command();
        command.s3_port = Some(9000);
        command.s3_host = "0.0.0.0".to_string();
        command.s3_access_key = vec!["AKIAS3EXAMPLE:secret:demo".to_string()];
        let error = resolve(&command, temp.path(), |_| None)
            .expect_err("non-loopback s3 host without --allow-network must be refused");
        assert!(error.to_string().contains("--allow-network"));

        command.allow_network = true;
        let resolved = resolve(&command, temp.path(), |_| None)
            .expect("--allow-network should admit the non-loopback s3 host");
        assert_eq!(
            resolved.s3.expect("s3 should resolve").bind_addr,
            "0.0.0.0:9000".parse().unwrap()
        );
    }
}

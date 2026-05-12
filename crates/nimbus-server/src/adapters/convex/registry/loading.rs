use super::*;
use std::path::Component;

impl ConvexRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_app_dir(app_dir: impl AsRef<Path>) -> Result<Self, Error> {
        let convex_dir = app_dir.as_ref().join(".nimbus").join("convex");
        Self::from_manifest_paths(
            convex_dir.join("functions.json"),
            Some(convex_dir.join("http_routes.json")),
        )
    }

    pub fn from_manifest_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::from_manifest_paths(path, None::<&Path>)
    }

    pub fn from_manifest_paths(
        functions_path: impl AsRef<Path>,
        http_routes_path: Option<impl AsRef<Path>>,
    ) -> Result<Self, Error> {
        let path = functions_path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to read Convex manifest {}: {error}",
                path.display()
            ))
        })?;
        let manifest: ConvexManifest = serde_json::from_str(&contents).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to parse Convex manifest {}: {error}",
                path.display()
            ))
        })?;

        let functions = manifest
            .functions
            .into_iter()
            .map(|function| (function.name.clone(), function))
            .collect();
        let http_routes = match http_routes_path {
            Some(path) => read_http_route_manifest(path.as_ref())?,
            None => Vec::new(),
        };
        let schema = path
            .parent()
            .map(|directory| directory.join("schema.json"))
            .as_deref()
            .map(read_schema_manifest)
            .transpose()?
            .flatten();
        if let Some(directory) = path.parent() {
            read_node_external_packages_manifest(directory)?;
        }
        let runtime_bundle = path
            .parent()
            .map(|directory| directory.join("bundle.mjs"))
            .filter(|bundle_path| bundle_path.is_file())
            .map(|bundle_path| load_runtime_bundle(&bundle_path))
            .transpose()?;
        let auth_verifier = path
            .parent()
            .map(|directory| directory.join("auth.config.json"))
            .map(read_auth_config)
            .transpose()?
            .map(ConvexAuthVerifier::from_config)
            .map(Arc::new)
            .unwrap_or_else(|| Arc::new(ConvexAuthVerifier::empty()));

        let runtime_policy = Arc::new(RuntimePolicy::default());
        let runtime_executor = Arc::new(RuntimeExecutor::new(runtime_policy.clone()));
        let (node20_runtime_policy, node20_runtime_executor) =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node20);
        let (node22_runtime_policy, node22_runtime_executor) =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node22);
        let (node24_runtime_policy, node24_runtime_executor) =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node24);
        Ok(Self {
            functions,
            http_routes,
            schema,
            runtime_bundle,
            auth_verifier,
            runtime_policy,
            runtime_executor,
            node20_runtime_policy,
            node20_runtime_executor,
            node22_runtime_policy,
            node22_runtime_executor,
            node24_runtime_policy,
            node24_runtime_executor,
        })
    }

    pub fn with_runtime_limits(mut self, limits: RuntimeLimits) -> Self {
        let policy = Arc::new(RuntimePolicy::new(limits.clone()));
        self.runtime_policy = policy.clone();
        self.runtime_executor = Arc::new(RuntimeExecutor::new(policy));
        let (node20_policy, node20_executor) =
            convex_node_runtime_lane(limits.clone(), RuntimeCompatibilityTarget::Node20);
        self.node20_runtime_policy = node20_policy;
        self.node20_runtime_executor = node20_executor;
        let (node22_policy, node22_executor) =
            convex_node_runtime_lane(limits.clone(), RuntimeCompatibilityTarget::Node22);
        self.node22_runtime_policy = node22_policy;
        self.node22_runtime_executor = node22_executor;
        let (node24_policy, node24_executor) =
            convex_node_runtime_lane(limits, RuntimeCompatibilityTarget::Node24);
        self.node24_runtime_policy = node24_policy;
        self.node24_runtime_executor = node24_executor;
        self
    }
}

fn read_http_route_manifest(path: &Path) -> Result<Vec<ConvexHttpRouteDefinition>, Error> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::InvalidInput(format!(
                "failed to read convex HTTP route manifest {}: {error}",
                path.display()
            )));
        }
    };
    let manifest: ConvexHttpRouteManifest = serde_json::from_str(&contents).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse convex HTTP route manifest {}: {error}",
            path.display()
        ))
    })?;
    Ok(manifest.routes)
}

fn load_runtime_bundle(bundle_path: &Path) -> Result<RuntimeBundle, Error> {
    let hash_path = bundle_path.with_extension("sha256");
    let expected_sha256 = std::fs::read_to_string(&hash_path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read convex runtime bundle hash {}: {error}",
            hash_path.display()
        ))
    })?;
    RuntimeBundle::with_expected_sha256(bundle_path, expected_sha256).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to load convex runtime bundle {}: {error}",
            bundle_path.display()
        ))
    })
}

fn read_schema_manifest(path: &Path) -> Result<Option<Schema>, Error> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::InvalidInput(format!(
                "failed to read convex schema manifest {}: {error}",
                path.display()
            )));
        }
    };

    let manifest: ConvexSchemaManifest = serde_json::from_str(&contents).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse convex schema manifest {}: {error}",
            path.display()
        ))
    })?;
    let schema = manifest.into_schema()?;
    if let Some(schema) = &schema {
        validate_schema_manifest(schema)?;
    }
    Ok(schema)
}

fn read_node_external_packages_manifest(convex_dir: &Path) -> Result<(), Error> {
    let path = convex_dir.join("node_external_packages.json");
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(Error::InvalidInput(format!(
                "failed to read convex Node external packages manifest {}: {error}",
                path.display()
            )));
        }
    };
    let manifest: ConvexNodeExternalPackagesManifest =
        serde_json::from_str(&contents).map_err(|error| {
            Error::InvalidInput(format!(
                "failed to parse convex Node external packages manifest {}: {error}",
                path.display()
            ))
        })?;
    validate_node_external_packages_manifest(convex_dir, &manifest)
}

fn validate_node_external_packages_manifest(
    convex_dir: &Path,
    manifest: &ConvexNodeExternalPackagesManifest,
) -> Result<(), Error> {
    if manifest.version != 1 {
        return Err(Error::InvalidInput(format!(
            "convex Node external packages manifest version {} is unsupported; expected 1",
            manifest.version
        )));
    }
    validate_relative_manifest_path("stagingRoot", &manifest.staging_root)?;
    match manifest.mode {
        ConvexNodeExternalPackageMode::None => {
            if !manifest.configured_external_packages.is_empty() || !manifest.packages.is_empty() {
                return Err(Error::InvalidInput(
                    "convex Node external packages manifest mode `none` must not declare configured packages or package entries".to_string(),
                ));
            }
        }
        ConvexNodeExternalPackageMode::All => {
            if manifest.configured_external_packages.len() != 1
                || manifest.configured_external_packages[0] != "*"
            {
                return Err(Error::InvalidInput(
                    "convex Node external packages manifest mode `all` must declare configuredExternalPackages as [\"*\"]".to_string(),
                ));
            }
        }
        ConvexNodeExternalPackageMode::Explicit => {
            if manifest.configured_external_packages.is_empty()
                || manifest
                    .configured_external_packages
                    .iter()
                    .any(|package| package == "*")
            {
                return Err(Error::InvalidInput(
                    "convex Node external packages manifest mode `explicit` must declare explicit package names without `*`".to_string(),
                ));
            }
        }
    }

    let app_dir = convex_dir.parent().and_then(Path::parent).ok_or_else(|| {
        Error::InvalidInput(format!(
            "convex Node external packages manifest directory {} is not under .nimbus/convex",
            convex_dir.display()
        ))
    })?;
    let staging_root = app_dir.join(&manifest.staging_root);

    for package in &manifest.packages {
        if package.package_name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "convex Node external packages manifest contains a package with an empty packageName"
                    .to_string(),
            ));
        }
        if package.resolved_specifiers.is_empty() {
            return Err(Error::InvalidInput(format!(
                "convex Node external package `{}` must list at least one resolved specifier",
                package.package_name
            )));
        }
        if package.importers.is_empty() {
            return Err(Error::InvalidInput(format!(
                "convex Node external package `{}` must list at least one importer",
                package.package_name
            )));
        }
        if package.size_bytes == 0 {
            return Err(Error::InvalidInput(format!(
                "convex Node external package `{}` must report a non-zero sizeBytes value",
                package.package_name
            )));
        }
        let package_root = package.package_root.as_deref().ok_or_else(|| {
            Error::InvalidInput(format!(
                "convex Node external package `{}` is missing packageRoot",
                package.package_name
            ))
        })?;
        let staged_package_root = package.staged_package_root.as_deref().ok_or_else(|| {
            Error::InvalidInput(format!(
                "convex Node external package `{}` is missing stagedPackageRoot",
                package.package_name
            ))
        })?;
        validate_relative_manifest_path("packageRoot", package_root)?;
        validate_relative_manifest_path("stagedPackageRoot", staged_package_root)?;
        for importer in &package.importers {
            validate_relative_manifest_path("importer.file", &importer.file)?;
            if importer.kind.trim().is_empty() || importer.specifier.trim().is_empty() {
                return Err(Error::InvalidInput(format!(
                    "convex Node external package `{}` has an importer with an empty kind or specifier",
                    package.package_name
                )));
            }
        }
        let staged_path = app_dir.join(staged_package_root);
        if !staged_path.starts_with(&staging_root) {
            return Err(Error::InvalidInput(format!(
                "convex Node external package `{}` staged path must be under stagingRoot",
                package.package_name
            )));
        }
        if !staged_path.is_dir() {
            return Err(Error::InvalidInput(format!(
                "convex Node external package `{}` staged path {} does not exist or is not a directory",
                package.package_name,
                staged_path.display()
            )));
        }
    }

    Ok(())
}

fn validate_relative_manifest_path(field: &str, value: &str) -> Result<(), Error> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(Error::InvalidInput(format!(
            "convex Node external packages manifest field `{field}` must be a non-empty relative path without parent traversal"
        )));
    }
    Ok(())
}

fn validate_schema_manifest(schema: &Schema) -> Result<(), Error> {
    for table_schema in schema.tables.values() {
        table_schema.validate_indexes()?;
        table_schema.validate_access_policy()?;
    }
    Ok(())
}

use super::*;

impl ConvexRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_app_dir(app_dir: impl AsRef<Path>) -> Result<Self, Error> {
        let convex_dir = app_dir.as_ref().join(".neovex").join("convex");
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
        Ok(Self {
            functions,
            http_routes,
            schema,
            runtime_bundle,
            auth_verifier,
            runtime_policy,
            runtime_executor,
        })
    }

    pub fn with_runtime_limits(mut self, limits: RuntimeLimits) -> Self {
        let policy = Arc::new(RuntimePolicy::new(limits));
        self.runtime_policy = policy.clone();
        self.runtime_executor = Arc::new(RuntimeExecutor::new(policy));
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

fn validate_schema_manifest(schema: &Schema) -> Result<(), Error> {
    for table_schema in schema.tables.values() {
        table_schema.validate_indexes()?;
        table_schema.validate_access_policy()?;
    }
    Ok(())
}

use super::*;

impl LicenseState {
    pub fn community() -> Self {
        Self {
            source: LicenseSourceInfo {
                kind: LicenseSourceKind::CommunityDefault,
                path: None,
            },
            document: LicenseDocument {
                schema_version: license_schema_version(),
                kind: LicenseKind::Community,
                issued_to: None,
                issued_by: None,
                issued_at_unix_ms: None,
                expires_at_unix_ms: None,
                trial_expires_at_unix_ms: None,
                revenue_limit_usd: Some(10_000_000),
                monthly_active_user_limit: Some(500),
                entitlements: LicenseEntitlements::default(),
                notes: None,
            },
        }
    }

    pub fn from_document(document: LicenseDocument, source: LicenseSourceInfo) -> Self {
        Self { source, document }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, LicenseLoadError> {
        Self::load_path(path.as_ref(), LicenseSourceKind::ExplicitFile)
    }

    pub fn load(explicit_path: Option<&Path>) -> Result<Self, LicenseLoadError> {
        if let Some(path) = explicit_path {
            return Self::load_path(path, LicenseSourceKind::ExplicitFile);
        }

        if let Some(path) = env::var_os(LICENSE_FILE_ENV) {
            return Self::load_path(Path::new(&path), LicenseSourceKind::EnvironmentFile);
        }

        let default_path = PathBuf::from(DEFAULT_LICENSE_PATH);
        if default_path.exists() {
            return Self::load_path(&default_path, LicenseSourceKind::DefaultPath);
        }

        Ok(Self::community())
    }

    fn load_path(path: &Path, source_kind: LicenseSourceKind) -> Result<Self, LicenseLoadError> {
        let display_path = path.display().to_string();
        let raw = fs::read_to_string(path).map_err(|source| LicenseLoadError::Read {
            path: display_path.clone(),
            source,
        })?;
        let document = serde_json::from_str(&raw).map_err(|source| LicenseLoadError::Parse {
            path: display_path.clone(),
            source,
        })?;
        Ok(Self {
            source: LicenseSourceInfo {
                kind: source_kind,
                path: Some(display_path),
            },
            document,
        })
    }
}

//! Protected subject models for local encryption.

use nimbus_core::TenantId;

/// The kind of protected local subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalKeySubjectKind {
    /// A local database file.
    Database(LocalDatabaseRole),
    /// A persisted artifact file (e.g., migration copy, rebuild staging file).
    Artifact(LocalArtifactRole),
}

impl LocalKeySubjectKind {
    /// Returns a stable string identifier for the subject kind, used in key derivation.
    pub fn derivation_tag(&self) -> &'static str {
        match self {
            Self::Database(role) => role.derivation_tag(),
            Self::Artifact(role) => role.derivation_tag(),
        }
    }
}

/// The role of a protected local database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDatabaseRole {
    /// Embedded SQLite tenant database.
    EmbeddedSqliteTenant,
    /// Embedded redb tenant database.
    EmbeddedRedbTenant,
    /// The retained redb control-plane database.
    ControlPlaneRedb,
    /// Local libsql replica cache database.
    LibsqlReplicaCache,
}

impl LocalDatabaseRole {
    /// Returns a stable string identifier for the role, used in key derivation.
    pub fn derivation_tag(&self) -> &'static str {
        match self {
            Self::EmbeddedSqliteTenant => "db:sqlite:tenant",
            Self::EmbeddedRedbTenant => "db:redb:tenant",
            Self::ControlPlaneRedb => "db:redb:control",
            Self::LibsqlReplicaCache => "db:libsql:cache",
        }
    }
}

/// The role of a protected persisted artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalArtifactRole {
    /// Migration working copy.
    MigrationCopy,
    /// Rebuild staging file.
    RebuildStaging,
    /// Retired replica cache pending cleanup.
    RetiredReplicaCache,
    /// Snapshot export file.
    SnapshotExport,
    /// Bootstrap bundle file.
    BootstrapBundle,
}

impl LocalArtifactRole {
    /// Returns a stable string identifier for the role, used in key derivation.
    pub fn derivation_tag(&self) -> &'static str {
        match self {
            Self::MigrationCopy => "artifact:migration",
            Self::RebuildStaging => "artifact:rebuild",
            Self::RetiredReplicaCache => "artifact:retired-cache",
            Self::SnapshotExport => "artifact:snapshot",
            Self::BootstrapBundle => "artifact:bootstrap",
        }
    }
}

/// Identifies a protected local subject for key management.
///
/// This struct provides all the metadata needed to derive or locate a subject's
/// encryption key, authenticate the wrapped key envelope, and generate safe
/// diagnostic descriptors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalKeySubject {
    /// The kind of protected subject (database or artifact).
    pub kind: LocalKeySubjectKind,

    /// The tenant ID if this is a tenant-scoped subject.
    pub tenant_id: Option<TenantId>,

    /// A logical name for the subject, typically derived from the file path.
    pub logical_name: String,
}

impl LocalKeySubject {
    /// Creates a new subject for an embedded SQLite tenant database.
    pub fn sqlite_tenant(tenant_id: TenantId, logical_name: impl Into<String>) -> Self {
        Self {
            kind: LocalKeySubjectKind::Database(LocalDatabaseRole::EmbeddedSqliteTenant),
            tenant_id: Some(tenant_id),
            logical_name: logical_name.into(),
        }
    }

    /// Creates a new subject for an embedded redb tenant database.
    pub fn redb_tenant(tenant_id: TenantId, logical_name: impl Into<String>) -> Self {
        Self {
            kind: LocalKeySubjectKind::Database(LocalDatabaseRole::EmbeddedRedbTenant),
            tenant_id: Some(tenant_id),
            logical_name: logical_name.into(),
        }
    }

    /// Creates a new subject for the control-plane redb database.
    pub fn control_plane(logical_name: impl Into<String>) -> Self {
        Self {
            kind: LocalKeySubjectKind::Database(LocalDatabaseRole::ControlPlaneRedb),
            tenant_id: None,
            logical_name: logical_name.into(),
        }
    }

    /// Creates a new subject for a libsql replica cache database.
    pub fn libsql_cache(tenant_id: TenantId, logical_name: impl Into<String>) -> Self {
        Self {
            kind: LocalKeySubjectKind::Database(LocalDatabaseRole::LibsqlReplicaCache),
            tenant_id: Some(tenant_id),
            logical_name: logical_name.into(),
        }
    }

    /// Creates a new subject for a migration working copy.
    pub fn migration_copy(tenant_id: Option<TenantId>, logical_name: impl Into<String>) -> Self {
        Self {
            kind: LocalKeySubjectKind::Artifact(LocalArtifactRole::MigrationCopy),
            tenant_id,
            logical_name: logical_name.into(),
        }
    }

    /// Returns the key derivation context for this subject.
    ///
    /// This is used as input to HKDF when deriving per-subject keys from a master key.
    pub fn derivation_context(&self) -> Vec<u8> {
        let mut context = Vec::new();

        // Add the kind tag
        context.extend_from_slice(self.kind.derivation_tag().as_bytes());
        context.push(0); // null separator

        // Add the tenant ID if present
        if let Some(ref tenant_id) = self.tenant_id {
            context.extend_from_slice(tenant_id.as_str().as_bytes());
        }
        context.push(0); // null separator

        // Add the logical name
        context.extend_from_slice(self.logical_name.as_bytes());

        context
    }

    /// Returns a diagnostics-safe descriptor for this subject.
    ///
    /// This does not include any key material.
    pub fn descriptor(&self) -> String {
        let kind_tag = self.kind.derivation_tag();
        if let Some(ref tenant_id) = self.tenant_id {
            format!("{}:{}:{}", kind_tag, tenant_id.as_str(), self.logical_name)
        } else {
            format!("{}:{}", kind_tag, self.logical_name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_context_includes_all_fields() {
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "demo.sqlite3");

        let context = subject.derivation_context();

        // Should contain the role tag, tenant id, and logical name
        assert!(context.len() > 20);
        assert!(context.starts_with(b"db:sqlite:tenant\0"));
    }

    #[test]
    fn control_plane_subject_has_no_tenant() {
        let subject = LocalKeySubject::control_plane("nimbus-control.db");

        assert!(subject.tenant_id.is_none());
        let descriptor = subject.descriptor();
        assert!(descriptor.contains("control"));
        assert!(descriptor.contains("nimbus-control.db"));
    }

    #[test]
    fn subject_descriptor_is_diagnostics_safe() {
        let tenant_id = TenantId::new("secret-tenant").expect("tenant id should build");
        let subject = LocalKeySubject::sqlite_tenant(tenant_id, "secret.sqlite3");

        let descriptor = subject.descriptor();

        // Descriptor contains identifiers but no key material
        assert!(descriptor.contains("secret-tenant"));
        assert!(descriptor.contains("secret.sqlite3"));
        // Should not contain any actual cryptographic material
        assert!(!descriptor.contains("key"));
    }
}

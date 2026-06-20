use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result};

use super::key::{DataEncryptionKey, GeneratedDataKey};
use super::manifest::{KeyManifest, KeyManifestHeader, ManifestCipher, ManifestReadError};
use super::materials::{EnvelopeKeyring, map_key_provider_error, validate_manifest};
use super::provider::LocalKeyProvider;
use super::subject::LocalKeySubject;

fn map_manifest_read_error(error: ManifestReadError) -> Error {
    match error {
        ManifestReadError::IoError { path, source } => {
            if source.kind() == io::ErrorKind::NotFound {
                Error::InvalidInput(format!("encryption manifest not found: {}", path.display()))
            } else if source.kind() == io::ErrorKind::PermissionDenied {
                Error::PermissionDenied(format!(
                    "cannot read encryption manifest (check permissions): {}",
                    path.display()
                ))
            } else {
                Error::Internal(format!(
                    "failed to read encryption manifest {}: {source}",
                    path.display()
                ))
            }
        }
        ManifestReadError::ParseError { path, message } => Error::InvalidInput(format!(
            "invalid encryption manifest {}: {message}",
            path.display()
        )),
        ManifestReadError::UnsupportedVersion { path, version } => Error::InvalidInput(format!(
            "unsupported encryption manifest version {version} at {}",
            path.display()
        )),
    }
}

fn map_manifest_write_error(path: &Path, error: impl std::fmt::Display) -> Error {
    Error::Internal(format!(
        "failed to write encryption manifest for {}: {error}",
        path.display()
    ))
}

pub fn generate_key_manifest(
    provider: &dyn LocalKeyProvider,
    subject: &LocalKeySubject,
    cipher: ManifestCipher,
) -> Result<(KeyManifest, GeneratedDataKey)> {
    let header = KeyManifestHeader {
        version: super::manifest::MANIFEST_VERSION,
        cipher,
        subject_descriptor: subject.descriptor(),
        key_provider: provider.kind(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
        rotated_at: std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
    };
    let generated = provider
        .generate_data_key(subject, &header)
        .map_err(map_key_provider_error)?;
    let manifest = KeyManifest {
        header,
        wrapped_key: generated.wrapped().clone(),
    };
    Ok((manifest, generated))
}

pub fn unwrap_key_manifest(
    manifest: &KeyManifest,
    provider: &dyn LocalKeyProvider,
    subject: &LocalKeySubject,
    expected_cipher: ManifestCipher,
    protected_path: &Path,
) -> Result<DataEncryptionKey> {
    validate_manifest(manifest, provider, subject, expected_cipher, protected_path)?;
    provider
        .unwrap_data_key(subject, &manifest.wrapped_key, &manifest.header)
        .map_err(map_key_provider_error)
}

pub fn resolve_subject_encryption_key(
    protected_path: &Path,
    provider: &dyn LocalKeyProvider,
    subject: &LocalKeySubject,
    cipher: ManifestCipher,
) -> Result<DataEncryptionKey> {
    if cipher == ManifestCipher::RedbAes256GcmSiv {
        super::rotation::recover_interrupted_dek_rotation(protected_path)?;
    }
    let manifest_path = KeyManifest::manifest_path(protected_path);
    if manifest_path.exists() {
        let read_started = Instant::now();
        let manifest = KeyManifest::read(&manifest_path).map_err(map_manifest_read_error)?;
        let read_elapsed = read_started.elapsed();
        let unwrap_started = Instant::now();
        let keyring = EnvelopeKeyring::new(provider);
        let key = keyring
            .open_materials(&manifest, subject, cipher, protected_path)?
            .into_data_key()
            .map_err(|error| Error::PermissionDenied(error.to_string()))?;
        if encryption_profile_enabled(protected_path) {
            trace_encryption_profile_unwrap(read_elapsed, unwrap_started.elapsed());
        }
        return Ok(key);
    }

    if protected_path.exists() {
        return Err(Error::InvalidInput(format!(
            "encryption is enabled for {}, but the sidecar manifest is missing; migrate the plaintext database before enabling encryption",
            protected_path.display()
        )));
    }

    let generate_started = Instant::now();
    let keyring = EnvelopeKeyring::new(provider);
    let materials = keyring.generate_materials(subject, cipher)?;
    let manifest = materials.manifest.clone();
    let generate_elapsed = generate_started.elapsed();
    let write_started = Instant::now();
    manifest
        .write_for(protected_path)
        .map_err(|error| map_manifest_write_error(protected_path, error))?;
    let write_elapsed = write_started.elapsed();
    if encryption_profile_enabled(protected_path) {
        trace_encryption_profile_generate(generate_elapsed, write_elapsed);
    }
    materials
        .into_data_key()
        .map_err(|error| Error::PermissionDenied(error.to_string()))
}

fn trace_encryption_profile_unwrap(read_elapsed: Duration, unwrap_elapsed: Duration) {
    tracing::debug!(
        target: "nimbus_crypto",
        action = "unwrap-manifest",
        read_ms = read_elapsed.as_millis(),
        unwrap_ms = unwrap_elapsed.as_millis(),
        total_ms = (read_elapsed + unwrap_elapsed).as_millis(),
        "subject encryption key resolved"
    );
}

fn trace_encryption_profile_generate(generate_elapsed: Duration, write_elapsed: Duration) {
    tracing::debug!(
        target: "nimbus_crypto",
        action = "generate-manifest",
        generate_ms = generate_elapsed.as_millis(),
        write_ms = write_elapsed.as_millis(),
        total_ms = (generate_elapsed + write_elapsed).as_millis(),
        "subject encryption manifest generated"
    );
}

fn encryption_profile_enabled(path: &Path) -> bool {
    std::env::var_os("NIMBUS_ENCRYPTION_PROFILE").is_some() && profile_scope_allows_path(path)
}

fn profile_scope_allows_path(path: &Path) -> bool {
    if std::env::var_os("NIMBUS_PROFILE_ONLY_COLD_SAMPLES").is_none() {
        return true;
    }

    path.to_string_lossy().contains("cold-sample")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::subscriber::Interest;
    use tracing::{Event, Metadata, Subscriber};

    use super::trace_encryption_profile_unwrap;

    #[derive(Clone, Debug, Default)]
    struct CapturingSubscriber {
        events: Arc<Mutex<Vec<CapturedEvent>>>,
    }

    #[derive(Clone, Debug, Default)]
    struct CapturedEvent {
        target: String,
        fields: BTreeMap<String, String>,
    }

    impl Subscriber for CapturingSubscriber {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut visitor = EventFieldVisitor::default();
            event.record(&mut visitor);
            self.events
                .lock()
                .expect("event buffer should be available")
                .push(CapturedEvent {
                    target: event.metadata().target().to_string(),
                    fields: visitor.fields,
                });
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}

        fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
            Interest::always()
        }
    }

    #[derive(Debug, Default)]
    struct EventFieldVisitor {
        fields: BTreeMap<String, String>,
    }

    impl Visit for EventFieldVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.fields
                .insert(field.name().to_string(), format!("{value:?}"));
        }
    }

    #[test]
    fn encryption_profile_trace_is_structured_and_path_free() {
        let subscriber = CapturingSubscriber::default();
        let events = subscriber.events.clone();
        tracing::dispatcher::with_default(&tracing::Dispatch::new(subscriber), || {
            trace_encryption_profile_unwrap(Duration::from_millis(2), Duration::from_millis(3));
        });

        let events = events.lock().expect("event buffer should be available");
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.target, "nimbus_crypto");
        assert_eq!(
            event.fields.get("action").map(String::as_str),
            Some("\"unwrap-manifest\"")
        );
        assert_eq!(event.fields.get("read_ms").map(String::as_str), Some("2"));
        assert_eq!(event.fields.get("unwrap_ms").map(String::as_str), Some("3"));
        assert_eq!(event.fields.get("total_ms").map(String::as_str), Some("5"));
        assert!(
            !event.fields.contains_key("path"),
            "profile traces must not expose protected database paths"
        );
    }
}

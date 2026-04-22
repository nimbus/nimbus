//! AWS KMS-backed key provider.
//!
//! This provider participates in the same manifest-backed envelope contract as
//! the local providers. The sidecar manifest remains the source of truth for
//! protected-path metadata; AWS KMS only replaces the wrapping provider.

use std::collections::HashMap;
use std::future::Future;

use aws_config::BehaviorVersion;
use aws_sdk_kms::Client;
use aws_sdk_kms::config::Region;
use aws_sdk_kms::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_kms::primitives::Blob;
use aws_sdk_kms::types::DataKeySpec;
use tokio::runtime::Handle;
use zeroize::Zeroize;

use super::key::{GeneratedDatabaseKey, WrappedDatabaseKey, WrappingCipher};
use super::manifest::KeyManifestHeader;
use super::provider::{
    KeyProviderKind, KeyProviderResult, LocalKeyProvider, LocalKeyProviderError,
};
use super::subject::LocalKeySubject;

const CONTEXT_VERSION_KEY: &str = "neovex:manifest_version";
const CONTEXT_CIPHER_KEY: &str = "neovex:manifest_cipher";
const CONTEXT_SUBJECT_KEY: &str = "neovex:subject_descriptor";
const CONTEXT_PROVIDER_KEY: &str = "neovex:key_provider";
const CONTEXT_CREATED_AT_KEY: &str = "neovex:created_at";
const CONTEXT_ROTATED_AT_KEY: &str = "neovex:rotated_at";

/// AWS KMS provider that wraps per-subject DEKs in KMS-managed ciphertext.
#[derive(Clone)]
pub struct AwsKmsKeyProvider {
    client: Client,
    key_id: String,
    region: Option<String>,
}

impl AwsKmsKeyProvider {
    /// Creates a new AWS KMS provider from operator configuration.
    pub fn new(
        key_id: impl Into<String>,
        region: Option<String>,
        endpoint_url: Option<String>,
    ) -> KeyProviderResult<Self> {
        let key_id = key_id.into();
        let region_for_loader = region.clone();
        let endpoint_for_builder = endpoint_url.clone();

        let shared_config = block_on_future(async move {
            let mut loader = aws_config::defaults(BehaviorVersion::latest());
            if let Some(region) = region_for_loader {
                loader = loader.region(Region::new(region));
            }
            loader.load().await
        })?;

        let mut config_builder = aws_sdk_kms::config::Builder::from(&shared_config);
        if let Some(endpoint_url) = endpoint_for_builder {
            config_builder = config_builder.endpoint_url(endpoint_url);
        }

        Ok(Self {
            client: Client::from_conf(config_builder.build()),
            key_id,
            region,
        })
    }

    fn encryption_context(header: &KeyManifestHeader) -> HashMap<String, String> {
        HashMap::from([
            (CONTEXT_VERSION_KEY.to_string(), header.version.to_string()),
            (
                CONTEXT_CIPHER_KEY.to_string(),
                header.cipher.as_str().to_string(),
            ),
            (
                CONTEXT_SUBJECT_KEY.to_string(),
                header.subject_descriptor.clone(),
            ),
            (
                CONTEXT_PROVIDER_KEY.to_string(),
                header.key_provider.to_string(),
            ),
            (
                CONTEXT_CREATED_AT_KEY.to_string(),
                header.created_at.to_string(),
            ),
            (
                CONTEXT_ROTATED_AT_KEY.to_string(),
                header.rotated_at.to_string(),
            ),
        ])
    }

    fn extract_plaintext_key(blob: Blob, operation: &'static str) -> KeyProviderResult<[u8; 32]> {
        let mut bytes = blob.into_inner();
        if bytes.len() != 32 {
            let actual = bytes.len();
            bytes.zeroize();
            return Err(LocalKeyProviderError::UnwrapError {
                message: format!(
                    "aws kms {operation} returned {actual} plaintext bytes instead of 32"
                ),
            });
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        bytes.zeroize();
        Ok(key)
    }
}

impl LocalKeyProvider for AwsKmsKeyProvider {
    fn generate_database_key(
        &self,
        _subject: &LocalKeySubject,
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<GeneratedDatabaseKey> {
        let client = self.client.clone();
        let key_id = self.key_id.clone();
        let context = Self::encryption_context(header);
        let mut output = block_on_future(async move {
            client
                .generate_data_key()
                .key_id(key_id)
                .key_spec(DataKeySpec::Aes256)
                .set_encryption_context(Some(context))
                .send()
                .await
        })?
        .map_err(|error| map_sdk_error("GenerateDataKey", &self.key_id, error))?;

        let plaintext_blob =
            output
                .plaintext
                .take()
                .ok_or_else(|| LocalKeyProviderError::AwsKmsOperationError {
                    operation: "GenerateDataKey",
                    message: "response did not include plaintext".to_string(),
                })?;
        let ciphertext_blob = output.ciphertext_blob.take().ok_or_else(|| {
            LocalKeyProviderError::AwsKmsOperationError {
                operation: "GenerateDataKey",
                message: "response did not include ciphertext".to_string(),
            }
        })?;

        let plaintext = Self::extract_plaintext_key(plaintext_blob, "GenerateDataKey")?;
        let wrapped = WrappedDatabaseKey::new(WrappingCipher::AwsKms, ciphertext_blob.into_inner());
        Ok(GeneratedDatabaseKey::new(plaintext, wrapped))
    }

    fn unwrap_database_key(
        &self,
        _subject: &LocalKeySubject,
        wrapped: &WrappedDatabaseKey,
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<[u8; 32]> {
        if wrapped.cipher != WrappingCipher::AwsKms {
            return Err(LocalKeyProviderError::UnsupportedCipher {
                cipher: wrapped.cipher.as_str().to_string(),
            });
        }

        let client = self.client.clone();
        let ciphertext = wrapped.ciphertext.clone();
        let context = Self::encryption_context(header);
        let mut output = block_on_future(async move {
            client
                .decrypt()
                .ciphertext_blob(Blob::new(ciphertext))
                .set_encryption_context(Some(context))
                .send()
                .await
        })?
        .map_err(|error| map_sdk_error("Decrypt", &self.key_id, error))?;

        let plaintext_blob =
            output
                .plaintext
                .take()
                .ok_or_else(|| LocalKeyProviderError::UnwrapError {
                    message: "aws kms decrypt returned no plaintext".to_string(),
                })?;

        Self::extract_plaintext_key(plaintext_blob, "Decrypt")
    }

    fn rewrap_database_key(
        &self,
        _subject: &LocalKeySubject,
        plaintext: &[u8; 32],
        header: &KeyManifestHeader,
    ) -> KeyProviderResult<WrappedDatabaseKey> {
        let client = self.client.clone();
        let key_id = self.key_id.clone();
        let plaintext = plaintext.to_vec();
        let context = Self::encryption_context(header);
        let mut output = block_on_future(async move {
            client
                .encrypt()
                .key_id(key_id)
                .plaintext(Blob::new(plaintext))
                .set_encryption_context(Some(context))
                .send()
                .await
        })?
        .map_err(|error| map_sdk_error("Encrypt", &self.key_id, error))?;

        let ciphertext_blob = output.ciphertext_blob.take().ok_or_else(|| {
            LocalKeyProviderError::AwsKmsOperationError {
                operation: "Encrypt",
                message: "response did not include ciphertext".to_string(),
            }
        })?;
        Ok(WrappedDatabaseKey::new(
            WrappingCipher::AwsKms,
            ciphertext_blob.into_inner(),
        ))
    }

    fn rewrap_wrapped_database_key(
        &self,
        _subject: &LocalKeySubject,
        wrapped: &WrappedDatabaseKey,
        current_header: &KeyManifestHeader,
        new_header: &KeyManifestHeader,
    ) -> KeyProviderResult<Option<WrappedDatabaseKey>> {
        if wrapped.cipher != WrappingCipher::AwsKms {
            return Ok(None);
        }

        let client = self.client.clone();
        let key_id = self.key_id.clone();
        let ciphertext = wrapped.ciphertext.clone();
        let source_context = Self::encryption_context(current_header);
        let destination_context = Self::encryption_context(new_header);
        let mut output = block_on_future(async move {
            client
                .re_encrypt()
                .ciphertext_blob(Blob::new(ciphertext))
                .destination_key_id(key_id)
                .set_source_encryption_context(Some(source_context))
                .set_destination_encryption_context(Some(destination_context))
                .send()
                .await
        })?
        .map_err(|error| map_sdk_error("ReEncrypt", &self.key_id, error))?;

        let ciphertext_blob = output.ciphertext_blob.take().ok_or_else(|| {
            LocalKeyProviderError::AwsKmsOperationError {
                operation: "ReEncrypt",
                message: "response did not include ciphertext".to_string(),
            }
        })?;
        Ok(Some(WrappedDatabaseKey::new(
            WrappingCipher::AwsKms,
            ciphertext_blob.into_inner(),
        )))
    }

    fn kind(&self) -> KeyProviderKind {
        KeyProviderKind::AwsKms {
            key_id: self.key_id.clone(),
            region: self.region.clone(),
        }
    }
}

fn block_on_future<F, T>(future: F) -> KeyProviderResult<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    if let Ok(handle) = Handle::try_current() {
        let join = std::thread::spawn(move || handle.block_on(future));
        return join
            .join()
            .map_err(|_| LocalKeyProviderError::AwsKmsOperationError {
                operation: "runtime-bridge",
                message: "aws kms bridge thread panicked".to_string(),
            });
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| LocalKeyProviderError::AwsKmsConfigurationError {
            message: format!("failed to create tokio runtime for aws kms: {error}"),
        })?;
    Ok(runtime.block_on(future))
}

fn map_sdk_error<E>(
    operation: &'static str,
    key_id: &str,
    error: SdkError<E>,
) -> LocalKeyProviderError
where
    E: ProvideErrorMetadata + std::fmt::Display + Send + Sync + 'static,
{
    match error {
        SdkError::ConstructionFailure(source) => LocalKeyProviderError::AwsKmsConfigurationError {
            message: format!("{operation} request construction failed: {source:?}"),
        },
        SdkError::TimeoutError(source) => LocalKeyProviderError::AwsKmsNetworkError {
            operation,
            message: format!("{source:?}"),
        },
        SdkError::DispatchFailure(source) => LocalKeyProviderError::AwsKmsNetworkError {
            operation,
            message: format!("{source:?}"),
        },
        SdkError::ResponseError(source) => LocalKeyProviderError::AwsKmsOperationError {
            operation,
            message: format!("{source:?}"),
        },
        SdkError::ServiceError(context) => {
            let service_error = context.into_err();
            match service_error.code() {
                Some("InvalidCiphertextException") => LocalKeyProviderError::UnwrapError {
                    message:
                        "decryption failed (wrong key, wrong encryption context, or corrupted ciphertext)"
                            .to_string(),
                },
                Some("NotFoundException") => LocalKeyProviderError::AwsKmsKeyNotFound {
                    key_id: key_id.to_string(),
                },
                Some("AccessDeniedException") => LocalKeyProviderError::AwsKmsPermissionDenied {
                    operation,
                    message: service_error.to_string(),
                },
                Some("UnrecognizedClientException" | "InvalidSignatureException"
                | "InvalidClientTokenId" | "ExpiredTokenException") => {
                    LocalKeyProviderError::AwsKmsAuthError {
                        message: service_error.to_string(),
                    }
                }
                Some(_) | None => LocalKeyProviderError::AwsKmsOperationError {
                    operation,
                    message: service_error.to_string(),
                },
            }
        }
        _ => LocalKeyProviderError::AwsKmsOperationError {
            operation,
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use hyper::body::to_bytes;
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Request, Response, Server, StatusCode};
    use neovex_core::TenantId;
    use serde_json::{Value, json};
    use serial_test::serial;

    use super::*;
    use crate::encryption::manifest::{MANIFEST_VERSION, ManifestCipher};

    #[derive(Debug, Clone)]
    struct ResponseSpec {
        target: &'static str,
        body: Value,
        status: StatusCode,
    }

    #[derive(Debug, Clone)]
    struct RequestRecord {
        target: String,
        body: Value,
    }

    struct TestKmsServer {
        endpoint_url: String,
        requests: Arc<Mutex<Vec<RequestRecord>>>,
    }

    impl TestKmsServer {
        async fn start(responses: Vec<ResponseSpec>) -> Self {
            let requests = Arc::new(Mutex::new(Vec::new()));
            let responses = Arc::new(Mutex::new(VecDeque::from(responses)));

            let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
            let addr = listener.local_addr().expect("listener should have address");

            let requests_for_server = Arc::clone(&requests);
            let responses_for_server = Arc::clone(&responses);
            tokio::spawn(async move {
                let make_service = make_service_fn(move |_| {
                    let requests = Arc::clone(&requests_for_server);
                    let responses = Arc::clone(&responses_for_server);
                    async move {
                        Ok::<_, Infallible>(service_fn(move |request: Request<Body>| {
                            let requests = Arc::clone(&requests);
                            let responses = Arc::clone(&responses);
                            async move {
                                let target = request
                                    .headers()
                                    .get("x-amz-target")
                                    .and_then(|value| value.to_str().ok())
                                    .unwrap_or_default()
                                    .to_string();
                                let body = to_bytes(request.into_body()).await.unwrap();
                                let body_json: Value = serde_json::from_slice(&body)
                                    .expect("kms request should be json");
                                requests.lock().unwrap().push(RequestRecord {
                                    target: target.clone(),
                                    body: body_json,
                                });

                                let response = responses
                                    .lock()
                                    .unwrap()
                                    .pop_front()
                                    .expect("response queue should be populated");
                                if target != response.target {
                                    let body = Body::from(format!(
                                        "unexpected target {target}, expected {}",
                                        response.target
                                    ));
                                    return Ok::<_, Infallible>(
                                        Response::builder()
                                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                                            .body(body)
                                            .unwrap(),
                                    );
                                }

                                Ok::<_, Infallible>(
                                    Response::builder()
                                        .status(response.status)
                                        .header("content-type", "application/x-amz-json-1.1")
                                        .header("x-amzn-RequestId", "test-request-id")
                                        .body(Body::from(
                                            serde_json::to_vec(&response.body)
                                                .expect("kms response should serialize"),
                                        ))
                                        .unwrap(),
                                )
                            }
                        }))
                    }
                });

                Server::from_tcp(listener)
                    .expect("hyper server should start")
                    .serve(make_service)
                    .await
                    .expect("hyper server should serve");
            });

            Self {
                endpoint_url: format!("http://{addr}"),
                requests,
            }
        }

        fn requests(&self) -> Vec<RequestRecord> {
            self.requests.lock().unwrap().clone()
        }
    }

    struct AwsEnvGuard {
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
        session_token: Option<String>,
        profile: Option<String>,
        ec2_metadata_disabled: Option<String>,
    }

    impl AwsEnvGuard {
        fn install() -> Self {
            let guard = Self {
                access_key_id: std::env::var("AWS_ACCESS_KEY_ID").ok(),
                secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
                session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
                profile: std::env::var("AWS_PROFILE").ok(),
                ec2_metadata_disabled: std::env::var("AWS_EC2_METADATA_DISABLED").ok(),
            };

            unsafe {
                std::env::set_var("AWS_ACCESS_KEY_ID", "neovex-test");
                std::env::set_var("AWS_SECRET_ACCESS_KEY", "neovex-test-secret");
                std::env::remove_var("AWS_SESSION_TOKEN");
                std::env::remove_var("AWS_PROFILE");
                std::env::set_var("AWS_EC2_METADATA_DISABLED", "true");
            }

            guard
        }
    }

    impl Drop for AwsEnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.access_key_id {
                    Some(value) => std::env::set_var("AWS_ACCESS_KEY_ID", value),
                    None => std::env::remove_var("AWS_ACCESS_KEY_ID"),
                }
                match &self.secret_access_key {
                    Some(value) => std::env::set_var("AWS_SECRET_ACCESS_KEY", value),
                    None => std::env::remove_var("AWS_SECRET_ACCESS_KEY"),
                }
                match &self.session_token {
                    Some(value) => std::env::set_var("AWS_SESSION_TOKEN", value),
                    None => std::env::remove_var("AWS_SESSION_TOKEN"),
                }
                match &self.profile {
                    Some(value) => std::env::set_var("AWS_PROFILE", value),
                    None => std::env::remove_var("AWS_PROFILE"),
                }
                match &self.ec2_metadata_disabled {
                    Some(value) => std::env::set_var("AWS_EC2_METADATA_DISABLED", value),
                    None => std::env::remove_var("AWS_EC2_METADATA_DISABLED"),
                }
            }
        }
    }

    fn provider(endpoint_url: &str) -> AwsKmsKeyProvider {
        AwsKmsKeyProvider::new(
            "alias/neovex-test",
            Some("us-east-1".to_string()),
            Some(endpoint_url.to_string()),
        )
        .expect("kms provider should build")
    }

    fn test_subject() -> LocalKeySubject {
        LocalKeySubject::sqlite_tenant(
            TenantId::new("demo").expect("tenant id should build"),
            "demo.sqlite3",
        )
    }

    fn test_header(provider: &AwsKmsKeyProvider, rotated_at: u64) -> KeyManifestHeader {
        KeyManifestHeader {
            version: MANIFEST_VERSION,
            cipher: ManifestCipher::SqlCipher,
            subject_descriptor: test_subject().descriptor(),
            key_provider: provider.kind(),
            created_at: 1000,
            rotated_at,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn generate_and_unwrap_round_trip_with_bound_encryption_context() {
        let _guard = AwsEnvGuard::install();
        let plaintext = vec![0xAB; 32];
        let ciphertext = b"kms-wrapped-generated".to_vec();

        let server = TestKmsServer::start(vec![
            ResponseSpec {
                target: "TrentService.GenerateDataKey",
                body: json!({
                    "Plaintext": BASE64.encode(&plaintext),
                    "CiphertextBlob": BASE64.encode(&ciphertext),
                    "KeyId": "arn:aws:kms:us-east-1:123456789012:key/generated",
                }),
                status: StatusCode::OK,
            },
            ResponseSpec {
                target: "TrentService.Decrypt",
                body: json!({
                    "Plaintext": BASE64.encode(&plaintext),
                    "KeyId": "arn:aws:kms:us-east-1:123456789012:key/generated",
                }),
                status: StatusCode::OK,
            },
        ])
        .await;

        let provider = provider(&server.endpoint_url);
        let subject = test_subject();
        let header = test_header(&provider, 1000);

        let generated = provider
            .generate_database_key(&subject, &header)
            .expect("generate should succeed");
        assert_eq!(generated.plaintext(), plaintext.as_slice());
        assert_eq!(generated.wrapped().cipher, WrappingCipher::AwsKms);

        let unwrapped = provider
            .unwrap_database_key(&subject, generated.wrapped(), &header)
            .expect("unwrap should succeed");
        assert_eq!(unwrapped, *generated.plaintext());

        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].target, "TrentService.GenerateDataKey");
        assert_eq!(requests[0].body["KeyId"], "alias/neovex-test");
        assert_eq!(requests[0].body["KeySpec"], "AES_256");
        assert_eq!(
            requests[0].body["EncryptionContext"][CONTEXT_SUBJECT_KEY],
            header.subject_descriptor
        );
        assert_eq!(
            requests[0].body["EncryptionContext"][CONTEXT_PROVIDER_KEY],
            header.key_provider.to_string()
        );
        assert_eq!(requests[1].target, "TrentService.Decrypt");
        assert_eq!(
            requests[1].body["EncryptionContext"][CONTEXT_ROTATED_AT_KEY],
            header.rotated_at.to_string()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn provider_native_rewrap_uses_reencrypt_with_old_and_new_context() {
        let _guard = AwsEnvGuard::install();
        let server = TestKmsServer::start(vec![ResponseSpec {
            target: "TrentService.ReEncrypt",
            body: json!({
                "CiphertextBlob": BASE64.encode(b"kms-rewrapped"),
                "KeyId": "arn:aws:kms:us-east-1:123456789012:key/rotated",
            }),
            status: StatusCode::OK,
        }])
        .await;

        let provider = provider(&server.endpoint_url);
        let subject = test_subject();
        let current_header = test_header(&provider, 1000);
        let new_header = test_header(&provider, 2000);
        let wrapped = WrappedDatabaseKey::new(WrappingCipher::AwsKms, b"old-ciphertext".to_vec());

        let rewrapped = provider
            .rewrap_wrapped_database_key(&subject, &wrapped, &current_header, &new_header)
            .expect("rewrap should succeed")
            .expect("kms should support provider-native rewrap");
        assert_eq!(rewrapped.cipher, WrappingCipher::AwsKms);
        assert_eq!(rewrapped.ciphertext, b"kms-rewrapped".to_vec());

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].target, "TrentService.ReEncrypt");
        assert_eq!(
            requests[0].body["SourceEncryptionContext"][CONTEXT_ROTATED_AT_KEY],
            current_header.rotated_at.to_string()
        );
        assert_eq!(
            requests[0].body["DestinationEncryptionContext"][CONTEXT_ROTATED_AT_KEY],
            new_header.rotated_at.to_string()
        );
        assert_eq!(requests[0].body["DestinationKeyId"], "alias/neovex-test");
    }
}

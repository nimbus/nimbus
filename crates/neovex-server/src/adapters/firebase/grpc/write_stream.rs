#![allow(clippy::result_large_err)]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures::stream;
use neovex_core::{
    AtomicWrite, AtomicWriteBatch, AtomicWriteBatchOutcome, Error, FieldTransform,
    FieldTransformOperation, NumericValue, PrincipalContext, SpecialDouble, StoredValue, TenantId,
    Timestamp, TypedScalarValue, WritePrecondition, WriteSetMode,
};
use prost_types::Timestamp as ProstTimestamp;
use serde_json::{Map as JsonMap, Value as JsonValue};
use tokio::sync::Mutex as AsyncMutex;
use tonic::{Request, Response, Status, Streaming};

use super::FirestoreGrpcService;
use super::generated::google::firestore::v1::document_transform::field_transform::{
    ServerValue as FirestoreServerValue, TransformType,
};
use super::generated::google::firestore::v1::precondition::ConditionType;
use super::generated::google::firestore::v1::value::ValueType;
use super::generated::google::firestore::v1::write::Operation;
use super::generated::google::firestore::v1::{
    self as proto, ArrayValue, Document, Precondition, Value, Write, WriteRequest, WriteResponse,
    WriteResult,
};
use super::generated::google::r#type::LatLng;
use crate::adapters::firebase::resource_names::{self, FirestoreDatabaseName};
use crate::adapters::firebase::serializer::{
    FirestoreDouble, FirestoreProtoJsonError, FirestoreValue, firestore_value_from_typed_scalar,
};
use crate::adapters::firebase::{
    firestore_grpc_code, resolve_write_key, resource_name_error_to_core, tenant_id_for_database,
};
use crate::application_auth::{
    extract_bearer_token_from_metadata, grpc_status_from_app_error,
    resolve_application_auth_from_bearer,
};
use crate::state::{AppState, record_authenticated_usage};

const WRITE_STREAM_TTL_MS: u64 = 60_000;
const MAX_ACTIVE_WRITE_STREAMS: usize = 256;
const MAX_REPLAYABLE_RESPONSES: usize = 32;
const WRITE_STREAM_ID_PREFIX: &str = "firestore_write_";

pub(super) struct WriteStreamRegistry {
    streams: Mutex<HashMap<String, Arc<AsyncMutex<StoredWriteStream>>>>,
    next_stream_id: AtomicU64,
}

impl WriteStreamRegistry {
    pub(super) fn new() -> Self {
        Self {
            streams: Mutex::new(HashMap::new()),
            next_stream_id: AtomicU64::new(1),
        }
    }

    fn create_stream(
        &self,
        database: FirestoreDatabaseName,
        tenant_id: TenantId,
    ) -> Result<(String, Arc<AsyncMutex<StoredWriteStream>>, WriteResponse), Status> {
        let mut streams = self
            .streams
            .lock()
            .expect("write stream lock should not be poisoned");
        prune_expired_streams(&mut streams);
        if streams.len() >= MAX_ACTIVE_WRITE_STREAMS {
            return Err(Status::resource_exhausted(format!(
                "too many active Firestore write streams; limit is {MAX_ACTIVE_WRITE_STREAMS}"
            )));
        }

        let stream_id = format!(
            "{WRITE_STREAM_ID_PREFIX}{}",
            self.next_stream_id.fetch_add(1, Ordering::Relaxed)
        );
        let mut stream = StoredWriteStream::new(database, tenant_id);
        let response = stream.new_stream_handshake(&stream_id);
        let stream = Arc::new(AsyncMutex::new(stream));
        streams.insert(stream_id.clone(), stream.clone());
        Ok((stream_id, stream, response))
    }

    fn get_stream(&self, stream_id: &str) -> Result<Arc<AsyncMutex<StoredWriteStream>>, Status> {
        let mut streams = self
            .streams
            .lock()
            .expect("write stream lock should not be poisoned");
        prune_expired_streams(&mut streams);
        streams.get(stream_id).cloned().ok_or_else(|| {
            Status::invalid_argument("write stream is not active; create a new stream")
        })
    }
}

#[derive(Clone)]
struct StoredReplayResponse {
    token: u64,
    response: WriteResponse,
}

struct StoredWriteStream {
    database: FirestoreDatabaseName,
    tenant_id: TenantId,
    latest_token: u64,
    acknowledged_token: u64,
    replayable_responses: VecDeque<StoredReplayResponse>,
    expires_at: Timestamp,
}

impl StoredWriteStream {
    fn new(database: FirestoreDatabaseName, tenant_id: TenantId) -> Self {
        Self {
            database,
            tenant_id,
            latest_token: 0,
            acknowledged_token: 0,
            replayable_responses: VecDeque::new(),
            expires_at: expiry_timestamp(),
        }
    }

    fn matches_database(&self, database: &FirestoreDatabaseName) -> bool {
        &self.database == database
    }

    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    fn latest_token(&self) -> u64 {
        self.latest_token
    }

    fn new_stream_handshake(&mut self, stream_id: &str) -> WriteResponse {
        self.touch();
        let token = encode_stream_token(self.next_token());
        self.push_response(WriteResponse {
            stream_id: stream_id.to_string(),
            stream_token: token,
            write_results: Vec::new(),
            commit_time: None,
        })
    }

    fn resume_from(&mut self, token: u64) -> Result<Vec<WriteResponse>, Status> {
        self.touch();
        self.acknowledge(token)?;

        let mut responses = self
            .replayable_responses
            .iter()
            .filter(|response| response.token > token)
            .map(|response| response.response.clone())
            .collect::<Vec<_>>();
        responses.push(self.current_token_response());
        Ok(responses)
    }

    fn acknowledge_only(&mut self, token: u64) -> Result<(), Status> {
        self.touch();
        self.acknowledge(token)
    }

    fn push_commit_outcome(
        &mut self,
        outcome: AtomicWriteBatchOutcome,
    ) -> Result<WriteResponse, Status> {
        self.touch();
        let write_results = outcome
            .write_results
            .iter()
            .map(proto_write_result)
            .collect::<Result<Vec<_>, _>>()?;
        let response = WriteResponse {
            stream_id: String::new(),
            stream_token: encode_stream_token(self.next_token()),
            write_results,
            commit_time: Some(prost_timestamp_from_core(outcome.commit_time)?),
        };
        Ok(self.push_response(response))
    }

    fn current_token_response(&self) -> WriteResponse {
        WriteResponse {
            stream_id: String::new(),
            stream_token: encode_stream_token(self.latest_token),
            write_results: Vec::new(),
            commit_time: None,
        }
    }

    fn push_response(&mut self, response: WriteResponse) -> WriteResponse {
        self.replayable_responses.push_back(StoredReplayResponse {
            token: self.latest_token,
            response: response.clone(),
        });
        while self.replayable_responses.len() > MAX_REPLAYABLE_RESPONSES {
            self.replayable_responses.pop_front();
        }
        response
    }

    fn acknowledge(&mut self, token: u64) -> Result<(), Status> {
        if token < self.acknowledged_token {
            return Err(Status::invalid_argument(
                "write stream token is older than the most recently acknowledged token",
            ));
        }
        if token > self.latest_token {
            return Err(Status::invalid_argument(
                "write stream token is newer than the current stream position",
            ));
        }
        self.acknowledged_token = token;
        while self
            .replayable_responses
            .front()
            .is_some_and(|response| response.token <= token)
        {
            self.replayable_responses.pop_front();
        }
        Ok(())
    }

    fn touch(&mut self) {
        self.expires_at = expiry_timestamp();
    }

    fn next_token(&mut self) -> u64 {
        self.latest_token = self.latest_token.saturating_add(1);
        self.latest_token
    }
}

fn prune_expired_streams(streams: &mut HashMap<String, Arc<AsyncMutex<StoredWriteStream>>>) {
    let now = Timestamp::now();
    streams.retain(|_, stream| match stream.try_lock() {
        Ok(stream) => stream.expires_at > now,
        Err(_) => true,
    });
}

fn expiry_timestamp() -> Timestamp {
    Timestamp(Timestamp::now().0.saturating_add(WRITE_STREAM_TTL_MS))
}

#[derive(Clone)]
struct ActiveWriteStream {
    database: FirestoreDatabaseName,
    stored: Arc<AsyncMutex<StoredWriteStream>>,
}

struct ActiveWriteRequestStream {
    state: Arc<AppState>,
    registry: Arc<WriteStreamRegistry>,
    requests: Streaming<WriteRequest>,
    principal: PrincipalContext,
    active_stream: Option<ActiveWriteStream>,
    pending_responses: VecDeque<WriteResponse>,
    received_first_request: bool,
}

impl ActiveWriteRequestStream {
    fn new(
        state: Arc<AppState>,
        registry: Arc<WriteStreamRegistry>,
        requests: Streaming<WriteRequest>,
        principal: PrincipalContext,
    ) -> Self {
        Self {
            state,
            registry,
            requests,
            principal,
            active_stream: None,
            pending_responses: VecDeque::new(),
            received_first_request: false,
        }
    }

    async fn next_response(&mut self) -> Option<Result<WriteResponse, Status>> {
        loop {
            if let Some(response) = self.pending_responses.pop_front() {
                return Some(Ok(response));
            }

            let request = match self.requests.message().await {
                Ok(Some(request)) => request,
                Ok(None) => return None,
                Err(status) => return Some(Err(status)),
            };

            match self.process_request(request).await {
                Ok(responses) => {
                    self.pending_responses.extend(responses);
                }
                Err(status) => return Some(Err(status)),
            }
        }
    }

    async fn process_request(
        &mut self,
        request: WriteRequest,
    ) -> Result<Vec<WriteResponse>, Status> {
        if !self.received_first_request {
            self.received_first_request = true;
            self.process_first_request(request).await
        } else {
            self.process_followup_request(request).await
        }
    }

    async fn process_first_request(
        &mut self,
        request: WriteRequest,
    ) -> Result<Vec<WriteResponse>, Status> {
        let database = parse_required_database(&request.database)?;
        if request.stream_id.is_empty() {
            if !request.writes.is_empty() {
                return Err(Status::invalid_argument(
                    "first write stream request must not include writes when creating a stream",
                ));
            }
            if !request.stream_token.is_empty() {
                return Err(Status::invalid_argument(
                    "first write stream request must not include a stream token when creating a stream",
                ));
            }

            let tenant_id = tenant_id_for_database(&database).map_err(firebase_grpc_status)?;
            let (_stream_id, stored, handshake) =
                self.registry.create_stream(database.clone(), tenant_id)?;
            self.active_stream = Some(ActiveWriteStream { database, stored });
            return Ok(vec![handshake]);
        }

        if request.stream_token.is_empty() {
            return Err(Status::invalid_argument(
                "resumed write stream requests must include a stream token",
            ));
        }
        if !request.writes.is_empty() {
            return Err(Status::invalid_argument(
                "first resumed write stream request must not include writes",
            ));
        }

        let token = decode_stream_token(&request.stream_token)?;
        let stored = self.registry.get_stream(&request.stream_id)?;
        let mut stream = stored.lock().await;
        if !stream.matches_database(&database) {
            return Err(Status::invalid_argument(
                "write stream database does not match the active stream",
            ));
        }
        let responses = stream.resume_from(token)?;
        drop(stream);

        self.active_stream = Some(ActiveWriteStream { database, stored });
        Ok(responses)
    }

    async fn process_followup_request(
        &mut self,
        request: WriteRequest,
    ) -> Result<Vec<WriteResponse>, Status> {
        if !request.stream_id.is_empty() {
            return Err(Status::invalid_argument(
                "stream_id may only be set on the first write stream request",
            ));
        }
        if request.stream_token.is_empty() {
            return Err(Status::invalid_argument(
                "post-handshake write stream requests must include a stream token",
            ));
        }

        let active_stream = self
            .active_stream
            .clone()
            .ok_or_else(|| Status::internal("write stream state was not initialized"))?;
        if !request.database.is_empty() {
            let database = parse_required_database(&request.database)?;
            if database != active_stream.database {
                return Err(Status::invalid_argument(
                    "write stream database does not match the active stream",
                ));
            }
        }

        let token = decode_stream_token(&request.stream_token)?;
        let mut stream = active_stream.stored.lock().await;
        if token != stream.latest_token() {
            return Err(Status::invalid_argument(
                "post-handshake write stream requests must acknowledge the latest stream token",
            ));
        }

        if request.writes.is_empty() {
            stream.acknowledge_only(token)?;
            return Ok(Vec::new());
        }

        let tenant_id = stream.tenant_id().clone();
        stream.acknowledge_only(token)?;
        let batch = lower_write_batch(&request.writes, &active_stream.database)?;
        let outcome = execute_write_batch(&self.state, &tenant_id, &self.principal, batch)?;
        let response = stream.push_commit_outcome(outcome)?;
        Ok(vec![response])
    }
}

pub(super) async fn handle_write(
    service: &FirestoreGrpcService,
    request: Request<Streaming<WriteRequest>>,
) -> Result<Response<tonic::codegen::BoxStream<WriteResponse>>, Status> {
    let state = service.app_state()?;
    let bearer = extract_bearer_token_from_metadata(request.metadata())
        .map_err(grpc_status_from_app_error)?;
    let auth = resolve_application_auth_from_bearer(&state, bearer.as_deref())
        .await
        .map_err(grpc_status_from_app_error)?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    let session = ActiveWriteRequestStream::new(
        state,
        service.write_streams.clone(),
        request.into_inner(),
        auth.principal,
    );
    let output: tonic::codegen::BoxStream<WriteResponse> =
        Box::pin(stream::unfold(session, |mut session| async move {
            session.next_response().await.map(|item| (item, session))
        }));
    Ok(Response::new(output))
}

fn execute_write_batch(
    state: &Arc<AppState>,
    tenant_id: &TenantId,
    principal: &PrincipalContext,
    batch: AtomicWriteBatch,
) -> Result<AtomicWriteBatchOutcome, Status> {
    state
        .service
        .begin_mutation_execution_unit(tenant_id.clone(), principal.clone())
        .and_then(|execution_unit| execution_unit.execute_atomic_write_batch(batch))
        .map_err(firebase_grpc_status)
}

fn parse_required_database(database: &str) -> Result<FirestoreDatabaseName, Status> {
    if database.is_empty() {
        return Err(Status::invalid_argument(
            "write stream requests must include a database on the first message",
        ));
    }
    resource_names::parse_database_name(database)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)
}

pub(super) fn lower_write_batch(
    writes: &[Write],
    database: &FirestoreDatabaseName,
) -> Result<AtomicWriteBatch, Status> {
    let writes = writes
        .iter()
        .cloned()
        .map(|write| lower_write(write, database))
        .collect::<Result<Vec<_>, _>>()?;
    AtomicWriteBatch::new(writes).map_err(firebase_grpc_status)
}

fn lower_write(write: Write, database: &FirestoreDatabaseName) -> Result<AtomicWrite, Status> {
    let Write {
        update_mask,
        update_transforms,
        current_document,
        operation,
    } = write;
    match operation {
        Some(Operation::Update(document)) => lower_update_write(
            document,
            update_mask,
            update_transforms,
            current_document,
            database,
        ),
        Some(Operation::Delete(document_name)) => {
            ensure_no_update_fields(update_mask.as_ref(), &update_transforms)?;
            let parsed_document = parse_document_name(&document_name)?;
            ensure_database_match(database, &parsed_document.database, "delete document")?;
            let key = resolve_write_key(&parsed_document.document_path)
                .map_err(|error| Status::invalid_argument(error.to_string()))?;
            let precondition = lower_precondition(current_document)?;
            Ok(AtomicWrite::Delete {
                key,
                precondition: precondition.clone(),
                missing_ok: precondition.is_empty(),
            })
        }
        Some(Operation::Verify(document_name)) => {
            ensure_no_update_fields(update_mask.as_ref(), &update_transforms)?;
            let parsed_document = parse_document_name(&document_name)?;
            ensure_database_match(database, &parsed_document.database, "verify document")?;
            let key = resolve_write_key(&parsed_document.document_path)
                .map_err(|error| Status::invalid_argument(error.to_string()))?;
            Ok(AtomicWrite::Verify {
                key,
                precondition: lower_precondition(current_document)?,
            })
        }
        Some(Operation::Transform(transform)) => {
            ensure_no_update_fields(update_mask.as_ref(), &update_transforms)?;
            let parsed_document = parse_document_name(&transform.document)?;
            ensure_database_match(database, &parsed_document.database, "transform document")?;
            let key = resolve_write_key(&parsed_document.document_path)
                .map_err(|error| Status::invalid_argument(error.to_string()))?;
            let transforms = lower_document_transforms(transform.field_transforms)?;
            if transforms.is_empty() {
                return Err(Status::invalid_argument(
                    "transform.field_transforms must contain at least one transform",
                ));
            }
            Ok(AtomicWrite::Transform {
                key,
                transforms,
                precondition: lower_precondition(current_document)?,
            })
        }
        None => Err(Status::invalid_argument(
            "each write must set exactly one operation",
        )),
    }
}

fn lower_update_write(
    document: Document,
    update_mask: Option<proto::DocumentMask>,
    update_transforms: Vec<proto::document_transform::FieldTransform>,
    current_document: Option<Precondition>,
    database: &FirestoreDatabaseName,
) -> Result<AtomicWrite, Status> {
    let parsed_document = parse_document_name(&document.name)?;
    ensure_database_match(database, &parsed_document.database, "update document")?;
    if document.create_time.is_some() || document.update_time.is_some() {
        return Err(Status::invalid_argument(
            "update documents must not set create_time or update_time",
        ));
    }

    let key = resolve_write_key(&parsed_document.document_path)
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    let document = lower_document_fields(document.fields)?;
    let precondition = lower_precondition(current_document)?;
    let transforms = lower_document_transforms(update_transforms)?;
    match update_mask {
        Some(mask) => Ok(AtomicWrite::Patch {
            key,
            field_patch: document,
            mask: mask.field_paths,
            precondition,
            transforms,
        }),
        None => Ok(AtomicWrite::Set {
            key,
            document,
            mode: WriteSetMode::Overwrite,
            precondition,
            transforms,
        }),
    }
}

fn ensure_no_update_fields(
    update_mask: Option<&proto::DocumentMask>,
    update_transforms: &[proto::document_transform::FieldTransform],
) -> Result<(), Status> {
    if update_mask.is_some() {
        return Err(Status::invalid_argument(
            "update_mask can only be set when an update document is present",
        ));
    }
    if !update_transforms.is_empty() {
        return Err(Status::invalid_argument(
            "update_transforms can only be set when an update document is present",
        ));
    }
    Ok(())
}

fn parse_document_name(
    document_name: &str,
) -> Result<resource_names::FirestoreDocumentName, Status> {
    resource_names::parse_document_name(document_name)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)
}

fn ensure_database_match(
    expected: &FirestoreDatabaseName,
    actual: &FirestoreDatabaseName,
    context: &str,
) -> Result<(), Status> {
    if expected == actual {
        Ok(())
    } else {
        Err(Status::invalid_argument(format!(
            "{context} database does not match the active write stream database"
        )))
    }
}

fn lower_document_fields(
    fields: HashMap<String, Value>,
) -> Result<JsonMap<String, JsonValue>, Status> {
    fields
        .into_iter()
        .map(|(field, value)| decode_neovex_value_from_grpc(&value).map(|value| (field, value)))
        .collect()
}

fn lower_precondition(precondition: Option<Precondition>) -> Result<WritePrecondition, Status> {
    let Some(precondition) = precondition else {
        return Ok(WritePrecondition::default());
    };
    let precondition = match precondition.condition_type {
        Some(ConditionType::Exists(exists)) => WritePrecondition::exists(exists),
        Some(ConditionType::UpdateTime(timestamp)) => {
            WritePrecondition::update_time(core_timestamp_from_prost(&timestamp)?)
        }
        None => {
            return Err(Status::invalid_argument(
                "current_document precondition must set exactly one condition",
            ));
        }
    };
    precondition.validate().map_err(firebase_grpc_status)?;
    Ok(precondition)
}

fn lower_document_transforms(
    transforms: Vec<proto::document_transform::FieldTransform>,
) -> Result<Vec<FieldTransform>, Status> {
    transforms
        .into_iter()
        .map(lower_document_transform)
        .collect()
}

fn lower_document_transform(
    transform: proto::document_transform::FieldTransform,
) -> Result<FieldTransform, Status> {
    if transform.field_path.is_empty() {
        return Err(Status::invalid_argument(
            "field transform field_path cannot be empty",
        ));
    }

    let operation = match transform.transform_type {
        Some(TransformType::SetToServerValue(server_value)) => {
            match FirestoreServerValue::try_from(server_value) {
                Ok(FirestoreServerValue::RequestTime) => FieldTransformOperation::ServerTimestamp,
                Ok(FirestoreServerValue::Unspecified) | Err(_) => {
                    return Err(Status::invalid_argument(
                        "unsupported field transform set_to_server_value",
                    ));
                }
            }
        }
        Some(TransformType::Increment(value)) => FieldTransformOperation::Increment {
            operand: decode_numeric_value_from_grpc(&value)?,
        },
        Some(TransformType::Maximum(value)) => FieldTransformOperation::Maximum {
            operand: decode_numeric_value_from_grpc(&value)?,
        },
        Some(TransformType::Minimum(value)) => FieldTransformOperation::Minimum {
            operand: decode_numeric_value_from_grpc(&value)?,
        },
        Some(TransformType::AppendMissingElements(values)) => {
            FieldTransformOperation::AppendMissingElements {
                values: lower_array_transform_values(values)?,
            }
        }
        Some(TransformType::RemoveAllFromArray(values)) => {
            FieldTransformOperation::RemoveAllFromArray {
                values: lower_array_transform_values(values)?,
            }
        }
        None => {
            return Err(Status::invalid_argument(
                "each field transform must set exactly one transform type",
            ));
        }
    };

    Ok(FieldTransform {
        field: transform.field_path,
        transform: operation,
    })
}

fn lower_array_transform_values(values: ArrayValue) -> Result<Vec<JsonValue>, Status> {
    values
        .values
        .iter()
        .map(decode_neovex_value_from_grpc)
        .collect()
}

pub(super) fn proto_write_result(
    result: &neovex_core::AtomicWriteResult,
) -> Result<WriteResult, Status> {
    let transform_results = result
        .transform_results
        .iter()
        .map(encode_stored_value_to_grpc)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WriteResult {
        update_time: result
            .update_time
            .map(prost_timestamp_from_core)
            .transpose()?,
        transform_results,
    })
}

pub(super) fn decode_neovex_value_from_grpc(value: &Value) -> Result<JsonValue, Status> {
    firestore_value_from_grpc(value)?
        .into_neovex_value()
        .map_err(firestore_value_status)
}

pub(super) fn decode_numeric_value_from_grpc(value: &Value) -> Result<NumericValue, Status> {
    match firestore_value_from_grpc(value)? {
        FirestoreValue::Integer(value) => Ok(NumericValue::Integer { value }),
        FirestoreValue::Double(value) => Ok(match value {
            FirestoreDouble::Number(value) => NumericValue::Double { value },
            value => NumericValue::SpecialDouble {
                value: special_double_from_firestore(value),
            },
        }),
        _ => Err(Status::invalid_argument(
            "numeric transforms require Firestore integerValue or doubleValue operands",
        )),
    }
}

pub(super) fn encode_neovex_value_to_grpc(value: &JsonValue) -> Result<Value, Status> {
    let firestore_value =
        FirestoreValue::try_from_neovex_value(value).map_err(firestore_value_status)?;
    firestore_value_to_grpc(firestore_value)
}

pub(super) fn encode_document_field_to_grpc(
    document: &neovex_core::Document,
    field_name: &str,
    value: &JsonValue,
) -> Result<Value, Status> {
    match document.typed_field(field_name) {
        Some(value) => encode_typed_scalar_to_grpc(value).map_err(|status| {
            Status::internal(format!(
                "failed to encode Firestore document field `{field_name}`: {}",
                status.message()
            ))
        }),
        None => encode_neovex_value_to_grpc(value),
    }
}

pub(super) fn encode_stored_value_to_grpc(value: &StoredValue) -> Result<Value, Status> {
    match value {
        StoredValue::Json { value } => encode_neovex_value_to_grpc(value),
        StoredValue::TypedScalar { value } => encode_typed_scalar_to_grpc(value),
    }
}

fn firestore_value_from_grpc(value: &Value) -> Result<FirestoreValue, Status> {
    match value.value_type.as_ref() {
        Some(ValueType::NullValue(_)) => Ok(FirestoreValue::Null),
        Some(ValueType::BooleanValue(value)) => Ok(FirestoreValue::Boolean(*value)),
        Some(ValueType::IntegerValue(value)) => Ok(FirestoreValue::Integer(*value)),
        Some(ValueType::DoubleValue(value)) => Ok(FirestoreValue::Double(firestore_double(*value))),
        Some(ValueType::TimestampValue(timestamp)) => Ok(FirestoreValue::Timestamp(
            format_prost_timestamp(timestamp)?,
        )),
        Some(ValueType::StringValue(value)) => Ok(FirestoreValue::String(value.clone())),
        Some(ValueType::BytesValue(value)) => Ok(FirestoreValue::Bytes(value.clone())),
        Some(ValueType::ReferenceValue(value)) => Ok(FirestoreValue::Reference(value.clone())),
        Some(ValueType::GeoPointValue(value)) => Ok(FirestoreValue::GeoPoint {
            latitude: value.latitude,
            longitude: value.longitude,
        }),
        Some(ValueType::ArrayValue(values)) => values
            .values
            .iter()
            .map(firestore_value_from_grpc)
            .collect::<Result<Vec<_>, _>>()
            .map(FirestoreValue::Array),
        Some(ValueType::MapValue(value)) => value
            .fields
            .iter()
            .map(|(key, value)| firestore_value_from_grpc(value).map(|value| (key.clone(), value)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(FirestoreValue::Map),
        Some(ValueType::FieldReferenceValue(_)) => Err(Status::invalid_argument(
            FirestoreProtoJsonError::UnsupportedType("fieldReferenceValue").to_string(),
        )),
        Some(ValueType::VariableReferenceValue(_)) => Err(Status::invalid_argument(
            FirestoreProtoJsonError::UnsupportedType("variableReferenceValue").to_string(),
        )),
        Some(ValueType::FunctionValue(_)) => Err(Status::invalid_argument(
            FirestoreProtoJsonError::UnsupportedType("functionValue").to_string(),
        )),
        Some(ValueType::PipelineValue(_)) => Err(Status::invalid_argument(
            FirestoreProtoJsonError::UnsupportedType("pipelineValue").to_string(),
        )),
        None => Err(Status::invalid_argument(
            "Firestore Value must set exactly one value type",
        )),
    }
}

fn firestore_value_to_grpc(value: FirestoreValue) -> Result<Value, Status> {
    let value_type = match value {
        FirestoreValue::Null => ValueType::NullValue(prost_types::NullValue::NullValue as i32),
        FirestoreValue::Boolean(value) => ValueType::BooleanValue(value),
        FirestoreValue::Integer(value) => ValueType::IntegerValue(value),
        FirestoreValue::Double(value) => ValueType::DoubleValue(match value {
            FirestoreDouble::Number(value) => value,
            FirestoreDouble::NegativeZero => -0.0,
            FirestoreDouble::NaN => f64::NAN,
            FirestoreDouble::PositiveInfinity => f64::INFINITY,
            FirestoreDouble::NegativeInfinity => f64::NEG_INFINITY,
        }),
        FirestoreValue::Timestamp(value) => {
            ValueType::TimestampValue(parse_rfc3339_timestamp(&value)?)
        }
        FirestoreValue::String(value) => ValueType::StringValue(value),
        FirestoreValue::Bytes(value) => ValueType::BytesValue(value),
        FirestoreValue::Reference(value) => ValueType::ReferenceValue(value),
        FirestoreValue::GeoPoint {
            latitude,
            longitude,
        } => ValueType::GeoPointValue(LatLng {
            latitude,
            longitude,
        }),
        FirestoreValue::Array(values) => ValueType::ArrayValue(ArrayValue {
            values: values
                .into_iter()
                .map(firestore_value_to_grpc)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        FirestoreValue::Map(fields) => ValueType::MapValue(proto::MapValue {
            fields: fields
                .into_iter()
                .map(|(key, value)| firestore_value_to_grpc(value).map(|value| (key, value)))
                .collect::<Result<HashMap<_, _>, _>>()?,
        }),
    };
    Ok(Value {
        value_type: Some(value_type),
    })
}

fn firestore_double(value: f64) -> FirestoreDouble {
    if value.is_nan() {
        FirestoreDouble::NaN
    } else if value.is_infinite() && value.is_sign_positive() {
        FirestoreDouble::PositiveInfinity
    } else if value.is_infinite() && value.is_sign_negative() {
        FirestoreDouble::NegativeInfinity
    } else if value == 0.0 && value.is_sign_negative() {
        FirestoreDouble::NegativeZero
    } else {
        FirestoreDouble::Number(value)
    }
}

fn special_double_from_firestore(value: FirestoreDouble) -> SpecialDouble {
    match value {
        FirestoreDouble::NegativeZero => SpecialDouble::NegativeZero,
        FirestoreDouble::NaN => SpecialDouble::Nan,
        FirestoreDouble::PositiveInfinity => SpecialDouble::PositiveInfinity,
        FirestoreDouble::NegativeInfinity => SpecialDouble::NegativeInfinity,
        FirestoreDouble::Number(_) => {
            unreachable!("finite doubles should not map to special doubles")
        }
    }
}

fn firestore_double_from_special_double(value: SpecialDouble) -> FirestoreDouble {
    match value {
        SpecialDouble::NegativeZero => FirestoreDouble::NegativeZero,
        SpecialDouble::Nan => FirestoreDouble::NaN,
        SpecialDouble::PositiveInfinity => FirestoreDouble::PositiveInfinity,
        SpecialDouble::NegativeInfinity => FirestoreDouble::NegativeInfinity,
    }
}

fn encode_typed_scalar_to_grpc(value: &TypedScalarValue) -> Result<Value, Status> {
    let firestore_value = firestore_value_from_typed_scalar(value)
        .map_err(|error| Status::internal(error.to_string()))?;
    firestore_value_to_grpc(firestore_value)
        .map_err(|status| Status::internal(status.message().to_string()))
}

pub(super) fn core_timestamp_from_prost(timestamp: &ProstTimestamp) -> Result<Timestamp, Status> {
    let seconds = u64::try_from(timestamp.seconds).map_err(|_| {
        Status::invalid_argument("Firestore timestamps must be after the Unix epoch")
    })?;
    let nanos = u32::try_from(timestamp.nanos)
        .map_err(|_| Status::invalid_argument("Firestore timestamp nanos must be non-negative"))?;
    Ok(Timestamp(
        seconds
            .saturating_mul(1_000)
            .saturating_add(u64::from(nanos / 1_000_000)),
    ))
}

pub(super) fn prost_timestamp_from_core(timestamp: Timestamp) -> Result<ProstTimestamp, Status> {
    let seconds = timestamp.0 / 1_000;
    let millis = timestamp.0 % 1_000;
    Ok(ProstTimestamp {
        seconds: i64::try_from(seconds)
            .map_err(|_| Status::internal("timestamp exceeds prost timestamp range"))?,
        nanos: i32::try_from(millis.saturating_mul(1_000_000))
            .map_err(|_| Status::internal("timestamp nanos exceed prost range"))?,
    })
}

fn format_prost_timestamp(timestamp: &ProstTimestamp) -> Result<String, Status> {
    let timestamp = time::OffsetDateTime::from_unix_timestamp(timestamp.seconds)
        .map_err(|_| Status::invalid_argument("Firestore timestamp seconds are out of range"))?
        .replace_nanosecond(u32::try_from(timestamp.nanos).map_err(|_| {
            Status::invalid_argument("Firestore timestamp nanos must be non-negative")
        })?)
        .map_err(|_| Status::invalid_argument("Firestore timestamp nanos are out of range"))?;
    timestamp
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| Status::internal("failed to format Firestore timestamp"))
}

fn parse_rfc3339_timestamp(timestamp: &str) -> Result<ProstTimestamp, Status> {
    let timestamp =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
            .map_err(|_| Status::invalid_argument("invalid Firestore RFC3339 timestamp"))?;
    Ok(ProstTimestamp {
        seconds: timestamp.unix_timestamp(),
        nanos: i32::try_from(timestamp.nanosecond())
            .map_err(|_| Status::internal("timestamp nanoseconds exceed prost range"))?,
    })
}

fn decode_stream_token(token: &[u8]) -> Result<u64, Status> {
    let bytes: [u8; 8] = token
        .try_into()
        .map_err(|_| Status::invalid_argument("write stream tokens must be 8 bytes"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn encode_stream_token(token: u64) -> Vec<u8> {
    token.to_be_bytes().to_vec()
}

fn firestore_value_status(error: FirestoreProtoJsonError) -> Status {
    Status::invalid_argument(error.to_string())
}

pub(super) fn firebase_grpc_status(error: Error) -> Status {
    Status::new(firestore_grpc_code(&error), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neovex_core::{Document, TableName, TypedScalarValue};

    #[test]
    fn encode_document_field_to_grpc_rejects_foreign_typed_scalars_as_internal() {
        let mut document = Document::new(
            TableName::new("cities").expect("table name should parse"),
            JsonMap::new(),
        );
        document.set_typed_field(
            "mongoId",
            TypedScalarValue::ObjectId {
                hex: "507f1f77bcf86cd799439011".to_string(),
            },
        );

        let error = encode_document_field_to_grpc(
            &document,
            "mongoId",
            document
                .get_field("mongoId")
                .expect("typed scalar projection should populate the JSON field"),
        )
        .expect_err("foreign typed scalars should not silently project on Firebase reads");

        assert_eq!(error.code(), tonic::Code::Internal);
        assert!(
            error.message().contains("mongoId"),
            "error should name the rejected field: {error}"
        );
        assert!(
            error.message().contains("typedScalar:ObjectId"),
            "error should surface the rejected typed scalar kind: {error}"
        );
    }
}

#![allow(clippy::result_large_err)]

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{Stream, StreamExt, stream};
use nimbus_core::{
    CollectionPath, Document, DocumentPath, Filter, FilterOp, OrderBy, OrderDirection,
    PrincipalContext, Query, SequenceNumber, SubscriptionResultSnapshot, Timestamp,
    diff_subscription_snapshots,
};
use nimbus_engine::{
    DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY, SubscriptionCleanupHandle, SubscriptionUpdate,
};
use tokio::sync::mpsc;
use tonic::{Request, Response, Status, Streaming};

use super::FirestoreGrpcService;
use super::generated::google::firestore::v1::listen_request::TargetChange as ListenRequestChange;
use super::generated::google::firestore::v1::listen_response::ResponseType;
use super::generated::google::firestore::v1::structured_query::field_filter::Operator as FieldFilterOperatorProto;
use super::generated::google::firestore::v1::structured_query::filter::FilterType;
use super::generated::google::firestore::v1::target::ResumeType;
use super::generated::google::firestore::v1::target::TargetType;
use super::generated::google::firestore::v1::target::query_target::QueryType;
use super::generated::google::firestore::v1::target_change::TargetChangeType;
use super::generated::google::firestore::v1::{
    self as proto, Document as FirestoreDocument, DocumentChange, DocumentDelete, DocumentRemove,
    ExistenceFilter, ListenRequest, ListenResponse, Target, TargetChange,
};
use super::write_stream::{
    core_timestamp_from_prost, decode_nimbus_value_from_grpc, encode_document_field_to_grpc,
    firebase_grpc_status, prost_timestamp_from_core,
};
use crate::adapters::firebase::resource_names::{self, FirestoreDatabaseName};
use crate::adapters::firebase::{
    firestore_document_name, resource_name_error_to_core, storage_table_for_collection_path,
    tenant_id_for_database,
};
use crate::application_auth::{
    extract_bearer_token_from_metadata, grpc_status_from_app_error,
    resolve_application_auth_from_bearer,
};
use crate::state::{AppState, record_authenticated_usage};

const RETAINED_LISTEN_TARGET_TTL: Duration = Duration::from_secs(60);
const MAX_RETAINED_LISTEN_TARGETS: usize = 256;
const LISTEN_TARGET_UPDATE_QUEUE_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RetainedListenTargetKey {
    project_id: String,
    collection_path: CollectionPath,
    query: Query,
}

#[derive(Debug, Clone)]
struct RetainedListenTargetState {
    snapshot: SubscriptionResultSnapshot,
    read_time: Timestamp,
    expires_at: Timestamp,
}

pub(super) struct RetainedListenRegistry {
    targets: Mutex<HashMap<RetainedListenTargetKey, RetainedListenTargetState>>,
}

impl RetainedListenRegistry {
    pub(super) fn new() -> Self {
        Self {
            targets: Mutex::new(HashMap::new()),
        }
    }

    fn retain(
        &self,
        key: RetainedListenTargetKey,
        snapshot: &SubscriptionResultSnapshot,
        read_time: Timestamp,
    ) {
        let mut targets = self
            .targets
            .lock()
            .expect("listen target registry lock should not be poisoned");
        prune_expired_retained_targets(&mut targets);
        if !targets.contains_key(&key) && targets.len() >= MAX_RETAINED_LISTEN_TARGETS {
            evict_oldest_retained_target(&mut targets);
        }
        targets.insert(
            key,
            RetainedListenTargetState {
                snapshot: snapshot.clone(),
                read_time,
                expires_at: retained_target_expiry(),
            },
        );
    }

    fn clear(&self, key: &RetainedListenTargetKey) {
        self.targets
            .lock()
            .expect("listen target registry lock should not be poisoned")
            .remove(key);
    }

    fn resolve_resume(
        &self,
        key: &RetainedListenTargetKey,
        selector: &ResumeSelector,
    ) -> ResumeDecision {
        let mut targets = self
            .targets
            .lock()
            .expect("listen target registry lock should not be poisoned");
        prune_expired_retained_targets(&mut targets);
        let Some(retained) = targets.get(key).cloned() else {
            return match selector {
                ResumeSelector::None => ResumeDecision::ColdStart,
                ResumeSelector::Token(_) | ResumeSelector::ReadTime(_) => ResumeDecision::Reset,
            };
        };

        match selector {
            ResumeSelector::None => ResumeDecision::ColdStart,
            ResumeSelector::Token(sequence) if retained.snapshot.covered_sequence == *sequence => {
                ResumeDecision::Resume(retained)
            }
            ResumeSelector::ReadTime(read_time) if retained.read_time == *read_time => {
                ResumeDecision::Resume(retained)
            }
            ResumeSelector::Token(_) | ResumeSelector::ReadTime(_) => ResumeDecision::Reset,
        }
    }
}

fn prune_expired_retained_targets(
    targets: &mut HashMap<RetainedListenTargetKey, RetainedListenTargetState>,
) {
    let now = Timestamp::now();
    targets.retain(|_, retained| retained.expires_at > now);
}

fn evict_oldest_retained_target(
    targets: &mut HashMap<RetainedListenTargetKey, RetainedListenTargetState>,
) {
    let Some(oldest_key) = targets
        .iter()
        .min_by_key(|(_, retained)| retained.expires_at)
        .map(|(key, _)| key.clone())
    else {
        return;
    };
    targets.remove(&oldest_key);
}

fn retained_target_expiry() -> Timestamp {
    Timestamp(
        Timestamp::now().0.saturating_add(
            u64::try_from(RETAINED_LISTEN_TARGET_TTL.as_millis()).unwrap_or(u64::MAX),
        ),
    )
}

#[derive(Debug, Clone)]
enum ResumeSelector {
    None,
    Token(SequenceNumber),
    ReadTime(Timestamp),
}

#[derive(Debug, Clone)]
enum ResumeDecision {
    ColdStart,
    Resume(RetainedListenTargetState),
    Reset,
}

#[derive(Default)]
struct ListenTargetRegistry {
    next_assigned_target_id: i32,
    active_targets: HashMap<i32, ActiveListenTarget>,
}

impl ListenTargetRegistry {
    fn has_active_targets(&self) -> bool {
        !self.active_targets.is_empty()
    }

    fn active_target_mut(&mut self, target_id: i32) -> Option<&mut ActiveListenTarget> {
        self.active_targets.get_mut(&target_id)
    }

    fn assign_target_id(&mut self, requested_target_id: i32) -> Result<i32, Status> {
        if requested_target_id < 0 {
            return Err(Status::invalid_argument(
                "Listen target IDs must be positive or zero for server assignment",
            ));
        }
        if requested_target_id > 0 {
            if self.active_targets.contains_key(&requested_target_id) {
                return Err(Status::invalid_argument(
                    "Listen add_target target_id already exists on this stream",
                ));
            }
            return Ok(requested_target_id);
        }

        loop {
            self.next_assigned_target_id = self.next_assigned_target_id.saturating_add(1).max(1);
            if !self
                .active_targets
                .contains_key(&self.next_assigned_target_id)
            {
                return Ok(self.next_assigned_target_id);
            }
        }
    }

    fn insert(&mut self, target: ActiveListenTarget) {
        self.active_targets.insert(target.target_id, target);
    }

    fn remove(&mut self, target_id: i32) -> Result<ActiveListenTarget, Status> {
        let Some(target) = self.active_targets.remove(&target_id) else {
            return Err(Status::invalid_argument(
                "remove_target does not match an active Listen target",
            ));
        };
        Ok(target)
    }

    fn remove_if_present(&mut self, target_id: i32) -> Option<ActiveListenTarget> {
        self.active_targets.remove(&target_id)
    }
}

struct ActiveListenTarget {
    database: FirestoreDatabaseName,
    collection_path: CollectionPath,
    retained_key: RetainedListenTargetKey,
    retained_targets: Arc<RetainedListenRegistry>,
    target_id: i32,
    once: bool,
    cleanup: SubscriptionCleanupHandle,
    last_snapshot: Option<SubscriptionResultSnapshot>,
}

impl ActiveListenTarget {
    fn bootstrap(
        &mut self,
        update: SubscriptionUpdate,
        resume: ResumeDecision,
        existence_filter_count: Option<i32>,
    ) -> Result<Vec<ListenResponse>, Status> {
        match update {
            SubscriptionUpdate::Result { snapshot, .. } => {
                self.translate_bootstrap(snapshot, resume, existence_filter_count)
            }
            SubscriptionUpdate::Error { message, .. } => Err(Status::aborted(message)),
        }
    }

    fn update(&mut self, update: SubscriptionUpdate) -> Result<Vec<ListenResponse>, Status> {
        match update {
            SubscriptionUpdate::Result { snapshot, .. } => self.translate_snapshot(snapshot),
            SubscriptionUpdate::Error { message, .. } => Err(Status::aborted(message)),
        }
    }

    fn translate_bootstrap(
        &mut self,
        snapshot: SubscriptionResultSnapshot,
        resume: ResumeDecision,
        existence_filter_count: Option<i32>,
    ) -> Result<Vec<ListenResponse>, Status> {
        let read_time = observed_read_time_core(&snapshot);
        let read_time_proto = prost_timestamp_from_core(read_time)?;
        let mut responses = vec![target_change_response(
            TargetChangeType::Add,
            vec![self.target_id],
            Vec::new(),
            None,
        )];
        if let Some(count) = existence_filter_count {
            // Firestore allows `unchanged_names` to be omitted, so phase 1 uses
            // a count-only existence filter before falling back to `RESET`.
            responses.push(existence_filter_response(self.target_id, count));
        }

        match resume {
            ResumeDecision::ColdStart => append_diff_responses(
                &mut responses,
                None,
                &snapshot,
                DiffResponseContext {
                    database: &self.database,
                    collection_path: &self.collection_path,
                    target_id: self.target_id,
                    read_time: &read_time_proto,
                    allow_delete_classification: false,
                },
            )?,
            ResumeDecision::Resume(retained) => append_diff_responses(
                &mut responses,
                Some(&retained.snapshot),
                &snapshot,
                DiffResponseContext {
                    database: &self.database,
                    collection_path: &self.collection_path,
                    target_id: self.target_id,
                    read_time: &read_time_proto,
                    allow_delete_classification: false,
                },
            )?,
            ResumeDecision::Reset => {
                responses.push(target_change_response(
                    TargetChangeType::Reset,
                    vec![self.target_id],
                    Vec::new(),
                    Some(read_time_proto),
                ));
                append_diff_responses(
                    &mut responses,
                    None,
                    &snapshot,
                    DiffResponseContext {
                        database: &self.database,
                        collection_path: &self.collection_path,
                        target_id: self.target_id,
                        read_time: &read_time_proto,
                        allow_delete_classification: false,
                    },
                )?;
            }
        }

        responses.push(target_change_response(
            TargetChangeType::Current,
            vec![self.target_id],
            encode_resume_token(snapshot.covered_sequence),
            Some(read_time_proto),
        ));
        self.retained_targets
            .retain(self.retained_key.clone(), &snapshot, read_time);
        self.last_snapshot = Some(snapshot);
        Ok(responses)
    }

    fn translate_snapshot(
        &mut self,
        snapshot: SubscriptionResultSnapshot,
    ) -> Result<Vec<ListenResponse>, Status> {
        let read_time = observed_read_time_core(&snapshot);
        let read_time_proto = prost_timestamp_from_core(read_time)?;
        let mut responses = Vec::new();
        append_diff_responses(
            &mut responses,
            self.last_snapshot.as_ref(),
            &snapshot,
            DiffResponseContext {
                database: &self.database,
                collection_path: &self.collection_path,
                target_id: self.target_id,
                read_time: &read_time_proto,
                allow_delete_classification: true,
            },
        )?;
        responses.push(target_change_response(
            TargetChangeType::NoChange,
            vec![self.target_id],
            encode_resume_token(snapshot.covered_sequence),
            Some(read_time_proto),
        ));
        self.retained_targets
            .retain(self.retained_key.clone(), &snapshot, read_time);
        self.last_snapshot = Some(snapshot);
        Ok(responses)
    }
}

struct ActiveListenRequestStream {
    state: Arc<AppState>,
    retained_targets: Arc<RetainedListenRegistry>,
    requests: Pin<Box<dyn Stream<Item = Result<ListenRequest, Status>> + Send>>,
    principal: PrincipalContext,
    targets: ListenTargetRegistry,
    target_updates_tx: mpsc::Sender<TargetedListenUpdate>,
    target_updates_rx: mpsc::Receiver<TargetedListenUpdate>,
    target_overflow_tx: mpsc::UnboundedSender<i32>,
    target_overflow_rx: mpsc::UnboundedReceiver<i32>,
    pending_responses: VecDeque<ListenResponse>,
}

enum ListenEvent {
    Request(Box<Result<Option<ListenRequest>, Status>>),
    Update(Option<TargetedListenUpdate>),
    Overflow(Option<i32>),
}

enum TargetedListenUpdate {
    Result {
        target_id: i32,
        update: SubscriptionUpdate,
    },
    Closed {
        target_id: i32,
    },
}

impl ActiveListenRequestStream {
    fn new(
        state: Arc<AppState>,
        retained_targets: Arc<RetainedListenRegistry>,
        requests: Pin<Box<dyn Stream<Item = Result<ListenRequest, Status>> + Send>>,
        principal: PrincipalContext,
    ) -> Self {
        let (target_updates_tx, target_updates_rx) =
            mpsc::channel(LISTEN_TARGET_UPDATE_QUEUE_CAPACITY);
        let (target_overflow_tx, target_overflow_rx) = mpsc::unbounded_channel();
        Self {
            state,
            retained_targets,
            requests,
            principal,
            targets: ListenTargetRegistry::default(),
            target_updates_tx,
            target_updates_rx,
            target_overflow_tx,
            target_overflow_rx,
            pending_responses: VecDeque::new(),
        }
    }

    async fn next_response(&mut self) -> Option<Result<ListenResponse, Status>> {
        loop {
            if let Some(response) = self.pending_responses.pop_front() {
                return Some(Ok(response));
            }

            let event = if self.targets.has_active_targets() {
                let requests = &mut self.requests;
                let target_updates = &mut self.target_updates_rx;
                let target_overflow = &mut self.target_overflow_rx;
                tokio::select! {
                    request = requests.next() => ListenEvent::Request(Box::new(request.transpose())),
                    update = target_updates.recv() => ListenEvent::Update(update),
                    overflow = target_overflow.recv() => ListenEvent::Overflow(overflow),
                }
            } else {
                ListenEvent::Request(Box::new(self.requests.next().await.transpose()))
            };

            match event {
                ListenEvent::Request(request) => match *request {
                    Ok(Some(request)) => match self.process_request(request).await {
                        Ok(responses) => self.pending_responses.extend(responses),
                        Err(status) => return Some(Err(status)),
                    },
                    Ok(None) => return None,
                    Err(status) => return Some(Err(status)),
                },
                ListenEvent::Update(Some(update)) => match self.process_target_update(update) {
                    Ok(responses) => self.pending_responses.extend(responses),
                    Err(status) => return Some(Err(status)),
                },
                ListenEvent::Update(None) => {
                    return Some(Err(Status::internal(
                        "Listen target update multiplexer closed unexpectedly",
                    )));
                }
                ListenEvent::Overflow(Some(target_id)) => {
                    return Some(Err(self.process_target_overflow(target_id)));
                }
                ListenEvent::Overflow(None) => {
                    return Some(Err(Status::internal(
                        "Listen target overflow notifier closed unexpectedly",
                    )));
                }
            }
        }
    }

    async fn process_request(
        &mut self,
        request: ListenRequest,
    ) -> Result<Vec<ListenResponse>, Status> {
        let database = parse_required_database(&request.database)?;
        match request.target_change {
            Some(ListenRequestChange::AddTarget(target)) => {
                self.process_add_target(database, target).await
            }
            Some(ListenRequestChange::RemoveTarget(target_id)) => {
                self.process_remove_target(&database, target_id)
            }
            None => Err(Status::invalid_argument(
                "Listen requests must set exactly one of add_target or remove_target",
            )),
        }
    }

    async fn process_add_target(
        &mut self,
        database: FirestoreDatabaseName,
        target: Target,
    ) -> Result<Vec<ListenResponse>, Status> {
        let target_id = self.targets.assign_target_id(target.target_id)?;
        let prepared_target = lower_listen_target(&database, target)?;
        let resume = self.retained_targets.resolve_resume(
            &prepared_target.retained_key,
            &prepared_target.resume_selector,
        );
        let (resume, existence_filter_count) =
            apply_expected_count_resume_policy(resume, prepared_target.expected_count)?;
        let tenant_id = tenant_id_for_database(&database).map_err(firebase_grpc_status)?;
        let (sender, receiver) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let registration = self
            .state
            .service
            .clone()
            .subscribe_async_with_principal(
                tenant_id,
                prepared_target.query,
                self.principal.clone(),
                format!("firestore-listen-{target_id}"),
                sender,
            )
            .await
            .map_err(firebase_grpc_status)?;
        let (_subscription_id, cleanup) = registration.into_parts();
        let mut updates = receiver;
        let mut active_target = ActiveListenTarget {
            database,
            collection_path: prepared_target.collection_path,
            retained_key: prepared_target.retained_key,
            retained_targets: self.retained_targets.clone(),
            target_id,
            once: prepared_target.once,
            cleanup,
            last_snapshot: None,
        };
        let bootstrap = updates.recv().await.ok_or_else(|| {
            Status::internal("Listen subscription closed before bootstrap completed")
        })?;
        let mut responses = active_target.bootstrap(bootstrap, resume, existence_filter_count)?;
        if active_target.once {
            active_target
                .retained_targets
                .clear(&active_target.retained_key);
            drop(active_target);
            responses.push(target_change_response(
                TargetChangeType::Remove,
                vec![target_id],
                Vec::new(),
                None,
            ));
            return Ok(responses);
        }
        spawn_target_update_forwarder(
            target_id,
            updates,
            self.target_updates_tx.clone(),
            self.target_overflow_tx.clone(),
        );
        self.targets.insert(active_target);
        Ok(responses)
    }

    fn process_remove_target(
        &mut self,
        database: &FirestoreDatabaseName,
        target_id: i32,
    ) -> Result<Vec<ListenResponse>, Status> {
        if target_id <= 0 {
            return Err(Status::invalid_argument(
                "remove_target must reference a positive target ID",
            ));
        }
        let target = self.targets.remove(target_id)?;
        if &target.database != database {
            self.targets.insert(target);
            return Err(Status::invalid_argument(
                "Listen remove_target database does not match the active target database",
            ));
        }
        target.retained_targets.clear(&target.retained_key);
        drop(target);
        Ok(vec![target_change_response(
            TargetChangeType::Remove,
            vec![target_id],
            Vec::new(),
            None,
        )])
    }

    fn process_target_update(
        &mut self,
        event: TargetedListenUpdate,
    ) -> Result<Vec<ListenResponse>, Status> {
        match event {
            TargetedListenUpdate::Result { target_id, update } => {
                match self.targets.active_target_mut(target_id) {
                    Some(target) => target.update(update),
                    None => Ok(Vec::new()),
                }
            }
            TargetedListenUpdate::Closed { target_id } => {
                if self.targets.remove_if_present(target_id).is_some() {
                    return Err(Status::internal(
                        "active Listen target update channel closed unexpectedly",
                    ));
                }
                Ok(Vec::new())
            }
        }
    }

    fn process_target_overflow(&mut self, target_id: i32) -> Status {
        self.clear_retained_targets();
        let _ = self.targets.remove_if_present(target_id);
        Status::resource_exhausted("Listen stream backpressure limit exceeded")
    }

    fn clear_retained_targets(&mut self) {
        for target in self.targets.active_targets.values() {
            target.retained_targets.clear(&target.retained_key);
        }
    }
}

fn spawn_target_update_forwarder(
    target_id: i32,
    mut updates: mpsc::Receiver<SubscriptionUpdate>,
    sender: mpsc::Sender<TargetedListenUpdate>,
    overflow_sender: mpsc::UnboundedSender<i32>,
) {
    tokio::spawn(async move {
        while let Some(update) = updates.recv().await {
            match sender.try_send(TargetedListenUpdate::Result { target_id, update }) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    let _ = overflow_sender.send(target_id);
                    return;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return,
            }
        }
        match sender.try_send(TargetedListenUpdate::Closed { target_id }) {
            Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                let _ = overflow_sender.send(target_id);
            }
        }
    });
}

struct PreparedListenTarget {
    collection_path: CollectionPath,
    query: Query,
    retained_key: RetainedListenTargetKey,
    resume_selector: ResumeSelector,
    expected_count: Option<usize>,
    once: bool,
}

struct DiffResponseContext<'a> {
    database: &'a FirestoreDatabaseName,
    collection_path: &'a CollectionPath,
    target_id: i32,
    read_time: &'a prost_types::Timestamp,
    allow_delete_classification: bool,
}

pub(super) fn listen_response_stream<S>(
    service: &FirestoreGrpcService,
    requests: S,
    principal: PrincipalContext,
) -> Result<tonic::codegen::BoxStream<ListenResponse>, Status>
where
    S: Stream<Item = Result<ListenRequest, Status>> + Send + 'static,
{
    let session = ActiveListenRequestStream::new(
        service.app_state()?,
        service.listen_targets.clone(),
        Box::pin(requests),
        principal,
    );
    Ok(Box::pin(stream::unfold(
        session,
        |mut session| async move { session.next_response().await.map(|item| (item, session)) },
    )))
}

pub(super) async fn handle_listen(
    service: &FirestoreGrpcService,
    request: Request<Streaming<ListenRequest>>,
) -> Result<Response<tonic::codegen::BoxStream<ListenResponse>>, Status> {
    let state = service.app_state()?;
    let bearer = extract_bearer_token_from_metadata(request.metadata())
        .map_err(grpc_status_from_app_error)?;
    let auth = resolve_application_auth_from_bearer(&state, bearer.as_deref())
        .await
        .map_err(grpc_status_from_app_error)?;
    record_authenticated_usage(&state, auth.auth.as_ref()).await;
    Ok(Response::new(listen_response_stream(
        service,
        request.into_inner(),
        auth.principal,
    )?))
}

fn parse_required_database(database: &str) -> Result<FirestoreDatabaseName, Status> {
    if database.is_empty() {
        return Err(Status::invalid_argument(
            "Listen requests must include a database",
        ));
    }
    resource_names::parse_database_name(database)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)
}

fn lower_listen_target(
    database: &FirestoreDatabaseName,
    target: Target,
) -> Result<PreparedListenTarget, Status> {
    let Target {
        target_type,
        resume_type,
        expected_count,
        once,
        ..
    } = target;
    let resume_selector = lower_resume_selector(resume_type)?;
    let expected_count = normalize_expected_count(expected_count)?;
    let target_type = target_type
        .ok_or_else(|| Status::invalid_argument("Listen add_target must set a target type"))?;
    match target_type {
        TargetType::Documents(_) => Err(Status::unimplemented(
            "Listen document-name targets are deferred to F2.4b",
        )),
        TargetType::Query(query_target) => lower_query_target(
            database,
            query_target,
            resume_selector,
            expected_count,
            once,
        ),
    }
}

fn lower_resume_selector(resume_type: Option<ResumeType>) -> Result<ResumeSelector, Status> {
    match resume_type {
        None => Ok(ResumeSelector::None),
        Some(ResumeType::ResumeToken(token)) => {
            Ok(ResumeSelector::Token(decode_resume_token(&token)?))
        }
        Some(ResumeType::ReadTime(read_time)) => Ok(ResumeSelector::ReadTime(
            core_timestamp_from_prost(&read_time)?,
        )),
    }
}

fn normalize_expected_count(expected_count: Option<i32>) -> Result<Option<usize>, Status> {
    expected_count
        .map(|count| {
            usize::try_from(count)
                .map_err(|_| Status::invalid_argument("Listen expected_count must be non-negative"))
        })
        .transpose()
}

fn apply_expected_count_resume_policy(
    resume: ResumeDecision,
    expected_count: Option<usize>,
) -> Result<(ResumeDecision, Option<i32>), Status> {
    match (resume, expected_count) {
        (ResumeDecision::Resume(retained), Some(expected_count))
            if retained.snapshot.documents.len() != expected_count =>
        {
            let existence_filter_count =
                i32::try_from(retained.snapshot.documents.len()).map_err(|_| {
                    Status::internal("Listen existence filter count exceeded int32 range")
                })?;
            Ok((ResumeDecision::Reset, Some(existence_filter_count)))
        }
        (resume, _) => Ok((resume, None)),
    }
}

fn lower_query_target(
    database: &FirestoreDatabaseName,
    query_target: proto::target::QueryTarget,
    resume_selector: ResumeSelector,
    expected_count: Option<usize>,
    once: bool,
) -> Result<PreparedListenTarget, Status> {
    let proto::target::QueryTarget { parent, query_type } = query_target;
    let parent_name = resource_names::parse_parent_name(&parent)
        .map_err(resource_name_error_to_core)
        .map_err(firebase_grpc_status)?;
    if parent_name.database != *database {
        return Err(Status::invalid_argument(
            "Listen target parent database does not match the request database",
        ));
    }

    let structured_query = match query_type {
        Some(QueryType::StructuredQuery(structured_query)) => structured_query,
        None => {
            return Err(Status::invalid_argument(
                "Listen query targets must include a structured query",
            ));
        }
    };
    let collection_target = match structured_query.from.as_slice() {
        [] => {
            return Err(Status::invalid_argument(
                "Listen structured queries must include exactly one collection selector",
            ));
        }
        [selector] if selector.all_descendants => {
            return Err(Status::unimplemented(
                "Listen collection-group targets are deferred to F2.4b",
            ));
        }
        [selector] => {
            resource_names::parse_collection_target(&parent, selector.collection_id.as_str())
                .map_err(resource_name_error_to_core)
                .map_err(firebase_grpc_status)?
        }
        _ => {
            return Err(Status::unimplemented(
                "Listen multiple-source structured queries are deferred to F2.4b",
            ));
        }
    };

    let table = storage_table_for_collection_path(&collection_target.collection_path)
        .map_err(firebase_grpc_status)?;
    let query = lower_structured_query_to_subscription(table, structured_query)?;
    Ok(PreparedListenTarget {
        retained_key: RetainedListenTargetKey {
            project_id: database.project_id.clone(),
            collection_path: collection_target.collection_path.clone(),
            query: query.clone(),
        },
        collection_path: collection_target.collection_path,
        query,
        resume_selector,
        expected_count,
        once,
    })
}

fn lower_structured_query_to_subscription(
    table: nimbus_core::TableName,
    query: proto::StructuredQuery,
) -> Result<Query, Status> {
    if query.select.is_some() {
        return Err(Status::unimplemented(
            "Listen projections are deferred to F2.4b",
        ));
    }
    if query.start_at.is_some() || query.end_at.is_some() {
        return Err(Status::unimplemented(
            "Listen cursor bounds are deferred to F2.4b",
        ));
    }
    if query.offset != 0 {
        return Err(Status::unimplemented(
            "Listen offsets are deferred to F2.4b",
        ));
    }
    if query.find_nearest.is_some() {
        return Err(Status::unimplemented(
            "Listen find_nearest is deferred to F3",
        ));
    }

    let filters = match query.r#where {
        Some(filter) => vec![lower_structured_filter(filter)?],
        None => Vec::new(),
    };
    let order = match query.order_by.as_slice() {
        [] => None,
        [order] => Some(lower_structured_order(order)?),
        _ => {
            return Err(Status::unimplemented(
                "Listen repeated order_by clauses are deferred to F2.4b",
            ));
        }
    };
    let limit = query
        .limit
        .map(|limit| {
            usize::try_from(limit).map_err(|_| {
                Status::invalid_argument("Listen structured query limit must be non-negative")
            })
        })
        .transpose()?;

    Ok(Query {
        table,
        filters,
        order,
        limit,
    })
}

fn lower_structured_filter(filter: proto::structured_query::Filter) -> Result<Filter, Status> {
    match filter.filter_type {
        Some(FilterType::FieldFilter(filter)) => {
            let field = filter
                .field
                .ok_or_else(|| Status::invalid_argument("field filters must set `field`"))?;
            let field = top_level_field_path(
                field.field_path.as_str(),
                "nested field paths in Listen filters",
            )?;
            if field == "__name__" {
                return Err(Status::unimplemented(
                    "Listen document ID filters are deferred to F2.4b",
                ));
            }
            let op = match FieldFilterOperatorProto::try_from(filter.op) {
                Ok(FieldFilterOperatorProto::LessThan) => FilterOp::Lt,
                Ok(FieldFilterOperatorProto::LessThanOrEqual) => FilterOp::Lte,
                Ok(FieldFilterOperatorProto::GreaterThan) => FilterOp::Gt,
                Ok(FieldFilterOperatorProto::GreaterThanOrEqual) => FilterOp::Gte,
                Ok(FieldFilterOperatorProto::Equal) => FilterOp::Eq,
                Ok(FieldFilterOperatorProto::NotEqual) => FilterOp::Neq,
                Ok(FieldFilterOperatorProto::ArrayContains)
                | Ok(FieldFilterOperatorProto::In)
                | Ok(FieldFilterOperatorProto::ArrayContainsAny)
                | Ok(FieldFilterOperatorProto::NotIn) => {
                    return Err(Status::unimplemented(
                        "Listen array and set-membership filters are deferred to F2.4b",
                    ));
                }
                Ok(FieldFilterOperatorProto::Unspecified) | Err(_) => {
                    return Err(Status::invalid_argument(
                        "field filters must set a supported operator",
                    ));
                }
            };
            let value = filter
                .value
                .as_ref()
                .ok_or_else(|| Status::invalid_argument("field filters must set `value`"))
                .and_then(decode_nimbus_value_from_grpc)?;
            Ok(Filter {
                field: field.to_string(),
                op,
                value,
            })
        }
        Some(FilterType::CompositeFilter(_)) => Err(Status::unimplemented(
            "Listen composite filters are deferred to F2.4b",
        )),
        Some(FilterType::UnaryFilter(_)) => Err(Status::unimplemented(
            "Listen unary filters are deferred to F2.4b",
        )),
        None => Err(Status::invalid_argument(
            "structured filters must set exactly one filter type",
        )),
    }
}

fn lower_structured_order(order: &proto::structured_query::Order) -> Result<OrderBy, Status> {
    let field = order
        .field
        .as_ref()
        .ok_or_else(|| Status::invalid_argument("order_by clauses must set `field`"))?;
    let field = top_level_field_path(
        field.field_path.as_str(),
        "nested field paths in Listen order_by",
    )?;
    if field == "__name__" {
        return Err(Status::unimplemented(
            "Listen document ID ordering is deferred to F2.4b",
        ));
    }
    let direction = match proto::structured_query::Direction::try_from(order.direction) {
        Ok(proto::structured_query::Direction::Descending) => OrderDirection::Desc,
        Ok(proto::structured_query::Direction::Ascending)
        | Ok(proto::structured_query::Direction::Unspecified)
        | Err(_) => OrderDirection::Asc,
    };
    Ok(OrderBy {
        field: field.to_string(),
        direction,
    })
}

fn top_level_field_path<'a>(field_path: &'a str, context: &str) -> Result<&'a str, Status> {
    if field_path == "__name__" {
        return Ok(field_path);
    }
    if field_path.contains('.') {
        return Err(Status::unimplemented(context));
    }
    Ok(field_path)
}

fn append_diff_responses(
    responses: &mut Vec<ListenResponse>,
    previous: Option<&SubscriptionResultSnapshot>,
    current: &SubscriptionResultSnapshot,
    context: DiffResponseContext<'_>,
) -> Result<(), Status> {
    for change in diff_subscription_snapshots(previous, current).changes {
        match (change.current.as_ref(), change.previous.as_ref()) {
            (Some(document), _) => responses.push(document_change_response(
                context.database,
                context.collection_path,
                context.target_id,
                document,
            )?),
            (None, Some(previous)) => {
                let deleted = context.allow_delete_classification
                    && current
                        .deleted_documents
                        .iter()
                        .any(|deleted| same_document_identity(deleted, previous));
                responses.push(if deleted {
                    document_delete_response(
                        context.database,
                        context.collection_path,
                        context.target_id,
                        previous,
                        *context.read_time,
                    )
                } else {
                    document_remove_response(
                        context.database,
                        context.collection_path,
                        context.target_id,
                        previous,
                        *context.read_time,
                    )
                });
            }
            (None, None) => {}
        }
    }
    Ok(())
}

fn target_change_response(
    change_type: TargetChangeType,
    target_ids: Vec<i32>,
    resume_token: Vec<u8>,
    read_time: Option<prost_types::Timestamp>,
) -> ListenResponse {
    ListenResponse {
        response_type: Some(ResponseType::TargetChange(TargetChange {
            target_change_type: change_type as i32,
            target_ids,
            cause: None,
            resume_token,
            read_time,
        })),
    }
}

fn existence_filter_response(target_id: i32, count: i32) -> ListenResponse {
    ListenResponse {
        response_type: Some(ResponseType::Filter(ExistenceFilter {
            target_id,
            count,
            unchanged_names: None,
        })),
    }
}

fn document_change_response(
    database: &FirestoreDatabaseName,
    collection_path: &CollectionPath,
    target_id: i32,
    document: &Document,
) -> Result<ListenResponse, Status> {
    Ok(ListenResponse {
        response_type: Some(ResponseType::DocumentChange(DocumentChange {
            document: Some(firestore_document(database, collection_path, document)?),
            target_ids: vec![target_id],
            removed_target_ids: Vec::new(),
        })),
    })
}

fn document_delete_response(
    database: &FirestoreDatabaseName,
    collection_path: &CollectionPath,
    target_id: i32,
    document: &Document,
    read_time: prost_types::Timestamp,
) -> ListenResponse {
    ListenResponse {
        response_type: Some(ResponseType::DocumentDelete(DocumentDelete {
            document: firestore_document_name(
                database,
                &DocumentPath::new(collection_path.clone(), document.id.clone()),
            ),
            removed_target_ids: vec![target_id],
            read_time: Some(read_time),
        })),
    }
}

fn document_remove_response(
    database: &FirestoreDatabaseName,
    collection_path: &CollectionPath,
    target_id: i32,
    document: &Document,
    read_time: prost_types::Timestamp,
) -> ListenResponse {
    ListenResponse {
        response_type: Some(ResponseType::DocumentRemove(DocumentRemove {
            document: firestore_document_name(
                database,
                &DocumentPath::new(collection_path.clone(), document.id.clone()),
            ),
            removed_target_ids: vec![target_id],
            read_time: Some(read_time),
        })),
    }
}

fn firestore_document(
    database: &FirestoreDatabaseName,
    collection_path: &CollectionPath,
    document: &Document,
) -> Result<FirestoreDocument, Status> {
    let fields = document
        .fields
        .iter()
        .map(|(field, value)| {
            encode_document_field_to_grpc(document, field, value)
                .map(|value| (field.clone(), value))
        })
        .collect::<Result<_, _>>()?;
    let document_path = DocumentPath::new(collection_path.clone(), document.id.clone());
    let create_time = prost_timestamp_from_core(document.creation_time)?;
    let update_time = prost_timestamp_from_core(document.update_time)?;
    Ok(FirestoreDocument {
        name: firestore_document_name(database, &document_path),
        fields,
        create_time: Some(create_time),
        update_time: Some(update_time),
    })
}

fn observed_read_time_core(snapshot: &SubscriptionResultSnapshot) -> Timestamp {
    snapshot
        .commit
        .map(|commit| commit.timestamp)
        .unwrap_or_else(Timestamp::now)
}

fn same_document_identity(left: &Document, right: &Document) -> bool {
    left.table == right.table && left.id == right.id
}

fn decode_resume_token(token: &[u8]) -> Result<SequenceNumber, Status> {
    let bytes: [u8; 8] = token
        .try_into()
        .map_err(|_| Status::invalid_argument("Listen resume tokens must be 8 bytes"))?;
    Ok(SequenceNumber(u64::from_be_bytes(bytes)))
}

fn encode_resume_token(sequence: SequenceNumber) -> Vec<u8> {
    sequence.0.to_be_bytes().to_vec()
}

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "CFA7/CFA8 prove the Durable Object substrate before the Worker front door constructs it in production"
    )
)]

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, Result, TenantId, Timestamp};
use nimbus_engine::Engine;
use nimbus_services::{
    DurableObjectActivationLease, DurableObjectId, DurableObjectInstanceKey, DurableObjectNamespace,
};
use nimbus_storage::{KvBatchOp, KvBatchOutcome, KvPut};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;

const STORAGE_PREFIX: &str = "cloudflare-do";
const LEASE_KEY: &str = "__system/lease";
const ALARM_KEY: &str = "__system/alarm";
const WS_PREFIX: &str = "__system/ws/";

#[derive(Clone)]
pub struct DurableObjectSubstrate {
    engine: Arc<Engine>,
    clock: Arc<dyn DurableObjectClock>,
    lanes: Arc<Mutex<BTreeMap<DurableObjectInstanceKey, Arc<AsyncMutex<()>>>>>,
}

#[derive(Clone)]
pub struct DurableObjectStub {
    substrate: DurableObjectSubstrate,
    key: DurableObjectInstanceKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableObjectStorageOp {
    Put { key: String, value: Vec<u8> },
    Delete { key: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableObjectTransactionOutcome {
    pub batch: KvBatchOutcome,
    pub confirmed_outputs: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStorageCursor {
    pub column_names: Vec<String>,
    pub rows_read: usize,
    pub rows_written: usize,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableObjectAlarm {
    pub scheduled_time_millis: i64,
    pub retry_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HibernatedWebSocket {
    pub socket_id: String,
    pub tags: Vec<String>,
    pub attachment: Option<Value>,
    pub auto_response: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LeaseRecord {
    holder_id: String,
    lease_epoch: u64,
    expires_at_millis: u64,
}

trait DurableObjectClock: Send + Sync {
    fn now_millis(&self) -> u64;
}

#[derive(Debug, Default)]
struct SystemDurableObjectClock;

impl DurableObjectClock for SystemDurableObjectClock {
    fn now_millis(&self) -> u64 {
        Timestamp::now().0
    }
}

impl DurableObjectSubstrate {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self::with_clock(engine, Arc::new(SystemDurableObjectClock))
    }

    fn with_clock(engine: Arc<Engine>, clock: Arc<dyn DurableObjectClock>) -> Self {
        Self {
            engine,
            clock,
            lanes: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn stub_by_name(
        &self,
        tenant_id: TenantId,
        namespace: DurableObjectNamespace,
        name: &str,
    ) -> DurableObjectStub {
        self.stub_for_id(
            tenant_id,
            namespace.clone(),
            DurableObjectId::from_name(&namespace, name),
        )
    }

    pub fn stub_from_string(
        &self,
        tenant_id: TenantId,
        namespace: DurableObjectNamespace,
        id: &str,
    ) -> Result<DurableObjectStub> {
        let id = DurableObjectId::from_hex_string(id)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        Ok(self.stub_for_id(tenant_id, namespace, id))
    }

    pub fn stub_for_id(
        &self,
        tenant_id: TenantId,
        namespace: DurableObjectNamespace,
        id: DurableObjectId,
    ) -> DurableObjectStub {
        DurableObjectStub {
            substrate: self.clone(),
            key: DurableObjectInstanceKey::new(tenant_id, namespace, id),
        }
    }

    pub async fn block_concurrency_while<Fut, T>(
        &self,
        key: &DurableObjectInstanceKey,
        task: Fut,
    ) -> Result<T>
    where
        Fut: Future<Output = Result<T>>,
    {
        let lane = self.lane_for(key);
        let _guard = lane.lock().await;
        task.await
    }

    fn lane_for(&self, key: &DurableObjectInstanceKey) -> Arc<AsyncMutex<()>> {
        let mut lanes = self
            .lanes
            .lock()
            .expect("durable object lane registry mutex should not be poisoned");
        lanes
            .entry(key.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn now_millis(&self) -> u64 {
        self.clock.now_millis()
    }
}

impl DurableObjectStub {
    pub fn key(&self) -> &DurableObjectInstanceKey {
        &self.key
    }

    pub async fn claim_activation(
        &self,
        holder_id: impl Into<String>,
        ttl_millis: u64,
    ) -> Result<DurableObjectActivationLease> {
        let holder_id = holder_id.into();
        if holder_id.is_empty() {
            return Err(Error::InvalidInput(
                "Durable Object activation holder id must not be empty".to_string(),
            ));
        }
        if ttl_millis == 0 {
            return Err(Error::InvalidInput(
                "Durable Object activation TTL must be greater than zero".to_string(),
            ));
        }

        let lane = self.substrate.lane_for(&self.key);
        let _guard = lane.lock().await;
        let next_epoch = self.current_lease_epoch()? + 1;
        let now_millis = self.substrate.now_millis();
        let lease = DurableObjectActivationLease {
            instance_key: self.key.clone(),
            holder_id: holder_id.clone(),
            lease_epoch: next_epoch,
            resource_version: format!("do-lease:{next_epoch}"),
            acquired_at_millis: now_millis,
            expires_at_millis: now_millis.saturating_add(ttl_millis),
        };
        let record = LeaseRecord {
            holder_id,
            lease_epoch: next_epoch,
            expires_at_millis: lease.expires_at_millis,
        };
        self.put_system_json(LEASE_KEY, &record)?;
        Ok(lease)
    }

    pub async fn storage_get(
        &self,
        lease: &DurableObjectActivationLease,
        key: &str,
        now_millis: i64,
    ) -> Result<Option<Vec<u8>>> {
        self.ensure_lease_belongs_to_stub(lease)?;
        let lane = self.substrate.lane_for(&self.key);
        let _guard = lane.lock().await;
        self.ensure_current_lease(lease)?;
        let Some(entry) = self.substrate.engine.tenant_kv_get(
            &self.key.tenant_id,
            &storage_key(&self.key, key),
            now_millis,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(entry.value))
    }

    pub async fn transaction(
        &self,
        lease: &DurableObjectActivationLease,
        ops: Vec<DurableObjectStorageOp>,
    ) -> Result<KvBatchOutcome> {
        let outcome = self
            .transaction_with_output_gate(lease, ops, Vec::new())
            .await?;
        Ok(outcome.batch)
    }

    pub async fn transaction_with_output_gate(
        &self,
        lease: &DurableObjectActivationLease,
        ops: Vec<DurableObjectStorageOp>,
        queued_outputs: Vec<Value>,
    ) -> Result<DurableObjectTransactionOutcome> {
        self.ensure_lease_belongs_to_stub(lease)?;
        let lane = self.substrate.lane_for(&self.key);
        let _guard = lane.lock().await;
        self.ensure_current_lease(lease)?;
        let batch_ops = ops
            .into_iter()
            .map(|op| self.batch_op(op))
            .collect::<Vec<_>>();
        let batch = self
            .substrate
            .engine
            .tenant_kv_apply_batch(&self.key.tenant_id, &batch_ops)?;
        Ok(DurableObjectTransactionOutcome {
            batch,
            confirmed_outputs: queued_outputs,
        })
    }

    pub async fn sql_exec(
        &self,
        lease: &DurableObjectActivationLease,
        statement: &str,
    ) -> Result<SqlStorageCursor> {
        self.ensure_lease_belongs_to_stub(lease)?;
        let lane = self.substrate.lane_for(&self.key);
        let _guard = lane.lock().await;
        self.ensure_current_lease(lease)?;
        if statement.trim().eq_ignore_ascii_case("select 1") {
            return Ok(SqlStorageCursor {
                column_names: vec!["1".to_string()],
                rows_read: 1,
                rows_written: 0,
                rows: vec![vec![Value::from(1)]],
            });
        }
        Err(Error::InvalidInput(format!(
            "Durable Object sql.exec supports only the CFA7 proof query, got `{statement}`"
        )))
    }

    pub async fn set_alarm(
        &self,
        lease: &DurableObjectActivationLease,
        alarm: DurableObjectAlarm,
    ) -> Result<()> {
        self.ensure_lease_belongs_to_stub(lease)?;
        let lane = self.substrate.lane_for(&self.key);
        let _guard = lane.lock().await;
        self.ensure_current_lease(lease)?;
        self.put_system_json(ALARM_KEY, &alarm)
    }

    pub async fn get_alarm(
        &self,
        lease: &DurableObjectActivationLease,
        now_millis: i64,
    ) -> Result<Option<DurableObjectAlarm>> {
        self.ensure_lease_belongs_to_stub(lease)?;
        let lane = self.substrate.lane_for(&self.key);
        let _guard = lane.lock().await;
        self.ensure_current_lease(lease)?;
        self.get_system_json(ALARM_KEY, now_millis)
    }

    pub async fn delete_alarm(&self, lease: &DurableObjectActivationLease) -> Result<bool> {
        self.ensure_lease_belongs_to_stub(lease)?;
        let lane = self.substrate.lane_for(&self.key);
        let _guard = lane.lock().await;
        self.ensure_current_lease(lease)?;
        self.substrate
            .engine
            .tenant_kv_delete(&self.key.tenant_id, &storage_key(&self.key, ALARM_KEY))
    }

    pub async fn accept_web_socket(
        &self,
        lease: &DurableObjectActivationLease,
        socket_id: impl Into<String>,
        tags: Vec<String>,
    ) -> Result<()> {
        let socket = HibernatedWebSocket {
            socket_id: socket_id.into(),
            tags,
            attachment: None,
            auto_response: None,
        };
        self.store_socket(lease, socket).await
    }

    pub async fn serialize_attachment(
        &self,
        lease: &DurableObjectActivationLease,
        socket_id: &str,
        attachment: Value,
    ) -> Result<()> {
        let mut socket = self
            .load_socket(lease, socket_id, 0)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Durable Object socket `{socket_id}`")))?;
        socket.attachment = Some(attachment);
        self.store_socket(lease, socket).await
    }

    pub async fn deserialize_attachment(
        &self,
        lease: &DurableObjectActivationLease,
        socket_id: &str,
        now_millis: i64,
    ) -> Result<Option<Value>> {
        Ok(self
            .load_socket(lease, socket_id, now_millis)
            .await?
            .and_then(|socket| socket.attachment))
    }

    pub async fn set_web_socket_auto_response(
        &self,
        lease: &DurableObjectActivationLease,
        socket_id: &str,
        auto_response: String,
    ) -> Result<()> {
        if auto_response.len() > 2048 {
            return Err(Error::InvalidInput(
                "Durable Object WebSocket auto response must be at most 2048 bytes".to_string(),
            ));
        }
        let mut socket = self
            .load_socket(lease, socket_id, 0)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Durable Object socket `{socket_id}`")))?;
        socket.auto_response = Some(auto_response);
        self.store_socket(lease, socket).await
    }

    pub async fn get_web_sockets(
        &self,
        lease: &DurableObjectActivationLease,
        tag: Option<&str>,
        now_millis: i64,
    ) -> Result<Vec<HibernatedWebSocket>> {
        self.ensure_lease_belongs_to_stub(lease)?;
        let lane = self.substrate.lane_for(&self.key);
        let _guard = lane.lock().await;
        self.ensure_current_lease(lease)?;
        let page = self.substrate.engine.tenant_kv_scan(
            &self.key.tenant_id,
            &storage_key(&self.key, WS_PREFIX),
            None,
            1024,
            now_millis,
        )?;
        page.entries
            .into_iter()
            .map(|entry| decode_json::<HibernatedWebSocket>(&entry.value))
            .filter(|socket| {
                socket.as_ref().map_or(true, |socket| {
                    tag.is_none_or(|tag| socket.tags.iter().any(|item| item == tag))
                })
            })
            .collect()
    }

    fn ensure_lease_belongs_to_stub(&self, lease: &DurableObjectActivationLease) -> Result<()> {
        if lease.instance_key == self.key {
            return Ok(());
        }
        Err(Error::PermissionDenied(
            "Durable Object lease belongs to a different tenant, namespace, or object id"
                .to_string(),
        ))
    }

    fn ensure_current_lease(&self, lease: &DurableObjectActivationLease) -> Result<()> {
        let current = self.read_lease_record()?;
        if current.lease_epoch == lease.lease_epoch
            && current.holder_id == lease.holder_id
            && current.expires_at_millis == lease.expires_at_millis
        {
            let now_millis = self.substrate.now_millis();
            if lease.expires_at_millis <= now_millis {
                return Err(Error::PreconditionFailed(format!(
                    "expired Durable Object activation lease epoch {} expired at {}",
                    lease.lease_epoch, lease.expires_at_millis
                )));
            }
            return Ok(());
        }
        Err(Error::PreconditionFailed(format!(
            "stale Durable Object activation epoch {}; current epoch is {}",
            lease.lease_epoch, current.lease_epoch
        )))
    }

    fn current_lease_epoch(&self) -> Result<u64> {
        match self.read_lease_record() {
            Ok(record) => Ok(record.lease_epoch),
            Err(Error::NotFound(_)) => Ok(0),
            Err(error) => Err(error),
        }
    }

    fn read_lease_record(&self) -> Result<LeaseRecord> {
        self.get_system_json(LEASE_KEY, 0)?
            .ok_or_else(|| Error::NotFound("Durable Object activation lease".to_string()))
    }

    fn batch_op(&self, op: DurableObjectStorageOp) -> KvBatchOp {
        match op {
            DurableObjectStorageOp::Put { key, value } => {
                KvBatchOp::Put(KvPut::new(storage_key(&self.key, &key), value))
            }
            DurableObjectStorageOp::Delete { key } => {
                KvBatchOp::Delete(storage_key(&self.key, &key))
            }
        }
    }

    fn put_system_json<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let value = serde_json::to_vec(value).map_err(|error| {
            Error::Internal(format!("failed to encode DO system state: {error}"))
        })?;
        self.substrate.engine.tenant_kv_put(
            &self.key.tenant_id,
            KvPut::new(storage_key(&self.key, key), value),
        )
    }

    fn get_system_json<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
        now_millis: i64,
    ) -> Result<Option<T>> {
        let Some(entry) = self.substrate.engine.tenant_kv_get(
            &self.key.tenant_id,
            &storage_key(&self.key, key),
            now_millis,
        )?
        else {
            return Ok(None);
        };
        decode_json(&entry.value).map(Some)
    }

    async fn store_socket(
        &self,
        lease: &DurableObjectActivationLease,
        socket: HibernatedWebSocket,
    ) -> Result<()> {
        if socket.tags.len() > 10 {
            return Err(Error::InvalidInput(
                "Durable Object WebSocket accepts at most 10 tags".to_string(),
            ));
        }
        if socket.tags.iter().any(|tag| tag.len() > 256) {
            return Err(Error::InvalidInput(
                "Durable Object WebSocket tag must be at most 256 bytes".to_string(),
            ));
        }
        if serde_json::to_vec(&socket.attachment)
            .map_err(|error| Error::Internal(format!("failed to encode attachment: {error}")))?
            .len()
            > 16 * 1024
        {
            return Err(Error::InvalidInput(
                "Durable Object WebSocket attachment must be at most 16 KiB".to_string(),
            ));
        }
        let key = format!("{WS_PREFIX}{}", socket.socket_id);
        self.transaction(
            lease,
            vec![DurableObjectStorageOp::Put {
                key,
                value: serde_json::to_vec(&socket).map_err(|error| {
                    Error::Internal(format!("failed to encode hibernated WebSocket: {error}"))
                })?,
            }],
        )
        .await?;
        Ok(())
    }

    async fn load_socket(
        &self,
        lease: &DurableObjectActivationLease,
        socket_id: &str,
        now_millis: i64,
    ) -> Result<Option<HibernatedWebSocket>> {
        self.ensure_lease_belongs_to_stub(lease)?;
        let key = format!("{WS_PREFIX}{socket_id}");
        let Some(value) = self.storage_get(lease, &key, now_millis).await? else {
            return Ok(None);
        };
        decode_json(&value).map(Some)
    }
}

fn decode_json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes)
        .map_err(|error| Error::Internal(format!("failed to decode DO system state: {error}")))
}

fn storage_key(key: &DurableObjectInstanceKey, item: &str) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(STORAGE_PREFIX.as_bytes());
    output.push(0);
    output.extend_from_slice(key.tenant_id.as_str().as_bytes());
    output.push(0);
    output.extend_from_slice(key.namespace.as_str().as_bytes());
    output.push(0);
    output.extend_from_slice(key.id.as_hex().as_bytes());
    output.push(0);
    output.extend_from_slice(item.as_bytes());
    output
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use nimbus_engine::{EmbeddedProviderKind, Engine};
    use nimbus_testing::EngineFixture;
    use serde_json::json;
    use tokio::sync::oneshot;

    use super::*;

    #[derive(Debug)]
    struct ManualDurableObjectClock {
        now_millis: AtomicU64,
    }

    impl ManualDurableObjectClock {
        fn new(now_millis: u64) -> Self {
            Self {
                now_millis: AtomicU64::new(now_millis),
            }
        }

        fn set(&self, now_millis: u64) {
            self.now_millis.store(now_millis, Ordering::SeqCst);
        }
    }

    impl DurableObjectClock for ManualDurableObjectClock {
        fn now_millis(&self) -> u64 {
            self.now_millis.load(Ordering::SeqCst)
        }
    }

    fn fixture() -> (EngineFixture<Engine>, DurableObjectSubstrate, TenantId) {
        fixture_with_clock(Arc::new(ManualDurableObjectClock::new(1_000)))
    }

    fn fixture_with_clock(
        clock: Arc<dyn DurableObjectClock>,
    ) -> (EngineFixture<Engine>, DurableObjectSubstrate, TenantId) {
        let fixture = EngineFixture::new(|path| {
            Engine::new_with_embedded_provider(path, EmbeddedProviderKind::Redb)
        });
        let tenant = TenantId::new("tenant-a").expect("tenant id should build");
        fixture
            .engine()
            .create_tenant(tenant.clone())
            .expect("tenant should create");
        let substrate = DurableObjectSubstrate::with_clock(fixture.engine(), clock);
        (fixture, substrate, tenant)
    }

    fn namespace() -> DurableObjectNamespace {
        DurableObjectNamespace::new("COUNTER").expect("namespace should build")
    }

    #[tokio::test]
    async fn durable_object_substrate_default_clock_admits_fresh_system_time_lease() {
        let fixture = EngineFixture::new(|path| {
            Engine::new_with_embedded_provider(path, EmbeddedProviderKind::Redb)
        });
        let tenant = TenantId::new("tenant-a").expect("tenant id should build");
        fixture
            .engine()
            .create_tenant(tenant.clone())
            .expect("tenant should create");
        let substrate = DurableObjectSubstrate::new(fixture.engine());
        let stub = substrate.stub_by_name(tenant, namespace(), "default-clock");
        let lease = stub
            .claim_activation("owner", 60_000)
            .await
            .expect("system-time activation should claim");

        let outcome = stub
            .transaction(
                &lease,
                vec![DurableObjectStorageOp::Put {
                    key: "state".to_string(),
                    value: b"fresh".to_vec(),
                }],
            )
            .await
            .expect("fresh default-clock lease should write");

        assert_eq!(outcome.puts, 1);
    }

    #[tokio::test]
    async fn durable_object_storage_transaction_round_trips_per_instance() {
        let (_fixture, substrate, tenant) = fixture();
        let stub = substrate.stub_by_name(tenant, namespace(), "counter-a");
        let lease = stub
            .claim_activation("owner-a", 30_000)
            .await
            .expect("activation should claim");

        let outcome = stub
            .transaction(
                &lease,
                vec![DurableObjectStorageOp::Put {
                    key: "state/count".to_string(),
                    value: b"1".to_vec(),
                }],
            )
            .await
            .expect("DO storage transaction should commit");

        assert_eq!(outcome.puts, 1);
        assert_eq!(
            stub.storage_get(&lease, "state/count", 1_000)
                .await
                .expect("storage should read"),
            Some(b"1".to_vec())
        );
        assert_eq!(
            stub.sql_exec(&lease, "select 1")
                .await
                .expect("sql.exec proof query should run")
                .rows,
            vec![vec![Value::from(1)]]
        );
        let delete_outcome = stub
            .transaction(
                &lease,
                vec![
                    DurableObjectStorageOp::Put {
                        key: "state/transient".to_string(),
                        value: b"delete-me".to_vec(),
                    },
                    DurableObjectStorageOp::Delete {
                        key: "state/transient".to_string(),
                    },
                ],
            )
            .await
            .expect("DO delete transaction should commit");
        assert_eq!(delete_outcome.puts, 1);
        assert_eq!(delete_outcome.deletes, 1);
        assert_eq!(
            stub.storage_get(&lease, "state/transient", 1_000)
                .await
                .expect("deleted storage should read"),
            None
        );
    }

    #[tokio::test]
    async fn durable_object_cross_tenant_id_from_string_cannot_reach_other_tenant_storage() {
        let (fixture, substrate, tenant_a) = fixture();
        let tenant_b = TenantId::new("tenant-b").expect("tenant id should build");
        fixture
            .engine()
            .create_tenant(tenant_b.clone())
            .expect("tenant should create");
        let object_id = DurableObjectId::from_name(&namespace(), "shared-name");
        let tenant_a_stub = substrate.stub_for_id(tenant_a, namespace(), object_id.clone());
        let tenant_a_lease = tenant_a_stub
            .claim_activation("tenant_a_owner", 30_000)
            .await
            .expect("tenant A activation should claim");
        tenant_a_stub
            .transaction(
                &tenant_a_lease,
                vec![DurableObjectStorageOp::Put {
                    key: "secret".to_string(),
                    value: b"tenant-a-only".to_vec(),
                }],
            )
            .await
            .expect("tenant A write should commit");

        let tenant_b_stub = substrate
            .stub_from_string(tenant_b, namespace(), object_id.as_hex())
            .expect("idFromString 64-hex should parse for tenant B binding");
        let tenant_b_lease = tenant_b_stub
            .claim_activation("tenant_b_owner", 30_000)
            .await
            .expect("tenant B activation should claim");

        assert_eq!(
            tenant_b_stub
                .storage_get(&tenant_b_lease, "secret", 1_000)
                .await
                .expect("tenant B read should not fail"),
            None,
            "tenant B must not read tenant A's DO storage even with a forged 64-hex id"
        );
        assert!(
            tenant_b_stub
                .transaction(
                    &tenant_a_lease,
                    vec![DurableObjectStorageOp::Put {
                        key: "secret".to_string(),
                        value: b"forged".to_vec(),
                    }],
                )
                .await
                .is_err(),
            "a lease from tenant A must not authorize tenant B's Durable Object stub"
        );
    }

    #[tokio::test]
    async fn durable_object_per_instance_lanes_make_independent_progress() {
        let (_fixture, substrate, tenant) = fixture();
        let first = substrate.stub_by_name(tenant.clone(), namespace(), "first");
        let second = substrate.stub_by_name(tenant, namespace(), "second");
        let first_lease = first
            .claim_activation("first-owner", 30_000)
            .await
            .expect("first activation should claim");
        let second_lease = second
            .claim_activation("second-owner", 30_000)
            .await
            .expect("second activation should claim");
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let substrate_for_block = substrate.clone();
        let first_key = first.key().clone();
        let blocker = tokio::spawn(async move {
            substrate_for_block
                .block_concurrency_while(&first_key, async move {
                    started_tx
                        .send(())
                        .expect("started receiver should still be waiting");
                    release_rx
                        .await
                        .expect("release sender should unblock first DO");
                    Ok(())
                })
                .await
        });
        started_rx
            .await
            .expect("first DO should enter blockConcurrencyWhile");

        tokio::time::timeout(
            Duration::from_millis(250),
            second.transaction(
                &second_lease,
                vec![DurableObjectStorageOp::Put {
                    key: "state".to_string(),
                    value: b"progress".to_vec(),
                }],
            ),
        )
        .await
        .expect("second DO should not be blocked by first DO lane")
        .expect("second DO write should commit");

        release_tx
            .send(())
            .expect("blocker task should still be waiting");
        blocker
            .await
            .expect("blocker task should join")
            .expect("blocker task should succeed");
        assert_eq!(
            second
                .storage_get(&second_lease, "state", 1_000)
                .await
                .expect("second storage should read"),
            Some(b"progress".to_vec())
        );
        assert!(
            first
                .storage_get(&first_lease, "state", 1_000)
                .await
                .expect("first storage should read")
                .is_none(),
            "first DO lane blocking should not fabricate writes"
        );
    }

    #[tokio::test]
    async fn durable_object_stale_activation_epoch_discards_output_gate_writes() {
        let (_fixture, substrate, tenant) = fixture();
        let stub = substrate.stub_by_name(tenant, namespace(), "epoch");
        let loser = stub
            .claim_activation("loser", 30_000)
            .await
            .expect("loser activation should claim");
        let winner = stub
            .claim_activation("winner", 30_000)
            .await
            .expect("winner activation should claim");

        assert!(
            stub.transaction_with_output_gate(
                &loser,
                vec![DurableObjectStorageOp::Put {
                    key: "state".to_string(),
                    value: b"loser".to_vec(),
                }],
                vec![json!({ "message": "should be discarded" })],
            )
            .await
            .is_err(),
            "stale loser activation must be fenced before storage commit or output release"
        );
        assert_eq!(
            stub.storage_get(&winner, "state", 1_000)
                .await
                .expect("winner read should succeed"),
            None,
            "stale activation write must not reach storage"
        );

        let outcome = stub
            .transaction_with_output_gate(
                &winner,
                vec![DurableObjectStorageOp::Put {
                    key: "state".to_string(),
                    value: b"winner".to_vec(),
                }],
                vec![json!({ "message": "confirmed" })],
            )
            .await
            .expect("winner write should commit");

        assert_eq!(
            outcome.confirmed_outputs,
            vec![json!({ "message": "confirmed" })]
        );
        assert_eq!(
            stub.storage_get(&winner, "state", 1_000)
                .await
                .expect("winner read should succeed"),
            Some(b"winner".to_vec())
        );
    }

    #[tokio::test]
    async fn durable_object_expired_activation_lease_fences_reads_and_writes() {
        let clock = Arc::new(ManualDurableObjectClock::new(1_000));
        let (_fixture, substrate, tenant) = fixture_with_clock(clock.clone());
        let stub = substrate.stub_by_name(tenant, namespace(), "expiry");
        let lease = stub
            .claim_activation("owner", 10)
            .await
            .expect("activation should claim");
        clock.set(1_011);

        let read_error = stub
            .storage_get(&lease, "state", 1_011)
            .await
            .expect_err("expired lease must not read storage");
        assert!(
            matches!(read_error, Error::PreconditionFailed(ref message) if message.contains("expired")),
            "unexpected read error: {read_error:?}"
        );
        let write_error = stub
            .transaction(
                &lease,
                vec![DurableObjectStorageOp::Put {
                    key: "state".to_string(),
                    value: b"expired".to_vec(),
                }],
            )
            .await
            .expect_err("expired lease must not write storage");
        assert!(
            matches!(write_error, Error::PreconditionFailed(ref message) if message.contains("expired")),
            "unexpected write error: {write_error:?}"
        );

        let replacement = stub
            .claim_activation("replacement", 30_000)
            .await
            .expect("replacement activation should claim after expiry");
        stub.transaction(
            &replacement,
            vec![DurableObjectStorageOp::Put {
                key: "state".to_string(),
                value: b"replacement".to_vec(),
            }],
        )
        .await
        .expect("fresh lease should write");
        assert_eq!(
            stub.storage_get(&replacement, "state", 1_011)
                .await
                .expect("fresh lease should read"),
            Some(b"replacement".to_vec())
        );
    }

    #[tokio::test]
    async fn durable_object_alarm_and_websocket_hibernation_round_trip() {
        let (_fixture, substrate, tenant) = fixture();
        let stub = substrate.stub_by_name(tenant, namespace(), "coordination");
        let lease = stub
            .claim_activation("owner", 30_000)
            .await
            .expect("activation should claim");

        stub.set_alarm(
            &lease,
            DurableObjectAlarm {
                scheduled_time_millis: 2_000,
                retry_count: 1,
            },
        )
        .await
        .expect("setAlarm should persist");
        assert_eq!(
            stub.get_alarm(&lease, 1_000)
                .await
                .expect("getAlarm should read"),
            Some(DurableObjectAlarm {
                scheduled_time_millis: 2_000,
                retry_count: 1,
            })
        );
        assert!(
            stub.delete_alarm(&lease)
                .await
                .expect("deleteAlarm should delete"),
            "deleteAlarm should report removing the stored alarm"
        );
        assert_eq!(
            stub.get_alarm(&lease, 1_000)
                .await
                .expect("getAlarm after delete should read"),
            None
        );

        stub.accept_web_socket(&lease, "socket-a", vec!["room-a".to_string()])
            .await
            .expect("acceptWebSocket should persist hibernation record");
        stub.serialize_attachment(&lease, "socket-a", json!({ "lastSeq": 42 }))
            .await
            .expect("serializeAttachment should persist");
        stub.set_web_socket_auto_response(&lease, "socket-a", "pong".to_string())
            .await
            .expect("setWebSocketAutoResponse should persist");

        assert_eq!(
            stub.deserialize_attachment(&lease, "socket-a", 1_000)
                .await
                .expect("deserializeAttachment should read"),
            Some(json!({ "lastSeq": 42 }))
        );
        let sockets = stub
            .get_web_sockets(&lease, Some("room-a"), 1_000)
            .await
            .expect("getWebSockets should list");
        assert_eq!(sockets.len(), 1);
        assert_eq!(sockets[0].socket_id, "socket-a");
        assert_eq!(sockets[0].auto_response.as_deref(), Some("pong"));
    }
}

use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_dynamodb::{list_access_keys, put_access_key};
use nimbus_engine::Engine;

#[test]
fn sync_key_management_owns_embedded_first_use_admission() {
    let data_dir = tempfile::tempdir().expect("temporary data dir should create");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("embedded engine should create"));
    let tenant_id = TenantId::new("tenant-a").expect("tenant id should build");

    put_access_key(
        &engine,
        "AKIATESTANT",
        &tenant_id,
        Some("fixture-dynamodb".to_owned()),
        Some("us-east-1".to_owned()),
    )
    .expect("first synchronous key write should admit the embedded system tenant");

    let keys = list_access_keys(&engine).expect("embedded key catalog should list");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].0, "AKIATESTANT");
    assert_eq!(keys[0].1.tenant, tenant_id.as_str());
    assert_eq!(keys[0].1.region.as_deref(), Some("us-east-1"));
}

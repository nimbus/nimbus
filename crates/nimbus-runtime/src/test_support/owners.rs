use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::{Mutex, OnceLock};

use crate::{RuntimeOwnerId, RuntimeOwnerLease, RuntimeOwnerLeaseIssuer};

/// Returns one stable owner lease per test tenant label.
///
/// Runtime unit tests that are not specifically testing missing authority use
/// this helper so retained-state behavior exercises the same mandatory owner
/// contract as production callers. Exploit and admission sentinels construct
/// ownerless contexts directly instead.
pub(crate) fn runtime_owner_lease_for_test(tenant_label: &str) -> RuntimeOwnerLease {
    static OWNERS: OnceLock<Mutex<HashMap<String, RuntimeOwnerLease>>> = OnceLock::new();
    let owners = OWNERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut owners = owners
        .lock()
        .expect("runtime test owner registry lock should not be poisoned");
    owners
        .entry(tenant_label.to_string())
        .or_insert_with(|| {
            let owner = RuntimeOwnerId::tenant(
                format!("runtime-test:{tenant_label}"),
                NonZeroU64::new(1).expect("test owner incarnation is nonzero"),
                Some(tenant_label),
            )
            .expect("runtime test owner should be valid");
            RuntimeOwnerLeaseIssuer.issue(owner).0
        })
        .clone()
}

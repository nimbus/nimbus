# DynamoDB Adapter — Tenant & Auth Isolation Report (D9.4)

Two access keys bound to two tenants must not cross-read, cross-write, list,
TTL-configure, tag, or infer each other's tables — even when both tenants use
the **same** table name. Proven through the public `dispatch` entrypoint in
`crates/nimbus-dynamodb/tests/tenant_isolation.rs` plus the official-SDK parity
runner.

## Isolation vectors and results

| Vector | Test | Result |
| --- | --- | --- |
| Cross-read / cross-write (same table name, different items) | `tables_and_items_are_isolated_across_tenants` — each tenant reads back only its own value | PASS |
| Table visibility (describe + list) | `one_tenants_table_is_invisible_to_another` — globex's DescribeTable → ResourceNotFound, ListTables omits it | PASS |
| TTL config leakage | `ttl_and_tag_metadata_are_isolated` — globex's identically-named table reports TTL DISABLED after acme enables it | PASS |
| Tag leakage | same test — globex's ListTagsOfResource is empty after acme tags its table | PASS |
| Wrong/unbound access key | `wrong_access_key_cannot_act_as_another_tenant` → UnrecognizedClientException | PASS |
| Wrong SigV4 signature (strict) | `strict_mode_rejects_a_wrong_secret` (parity runner) → InvalidSignatureException | PASS |
| Cross-tenant through the real SDK | `two_access_keys_are_isolated_through_official_sdk` (parity runner) | PASS |

Reserved-store isolation note: the catalog (`_ddb_catalog`), TTL (`_ddb_ttl`),
tag (`_ddb_tags`), and stream stores are all tenant-scoped, so two tenants'
same-named tables share neither data nor metadata. The persisted access-key
store (`_ddb_access_keys`) is the one deliberately global store and maps each key
to exactly one tenant.

## Verdict

- **0 cross-tenant visibility violations.** No tenant can describe, read, list,
  or infer another tenant's table.
- **0 cross-tenant mutation violations.** Writes, TTL config, and tags are
  scoped to the writing tenant; same-named tables are fully independent.
- **Auth fails closed.** An unbound access key is `UnrecognizedClientException`;
  a wrong signature in strict mode is `InvalidSignatureException`.
- `cargo test -p nimbus-dynamodb --test tenant_isolation` → **4 passed, 0 failed**.

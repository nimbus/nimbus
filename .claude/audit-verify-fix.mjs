export const meta = {
  name: 'audit-verify-fix',
  description: 'Independently re-verify each 2026-06-16 audit finding from source (refute-by-default) and design the canonical architectural fix for every confirmed+understood issue, then synthesize an ordered, disjoint-file implementation plan',
  phases: [
    { title: 'Verify+Design' },
    { title: 'Plan' },
  ],
}

const INVARIANTS = `Architecture invariants (canonical fixes must respect these):
- nimbus-core ZERO I/O; nimbus-runtime ZERO workspace deps.
- SINGLE mutation path (engine apply_mutation_with_mode* + queued journal). No second path.
- Storage atomicity: doc write + index effects + commit-log append = ONE txn.
- Runtime bundles SHA-256 integrity-checked before invocation; host ops via the Engine path (no bypass).
- Schema optional. Adapters MUST NOT expose ctx.services/sandboxes/sessions shortcuts. Isolates are NOT sandboxes.

Pre-launch posture: breaking renames PREFERRED over compat shims; NO migration shims / feature flags for legacy / backwards-compat code. Missing-backcompat is NOT a finding; a LEFTOVER compat shim/alias that should have been deleted IS. Banned durable names (must be 0 in prod): ServiceImplementation, SandboxBackedServiceImplementation, SandboxImageLaunchSpec, SandboxBuildLaunchSpec, SandboxImageProcessOverrides, start_from_image, start_from_build, tenant_workload_identity_*.

A CANONICAL fix is the architecturally-correct, enterprise-grade solution — NOT a band-aid. Where the repo already has a correct pattern for the same problem (e.g. postgres_string_literal escaping, AccessKeyRegistry credential->tenant binding, maintained_indexes()/is_maintained() filtering, atomic AtomicWriteBatch), the fix MUST mirror that canonical pattern rather than invent a new one. Every behavior fix MUST come with a behavioral test that fails before and passes after. Gates (fmt/clippy/deny) are already green — do not propose lint/format changes.`

const VERIFY_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['finding_id', 'verdict', 'corrected_severity', 'understood', 'root_cause', 'canonical_fix'],
  properties: {
    finding_id: { type: 'string' },
    verdict: { enum: ['confirmed', 'refuted', 'refined'], description: 'confirmed = real as stated; refined = real but materially different than described; refuted = not a genuine issue under pre-launch posture' },
    corrected_severity: { enum: ['critical', 'high', 'medium', 'low', 'info', 'none'] },
    understood: { enum: ['yes', 'partial', 'no'], description: 'Do you fully understand the root cause and the correct fix with high confidence?' },
    root_cause: { type: 'string', description: 'Precise mechanism with CURRENT file:line + quoted code you actually read (not the audit text). State what is wrong and why.' },
    canonical_fix: {
      type: 'object', additionalProperties: false,
      required: ['approach', 'files', 'edits', 'test', 'mirrors_pattern', 'decision_needed', 'risk'],
      properties: {
        approach: { type: 'string', description: 'The architecturally-correct solution and WHY it is canonical for this repo.' },
        files: { type: 'array', items: { type: 'string' }, description: 'Exact files the fix touches (for disjoint-batch planning).' },
        edits: { type: 'string', description: 'Concrete, implementable change description: what to change at which lines, the new logic/signatures.' },
        test: { type: 'string', description: 'The behavioral test to add: file, name, and the exact assertion that distinguishes fixed from broken.' },
        mirrors_pattern: { type: 'string', description: 'The existing in-repo canonical pattern this mirrors, with file:line, or "none (new pattern, justified)".' },
        decision_needed: { type: 'string', description: '"none", or the genuine human decision required + your recommended default.' },
        risk: { enum: ['low', 'medium', 'high'], description: 'Implementation blast radius / regression risk.' },
      },
    },
  },
}

const FINDINGS = [
  { id: 'M1', sev: 'medium', cat: 'security', area: 'storage-core',
    title: 'Encrypted-redb partial-write RMW silently swallows AES-GCM-SIV integrity failures',
    loc: 'crates/nimbus-storage/src/encrypted_redb.rs:433-435 (file write) & :715-717 (memory write); read_encrypted_page error kinds at :281-286/:586-591',
    claim: 'Partial-page writes RMW via `read_encrypted_page(...).unwrap_or([0u8; LOGICAL_PAGE_SIZE])`. Bounds-checks guarantee the page is present, so the only reachable failure unwrap_or swallows is the AES-256-GCM-SIV integrity failure (InvalidData "decryption failed"). Every other call site propagates with `?` (incl. set_len doing the same RMW). A partial write over a tampered page zero-fills + re-encrypts, destroying corruption evidence + surrounding plaintext.',
    proposed: 'Match the error: zero-fill only on UnexpectedEof / "read beyond end of buffer" not-present case; propagate InvalidData (+other IO errors) out of write. Add a corrupt-page-then-partial-write test asserting InvalidData not zeroing.' },
  { id: 'M2', sev: 'medium', cat: 'security', area: 'storage-providers',
    title: 'SQLite/MySQL index field-name JSON-path interpolation does not escape the single-quote delimiter',
    loc: 'crates/nimbus-storage/src/sqlite/schema.rs:317-322 (json_extract_expr); crates/nimbus-storage/src/mysql/query_helpers.rs:346-350 (mysql_generated_column_expr); canonical PostgreSQL escaper postgres/backend.rs:1418/1425 (postgres_string_literal); validator nimbus-core/types.rs:415-434 (validate_logical_name)',
    claim: 'SQLite escapes only `"`, MySQL only `\\` and `"`; neither escapes the `\'...\'` string-literal delimiter. PostgreSQL routes the field through postgres_string_literal (doubles `\'`) and is safe. validate_logical_name is applied to tenant/table/index ids but NEVER to FieldSchema.name, so a field named `a\' || (subquery) || \'` injects SQL into index DDL on SQLite (via execute_batch, multi-statement) / MySQL while safe on PostgreSQL. Reachable via the privileged schema-authoring path.',
    proposed: 'Defense in depth: route field names through validate_logical_name in nimbus-core AND make SQLite/MySQL double `\'` like PostgreSQL (shared string-literal escaper). Negative test on all three backends.' },
  { id: 'M3', sev: 'medium', cat: 'correctness', area: 'sandbox',
    title: 'Egress proxy truncates plain-HTTP request bodies in the ForwardHttp path',
    loc: 'crates/nimbus-sandbox/src/egress_proxy.rs:316-323 (handle_client ForwardHttp) with read_http_headers :330-357; tunnel_connect :461-480; production caller backends/container/runtime.rs:731',
    claim: 'read_http_headers returns Ok the instant find_header_end matches — never reads the body. ForwardHttp forwards only co-buffered bytes then immediately relays the response, never reading the client socket again. tunnel_connect (HTTPS/CONNECT) does bidirectional io::copy, so the asymmetry confirms defect. Any allowed POST/PUT whose body does not fully arrive in the same read() as headers loses the remainder and stalls until the 10s timeout.',
    proposed: 'After writing the buffered prefix, fully relay the client body to upstream (second io::copy thread with shutdown(Write), mirroring tunnel_connect, OR drain by Content-Length/Transfer-Encoding) before reading the response. Regression test posting a body larger than the header buffer.' },
  { id: 'M4', sev: 'medium', cat: 'correctness', area: 'engine',
    title: 'Consistency verifier digests table_identities but never field-compares them (green-on-drift)',
    loc: 'crates/nimbus-engine/src/verification.rs:100-222 (compare_materialized_journal_snapshots), :291-305 (canonicalize); driven by engine/queries/verification.rs:83 (ok: mismatches.is_empty())',
    claim: 'canonicalize includes table_identities and snapshot_fingerprint hashes the whole struct (so identity drift changes the digest), but compare_materialized_journal_snapshots field-compares only version/applied_sequence/durable_head/schema/documents/scheduled_execution_ids — NO branch compares table_identities. ok = mismatches.is_empty(). table_identities is a real mutable per-table field (Active/Hidden/Deleting), so the triad can diverge: two snapshots differing only in stable table identity yield different digests yet report ok=true with empty mismatches — the diff and the integrity flag silently disagree.',
    proposed: 'Add a table_identities comparison branch (sorted CanonicalTableIdentity vectors, emit a table_identities mismatch), OR derive ok from fingerprint-digest equality so the flag can never disagree with the hash. Test divergence on identity only -> ok=false.' },
  { id: 'M5', sev: 'medium', cat: 'correctness', area: 'storage-core',
    title: 'Index-state maintenance inconsistent across write paths',
    loc: 'crates/nimbus-storage/src/store/write/batch.rs:170-176/:236-251/:301-307; store/schema_rewrite.rs:70-75; store/journal.rs:467-521; interactive filter index/maintenance/transaction.rs:28/93/158 (is_maintained); history store/index_versions.rs:711 (maintained_indexes); is_maintained nimbus-core/schema.rs:78-80',
    claim: 'batch + journal-replay paths iterate `for index in indexes` with NO state filter; interactive path filters .is_maintained() and history path uses maintained_indexes() (= Backfilling|Enabled). Pending/Deleting indexes handled differently across equivalent paths. Latent today (IndexState defaults Enabled; non-Enabled constructions are #[cfg(test)]) but reconcile_index_metadata preserves incoming state, so structurally non-Enabled states can reach table_schema.indexes -> silent index drift once backfill lifecycle ships.',
    proposed: 'Route every live-index write through maintained_indexes()/is_maintained() in batch.rs apply_insert/apply_update/apply_delete, schema_rewrite.rs, journal.rs. Test that a staged Pending index writes no INDEXES entry on the batch path (parity with interactive).' },
  { id: 'M6', sev: 'medium', cat: 'correctness', area: 'tenant-node-operator',
    title: 'reconcile_running treats inspect InvalidInput as workload-missing then starts',
    loc: 'crates/nimbus-node/src/reconciler.rs:179-187; in-memory backend direct_process.rs:154-159; live backend zbus_client/mod.rs:278-279 + error.rs:64-73/:82-87; production caller node_workload_executor.rs:189-198; Error::NotFound exists nimbus-core error.rs:103',
    claim: 'Arm `Ok(_) | Err(Error::InvalidInput(_))` merges not-running with ALL InvalidInput then calls backend.start. Missing-workload is InvalidInput only in the in-memory backend; the live backend maps missing units to inactive-dead/NotFound, so a live InvalidInput is a genuine systemd .Failed inspect fault — silently treated as workload-absent, triggering a redundant StartTransientUnit and masking the error.',
    proposed: 'Return Error::NotFound for missing-workload in the in-memory backends and match `Ok(_) | Err(NotFound)`, so InvalidInput propagates via the final Err arm. Tests for both branches.' },
  { id: 'M7', sev: 'medium', cat: 'correctness', area: 'js-firebase',
    title: 'Firestore value encoder cannot write Date/Timestamp/Bytes/GeoPoint/DocumentReference',
    loc: 'packages/firebase/src/internal/document-data.ts:154-194 (encodeFirestoreValue) & :239-248 (decodeFirestoreValue)',
    claim: 'encode handles null/boolean/number/string/array/plain-object then throws "Unsupported Firestore value type". No branch for Date (timestampValue), Uint8Array (bytesValue), GeoPoint, DocumentReference — yet decode RETURNS those wire kinds. setDoc(ref,{createdAt:new Date()}) throws on write while reads pass them back, breaking read-modify-write round-tripping. Real Firebase Web SDK treats these as first-class writable.',
    proposed: 'Add encoder branches for Date (->timestampValue ISO-8601), Uint8Array (->bytesValue base64), and the Timestamp/GeoPoint/Bytes/DocumentReference sentinel shapes (->their *Value wire forms); OR make decode reject (not passthrough) those kinds and document the narrowing symmetrically. Prefer adding write support (canonical Firebase parity).' },
  { id: 'M8', sev: 'medium', cat: 'compat-shim', area: 'js-sdk-ui',
    title: 'convex/internal/shared.ts forks the nimbus original and has already drifted',
    loc: 'packages/convex/src/internal/shared.ts:1-218 (vs packages/nimbus/src/internal/shared.ts:1-228); dead helpers :176-217; drift: convex websocketUrlFromBase :194 (no stripTrailingSlash) vs nimbus :204',
    claim: 'The two files are the same shapes modulo Convex*/Nimbus* prefixes. Fork already drifted: convex websocketUrlFromBase yields //ws for a trailing-slash base. Five helpers (validateDeploymentUrl/stripTrailingSlash/websocketUrlFromBase/normalizeArgs/createConvexError) have ZERO importers in packages/convex (live path inherits from nimbus base classes). THIN-wrapper import-and-alias is already the norm in sibling files; this forked file is the inconsistent exception.',
    proposed: 'Re-export shared shapes/factories from @nimbus/nimbus/internal/shared and alias the Convex-branded names; delete the five dead drifted helpers (:176-217). Verify typecheck + tests still pass.' },
  { id: 'M9', sev: 'medium', cat: 'security', area: 'adapters-mongodb',
    title: 'MongoDB: one tenant-agnostic credential reaches every tenant via the wire $db name',
    loc: 'crates/nimbus-mongodb/src/lib.rs:14-34 (AuthConfig), crates/nimbus-mongodb/src/commands/tenant.rs:10-15 (resolve_tenant_id), crates/nimbus-server/src/adapters/mongodb/listener.rs:24 (loopback guard); canonical DynamoDB pattern crates/nimbus-dynamodb/src/tenant.rs:71-186 (AccessKeyRegistry::resolve(access_key_id)->TenantId)',
    claim: 'AuthConfig holds one SCRAM credential. dispatch() authenticates the connection but the tenant is then selected from the wire $db name via resolve_tenant_id(db_name), NOT from the authenticated principal — so any holder of the one credential reaches every tenant by varying $db. Mitigated ONLY by guard_listener_is_loopback_only. Auth gate is FIXED (prior H1 resolved); per-tenant credential binding is OPEN. DynamoDB binds each access key to a tenant.',
    proposed: 'Bind each SCRAM credential to a specific tenant (mirror DynamoDB AccessKeyRegistry) so authentication, not $db, decides tenant. This is feature-sized — design the canonical credential->tenant registry for MongoDB, decide config surface, keep the loopback guard load-bearing until it ships. FLAG the scope/config decision.' },
  { id: 'L1', sev: 'low', cat: 'docs-accuracy', area: 'engine',
    title: 'Encryption test comments claim libsql replica is the only fully-wired path',
    loc: 'crates/nimbus-engine/src/engine/encryption/mod.rs:237 & :255 vs :20-24/:44-57; sibling tests :166-182/:184-199',
    claim: 'Test comments assert libsql replica is "the only fully-wired path" but the module doc + match arms treat all four families (EmbeddedSqlite/EmbeddedRedb/ControlPlaneRedb/LibsqlReplicaCache) as fully supported, and sibling tests prove sqlite+redb succeed.',
    proposed: 'Replace both comments with the accurate rationale (libsql used only to exercise the missing-key-file error from one representative wired family); drop "only fully-wired path".' },
  { id: 'L2', sev: 'low', cat: 'docs-accuracy', area: 'storage-core',
    title: 'TenantStore docstring still frames redb as a transitional migration-window surface',
    loc: 'crates/nimbus-storage/src/store.rs:76-88; peers nimbus-engine/persistence/tenant.rs:20-24, persistence/provider.rs:198-214',
    claim: 'Docstring says "Authoritative tenant persistence surface during the migration window ... SQLite migration work should preserve ...". But redb and sqlite are live coexisting peer backends (TenantPersistence::Redb/Sqlite both constructed).',
    proposed: 'Rewrite the docstring to describe TenantStore as the embedded redb backend (one of redb/sqlite/libsql/postgres/mysql); drop the migration-window framing.' },
  { id: 'L3', sev: 'low', cat: 'compat-shim', area: 'storage-core',
    title: 'Dual durable_record terminology: pass-through wrappers + type alias duplicate the API',
    loc: 'crates/nimbus-storage/src/commit_log.rs:16-26 (serialize/deserialize_durable_record); alias crates/nimbus-core/src/mutation.rs:365 (DurableMutationRecord = TenantEventRecord); ~18 call sites',
    claim: 'serialize/deserialize_durable_record are byte-identical pass-throughs to *_tenant_event_record, self-described "Compatibility wrapper for storage code that still uses the old durable mutation terminology." DurableMutationRecord is just a type alias. Pre-launch prefers deleting compat shims over dual terminology.',
    proposed: 'Pick one terminology. Either collapse the alias + *_durable_record wrappers into tenant_event_record names (update ~18 call sites, re-point alias), OR if durable_record is canonical delete the alias indirection + "compatibility wrapper" comments so they read as first-class API. Decide which name is canonical.' },
  { id: 'L4', sev: 'low', cat: 'naming', area: 'sandbox-services',
    title: 'Launch->Start rename half-done: launch_* identifiers + serde wire field launch_mode survive (committed + in-flight)',
    loc: 'crates/nimbus-sandbox/src/backends/krun/vm.rs:93/244/253/281 + vm/start.rs; crates/nimbus-sandbox/src/backends/container/runtime.rs:85/104/129/160/231/241/296-314/334-345/390/435-475 (incl serde field launch_mode, cleanup_manifest_launch_artifacts, ensure_launch_quota, build_launch_plan, conmon_launch, launch_manifest, PreparedMaterializedImageLaunch, ContainerLaunchArtifact, resolved_launch)',
    claim: 'Public discriminant types renamed (*StartMode/*StartPlan) and top-level fns (*_start), but a large internal surface still reads launch_* including the PERSISTED serde field launch_mode (on-disk manifest). Not a banned-name violation; pre-launch prefers a clean rename. Note launch_mode is a wire field -> renaming it is a deliberate (acceptable pre-launch) wire-format change. (Merges committed-state L4 + in-flight L12.)',
    proposed: 'Finish the rename across the internal container+krun surface: launch_mode->start_mode (serde wire change, no compat shim), *ResolvedLaunchSpec/*LaunchArtifact->*ResolvedStartSpec/*StartArtifact, cleanup_manifest_launch_artifacts/ensure_launch_quota/build_launch_plan/conmon_launch/launch_manifest and resolved_launch locals -> *start*. Verify no dangling old names + serde round-trip.' },
  { id: 'L5', sev: 'low', cat: 'docs-accuracy', area: 'js-firebase',
    title: 'Nested field-path rejection says "not supported yet" (contradicts intentionally-narrow docs)',
    loc: 'packages/firebase/src/internal/firestore-helpers.ts:376-380; docs/reference/firebase/compatibility.md:67; selftest rest_surface.mjs:889',
    claim: 'Error throws "Firestore <context> nested field paths are not supported yet." but docs frame it as intentional ("intentionally narrow") and a selftest asserts the throw. "yet" reads like a TODO contradicting the deliberate boundary.',
    proposed: 'Drop "yet" — reword to match the intentionally-narrow compatibility language. Keep the selftest passing (update the asserted string if it matches on text).' },
  { id: 'L6', sev: 'low', cat: 'compat-shim', area: 'js-sdk-ui',
    title: 'convex/values.ts is a byte-identical copy of nimbus values.ts, not a thin re-export',
    loc: 'packages/convex/src/values.ts:1-66 (vs packages/nimbus/src/values.ts:1-66, byte-identical); nimbus exports "./values"; convex already depends on @nimbus/nimbus',
    claim: 'convex/values.ts re-implements the entire validator builder verbatim (validator<T>, the full `v` object, Validator/GenericId/Infer). Sibling convex files are thin wrappers; values.ts has zero reference to @nimbus/nimbus. Violates the THIN-wrapper rule; future drift risk (convex/server.ts imports Infer/Validator from local ./values.ts).',
    proposed: 'Replace the body with re-exports: `export { v } from "@nimbus/nimbus/values"; export type { Validator, Infer, GenericId } from "@nimbus/nimbus/values";`. Verify typecheck.' },
  { id: 'L7', sev: 'low', cat: 'correctness', area: 'js-sdk-ui',
    title: 'filterFunctionTree returns count:0 with a false "recomputed below" comment',
    loc: 'packages/nimbus-ui/src/shell/function-tree.ts:137-138; consumer function-tree-view.tsx:47 (reads unfiltered tree.count); spec function-tree.spec.ts',
    claim: 'filterFunctionTree returns `{ ..., count: 0, // recomputed below }` but the object is returned immediately and count is never recomputed. Current consumer reads tree.count from the UNFILTERED tree so the lie is invisible; no spec asserts filtered.count. Latent trap for future callers.',
    proposed: 'Either recompute count by summing matched leaf functions before returning, or drop the count field from the filtered result type and remove the comment. Add/extend a spec asserting filtered count.' },
  { id: 'L8', sev: 'low', cat: 'naming', area: 'js-sdk-ui',
    title: 'Hardcoded "convex" branding throughout the canonical @nimbus/codegen runtime-bundle emit',
    loc: 'packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs:20; runtime_bundle_dispatch_invocation.mjs:5/9/32/39/45/55; runtime_bundle_query_helpers.mjs:33/68; runtime_bundle_mutation_helpers.mjs:45; runtime_bundle_preamble.mjs:156; reference_helpers.mjs:26',
    claim: 'Generated error strings + constants hardcode Convex though @nimbus/codegen also targets native nimbus/ apps. E.g. "convex function or route not found", "unsupported convex filter op", host_call_session_id "convex-runtime-query-plan". Session-ids are opaque to Rust host.rs (diagnostic only). Cosmetic/diagnostic drift.',
    proposed: 'Rename user-facing error strings to neutral wording ("function or route not found", "unsupported filter op") and host_call_session_id constants to nimbus-neutral labels. Verify codegen snapshot/golden tests updated.' },
  { id: 'L9', sev: 'low', cat: 'modularity', area: 'engine',
    title: 'execution_units/tests.rs is 2001 lines, over the 2000-line decompose-or-document threshold',
    loc: 'crates/nimbus-engine/src/engine/execution_units/tests.rs:1-2001',
    claim: 'wc -l = 2001. Repo rule: files >=2000 must be decomposed or documented as a strong ownership-based exception; no exception recorded.',
    proposed: 'Split into concept-owned children under execution_units/tests/ (occ/batch_writes/field_transforms/triggers) matching the behaviors exercised, OR add an explicit ownership-exception note in the owning active plan. Recommend the lower-risk correct option for a coherent test module.' },
  { id: 'L10', sev: 'low', cat: 'modularity', area: 'storage-providers',
    title: 'Three >=2000-line provider test files lack the required documented ownership exception',
    loc: 'crates/nimbus-storage/src/tests/libsql_provider.rs (2406), mysql_provider.rs (2286), postgres_provider.rs (2235)',
    claim: 'All three exceed the 2000-line hard threshold. Each is a single flat module of #[tokio::test] #[serial] testcontainer integration tests. No owning active plan documents the size as a deliberate exception.',
    proposed: 'Either split each along natural seams (registry/namespaces, document MVCC, index versions, scheduler) under tests/<provider>_provider/, OR add a one-line ownership-exception note in the owning storage/multi-backend active plan. Recommend which.' },
  { id: 'L11', sev: 'low', cat: 'consistency', area: 'adapters',
    title: 'Two wire-protocol adapters use divergent credential->tenant models with no shared contract',
    loc: 'crates/nimbus-dynamodb/src/tenant.rs:71-186 (per-key tenant binding) vs crates/nimbus-mongodb/src/commands/tenant.rs:10-26 (db-name-derived); docs/private/architecture/runtime/adapter-boundary.md / auth-runtime-trust.md',
    claim: 'DynamoDB credential is tenant-bound (strict signed mode safe off-loopback); MongoDB credential is tenant-agnostic (entirely loopback-gated). No shared trait/documented contract states the per-adapter tenant-resolution rule, so the next adapter author has no canonical pattern.',
    proposed: 'Document the canonical "authentication decides tenant, never a wire-supplied name" rule in adapter-boundary.md/auth-runtime-trust.md and make DynamoDB the reference pattern; record MongoDB db-name model as the known exception with its loopback precondition. (Pairs with M9; docs/private is local-only.)' },
  { id: 'I1', sev: 'info', cat: 'modularity', area: 'sandbox',
    title: 'krun vm/tests.rs exceeds the 1500-line soft modularity threshold',
    loc: 'crates/nimbus-sandbox/src/backends/krun/vm/tests.rs (1526 lines)',
    claim: 'wc -l = 1526; only file in the slice at/above 1500. 1500-1999 needs an explicit ownership justification in the owning active plan.',
    proposed: 'Record a one-line ownership justification in the sandbox active plan, OR split along the natural seams (start-planning / readiness-probe / restart-policy / manifest-serde). Recommend which.' },
  { id: 'G3', sev: 'info', cat: 'architecture-posture', area: 'runtime',
    title: 'Bundle SHA-256 enforcement is provenance-gated, not unconditional (doc vs code mismatch)',
    loc: 'crates/nimbus-runtime/src/runtime/bundle.rs:187 (verify_integrity early-returns Ok when expected_sha256 is None); ARCHITECTURE.md "integrity-checked before every invocation"',
    claim: 'verify_integrity() returns Ok(()) early when expected_sha256 is None, so the "SHA-256 integrity-checked before every invocation" invariant only actually hashes when provenance supplied a digest. Possibly intended (path-backed bundles without provenance), but the doc wording is stronger than the code.',
    proposed: 'Determine defect vs intended. If intended: reconcile the ARCHITECTURE.md wording with the provenance-gated reality. If a gap: make the gate unconditional (compute + record a digest on first load and verify thereafter). Recommend, and flag the decision.' },
]

function verifyPrompt(f) {
  return `You are independently RE-VERIFYING one finding from the Nimbus 2026-06-16 audit, then designing its CANONICAL architectural fix. Do NOT trust the audit text — open the actual code and judge for yourself. Refute-by-default: if you cannot prove it from current source, say refuted.

FINDING ${f.id} [${f.sev} ${f.cat}] (${f.area})
Title: ${f.title}
Cited location(s): ${f.loc}
Audit's claim: ${f.claim}
Audit's proposed fix: ${f.proposed}

Steps:
1. Open every cited file at the CURRENT working-tree state (some changes are uncommitted — use git diff/git show as needed). Read the surrounding function(s), callers, and existing tests. Quote the actual current code (with current line numbers) in root_cause — line numbers may have shifted from the audit.
2. Decide verdict: confirmed / refined / refuted, and corrected_severity. Apply pre-launch posture (missing-backcompat is NOT a bug; leftover shim IS).
3. If confirmed/refined and you fully understand it, design the CANONICAL fix: the architecturally-correct, enterprise-grade solution. Where the repo already solves the same problem correctly elsewhere, MIRROR that pattern and cite it (mirrors_pattern). Specify exact files, concrete edits, and the behavioral test that fails-before/passes-after. Set decision_needed only for a GENUINE judgment call (e.g. naming choice, feature scope), with your recommended default.
4. Be honest about implementation risk.

${INVARIANTS}

You are READ-ONLY in this phase: investigate and design, but DO NOT modify any file. Return the structured verdict + canonical_fix.`
}

log(`Independently re-verifying + designing canonical fixes for ${FINDINGS.length} findings (read-only).`)

const verified = await parallel(FINDINGS.map(f => () =>
  agent(verifyPrompt(f), { label: `verify:${f.id}`, phase: 'Verify+Design', schema: VERIFY_SCHEMA, agentType: 'general-purpose' })
    .then(v => ({ ...v, _sev: f.sev, _title: f.title, _area: f.area }))
))

const ok = verified.filter(Boolean)
const confirmed = ok.filter(v => v.verdict === 'confirmed' || v.verdict === 'refined')
const refuted = ok.filter(v => v.verdict === 'refuted')
log(`Re-verification: ${confirmed.length} confirmed/refined, ${refuted.length} refuted, of ${ok.length} examined.`)

phase('Plan')
const PLAN_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['overall', 'confirmed_count', 'refuted', 'batches', 'decisions'],
  properties: {
    overall: { type: 'string' },
    confirmed_count: { type: 'number' },
    refuted: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['finding_id', 'reason'], properties: { finding_id: { type: 'string' }, reason: { type: 'string' } } } },
    batches: {
      type: 'array', description: 'Implementation batches ordered for safe sequential application; files MUST be disjoint across batches that could run in parallel, and ordered integrity-critical-first.',
      items: {
        type: 'object', additionalProperties: false,
        required: ['batch_id', 'findings', 'files', 'rationale', 'risk'],
        properties: {
          batch_id: { type: 'string' },
          findings: { type: 'array', items: { type: 'string' } },
          files: { type: 'array', items: { type: 'string' } },
          rationale: { type: 'string' },
          risk: { enum: ['low', 'medium', 'high'] },
        },
      },
    },
    decisions: { type: 'array', description: 'Genuine human decision points before/within implementation, each with a recommended default.', items: { type: 'object', additionalProperties: false, required: ['finding_id', 'decision', 'recommendation'], properties: { finding_id: { type: 'string' }, decision: { type: 'string' }, recommendation: { type: 'string' } } } },
  },
}

const plan = await agent(
  `You are the implementation-planning lead. Below are the independently re-verified audit findings with their canonical fix designs. Produce an ordered implementation plan.

Rules:
- Drop refuted findings (list them in refuted[] with the reason).
- Group confirmed/refined fixes into BATCHES whose touched file sets are DISJOINT from each other, so each batch can be implemented + verified without cross-conflict. Within the plan, order integrity/security-critical correctness fixes FIRST (M1 encrypted-redb, M4 verifier, M3 egress, M5 index-state, M6 reconciler, M2 SQL-escape), then compat/parity (M7, M8), then the feature-sized M9, then low naming/docs/modularity cleanup.
- Note any finding whose files overlap another's (so it must be a separate sequential batch, not parallel).
- Surface every genuine decision_needed in decisions[] with a recommended default (especially M9 scope/config, L3 canonical-name choice, L9/L10/I1 split-vs-document, G3 defect-vs-intended).

VERIFIED FINDINGS:
${JSON.stringify(confirmed.map(v => ({ id: v.finding_id, verdict: v.verdict, severity: v.corrected_severity, understood: v.understood, area: v._area, title: v._title, root_cause: v.root_cause, fix: v.canonical_fix })), null, 1)}

REFUTED:
${JSON.stringify(refuted.map(v => ({ id: v.finding_id, severity: v.corrected_severity, root_cause: v.root_cause })), null, 1)}`,
  { label: 'plan', phase: 'Plan', schema: PLAN_SCHEMA, agentType: 'general-purpose' }
)

return {
  mode: 'verify-and-design',
  examined: ok.length,
  confirmed: confirmed.length,
  refuted: refuted.length,
  verified_findings: confirmed.map(v => ({ id: v.finding_id, verdict: v.verdict, severity: v.corrected_severity, understood: v.understood, title: v._title, canonical_fix: v.canonical_fix, root_cause: v.root_cause })),
  refuted_findings: refuted.map(v => ({ id: v.finding_id, title: v._title, severity: v.corrected_severity, reason: v.root_cause })),
  plan,
}

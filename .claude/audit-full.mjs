export const meta = {
  name: 'audit-full',
  description: 'Entire fresh whole-repo code review + audit of current state (all 27 crates + 7 JS packages + docs/website/CI), plus regression-verification of the 2026-06-09 critical/high findings',
  phases: [
    { title: 'Audit' },
    { title: 'Verify' },
    { title: 'Synthesize' },
    { title: 'Critic' },
  ],
}

// ---- shared context -------------------------------------------------------
// Fresh whole-repo audit: agents audit the CURRENT state of the working tree
// (committed + uncommitted), not a diff range. HEAD is referenced only so the
// regression slice can compare prior findings against present code.
const HEAD = args?.head || '0d7ab207e'

// Generated/vendored blobs to keep out of every read (not hand-written review surface).
const EXCL = [
  ':(exclude)**/*.lock',
  ':(exclude)Cargo.lock',
  ':(exclude)**/package-lock.json',
  ':(exclude)tests/runtime/node/**/*.json',
  ':(exclude)tests/runtime/node/**/*.csv',
  ':(exclude)crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/**',
  ':(exclude)**/dist/**',
].join(' ')

// Build/lint/format gates are GREEN as of this run — do not report fmt/clippy/deny issues.
const GATES = `Build gate status at audit time (already verified GREEN — do NOT re-report these):
- cargo fmt --all --check : PASS (exit 0)
- make clippy             : PASS (exit 0)
- make deny               : PASS (exit 0)
So: no formatting, no clippy lints, no dependency/license-advisory findings. Focus on what gates CANNOT catch — logic/correctness bugs, security/auth/trust-boundary defects, architecture-invariant violations, weak/absent tests, dead code, leftover compat shims, naming drift, oversized files, and doc/code drift.`

const INVARIANTS = `Architecture invariants to audit against (violations are findings):
- nimbus-core has ZERO I/O (types + validation only; no fs/network).
- nimbus-runtime has ZERO workspace deps (defines V8 surface + HostBridge trait; Nimbus integration lives in the server bridge impl).
- SINGLE mutation path: every mutation (HTTP, WS, scheduler, V8) flows through engine apply_mutation_with_mode* + the queued journal path. No second path.
- Storage atomicity: document write + index effects + commit-log append must be ONE storage transaction. Never a doc without its index entries; never a commit without the doc write.
- Runtime bundles are SHA-256 integrity-checked before every invocation; runtime host ops go through the same Service/Engine path as direct HTTP (no bypass).
- Schema is optional: a table without a schema accepts any document; setting a schema only adds constraints.
- Adapters must NOT expose ctx.services / ctx.sandboxes / ctx.sessions shortcuts.
- Runtime invocation isolates are NOT SDK sandboxes.

Pre-launch posture (IMPORTANT — affects what counts as a finding):
- This repo has NOT launched. Breaking renames are PREFERRED over compatibility aliases/shims.
- "Missing backwards compatibility" is NOT a finding.
- A LEFTOVER compat shim/alias/deprecated path that should have been deleted IS a finding (category compat-shim).
- Banned durable names that must be 0 in production code: ServiceImplementation, SandboxBackedServiceImplementation, SandboxImageLaunchSpec, SandboxBuildLaunchSpec, SandboxImageProcessOverrides, start_from_image, start_from_build, tenant_workload_identity_*, non-sandbox service_process_snapshot.

Naming rules: Backend=executor seam; Spec=declarative payload; Kind=discriminant. No Launch/Backing in durable types. Concept-owned filenames (bootstrap.rs, provider.rs, read.rs, write.rs, state.rs) not helpers.rs/common.rs/misc.rs/utils.rs unless ownership is truly shared.

Modularity: <1500 lines OK; 1500-1999 needs an explicit ownership justification; >=2000 must be decomposed or documented as a strong ownership exception.

Test quality: every test must assert a specific behavioral outcome (happy + edge + error). A test that only checks "did not panic" or "compiles" is a finding (category test-quality).`

const FINDINGS_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['slice', 'summary', 'findings'],
  properties: {
    slice: { type: 'string' },
    summary: { type: 'string', description: 'One-paragraph health read of this slice (call out what is clean too).' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['severity', 'category', 'title', 'location', 'evidence', 'impact', 'fix'],
        properties: {
          severity: { enum: ['critical', 'high', 'medium', 'low', 'info'] },
          category: { enum: ['correctness', 'security', 'architecture-invariant', 'test-quality', 'naming', 'modularity', 'docs-accuracy', 'dead-code', 'build-ci', 'compat-shim'] },
          title: { type: 'string' },
          location: { type: 'string', description: 'file:line(s), exact.' },
          evidence: { type: 'string', description: 'Concrete quoted code/diff/doc proving the issue. No hand-waving.' },
          impact: { type: 'string' },
          fix: { type: 'string', description: 'Specific recommended change.' },
        },
      },
    },
  },
}

const VERDICT_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['verdict', 'reasoning', 'corrected_severity'],
  properties: {
    verdict: { enum: ['confirmed', 'refuted', 'uncertain'], description: 'confirmed=real issue; refuted=not a real issue / reviewer misread; uncertain=cannot determine without runtime.' },
    reasoning: { type: 'string', description: 'What you checked in the actual code to reach this verdict.' },
    corrected_severity: { enum: ['critical', 'high', 'medium', 'low', 'info', 'none'], description: 'Re-rated severity after verification (none if refuted).' },
  },
}

function auditPrompt(slice) {
  return `You are auditing ONE slice of an ENTIRE FRESH whole-repo code review. You audit the CURRENT STATE of the working tree (committed + uncommitted), not a diff. Your slice: "${slice.key}".

${slice.focus}

Files in scope (path prefixes / pathspecs): ${slice.paths.join('  ')}

STEP 1 — map the slice. Enumerate and size the files:
  git ls-files -- ${slice.paths.join(' ')} ${EXCL}
  (and for uncommitted/new files) git status --short -- ${slice.paths.join(' ')}
Then identify the highest-RISK files: the largest, the most security-sensitive (auth, trust boundaries, secrets, SQL, network, unsafe), and the most recently/actively changed. Check recent + uncommitted churn:
  git log --oneline -15 -- ${slice.paths.join(' ')}
  git diff -- ${slice.paths.join(' ')} ${EXCL}        (uncommitted working-tree changes — FRESH code, audit it hard)
You do NOT have budget to read every line of a 400k-LOC repo. Be a smart auditor: prioritize the risk-ranked files, read them in full, and skim the rest.

STEP 2 — read the CURRENT source (and its callers + tests where correctness depends on them). For any suspicious construct, open the actual file at the actual lines and confirm before reporting.

STEP 3 — audit hard against the invariants below, plus: correctness/logic bugs (races, panics on external input, error-swallowing, off-by-one, unchecked arithmetic, resource leaks), security (auth gates on every request path, trust boundaries, secret handling, CORS, SSRF, injection, integrity checks on fetched bytes), dead code, leftover compat shims, naming drift, oversized files, weak/missing tests, and docs that contradict the code.

${GATES}

${INVARIANTS}

Report ONLY real, defensible findings with concrete evidence (quote the code/line). An empty findings array is a valid and GOOD answer if the slice is genuinely clean — do NOT invent issues to fill space. Every finding needs an exact file:line location and quoted evidence. Be specific in the fix. In your summary, say plainly whether the slice is healthy.

Return the structured object: {slice, summary, findings[]}.`
}

// ---- slices ---------------------------------------------------------------
const SLICES = [
  { key: 'engine', paths: ['crates/nimbus-engine/src', 'crates/nimbus-engine/benches'],
    focus: 'Engine = central coordinator. Audit: single mutation path (apply_mutation_with_mode* + queued journal — confirm there is exactly ONE path), applied-visibility wait races (recent commit "close lost-wakeup race in applied-visibility wait" — verify it is correct AND that no sibling wait has the same lost-wakeup bug), OCC/seq-lock ordering (is the optimistic-concurrency check INSIDE the seq lock?), read-after-write visibility (is applied_head bumped only AFTER cache invalidation?), commit-log ordering, scheduler/trigger recovery after crash (are Running triggers re-enqueued?).' },
  { key: 'storage-core', paths: ['crates/nimbus-storage/src', ':(exclude)crates/nimbus-storage/src/**/tests.rs', ':(exclude)crates/nimbus-storage/src/**/tests'],
    focus: 'Storage core (non-provider-test). Audit: atomicity (doc write + index effects + commit-log append = ONE txn), table catalog / stable logical table identity, index identity/lifecycle, read-consistency routing, transaction boundaries, range-scan correctness (can a range/scan leak documents of a DIFFERENT key type on the default redb backend? prior A2-1).' },
  { key: 'storage-providers', paths: ['crates/nimbus-storage/src/**/tests.rs', 'crates/nimbus-storage/src/**/libsql*', 'crates/nimbus-storage/src/**/mysql*', 'crates/nimbus-storage/src/**/postgres*', 'crates/nimbus-storage/src/**/sqlite*'],
    focus: 'Storage SQL providers (libsql/mysql/postgres/sqlite) + their large test files. Audit: SQL-safety (parameterization, NO string-built SQL with external input), per-backend parity, dual-target coverage, and whether the >=2000-line provider test files (libsql_provider 2406, mysql_provider 2286, postgres_provider 2235) are an acceptable ownership exception or genuinely need splitting.' },
  { key: 'runtime', paths: ['crates/nimbus-runtime/src', 'crates/nimbus-runtime/Cargo.toml'],
    focus: 'V8 runtime. Audit: nimbus-runtime ZERO-workspace-deps invariant (READ Cargo.toml — any path = ../ workspace dep is a violation), HostBridge trait surface, bundle SHA-256 integrity before invocation (is it actually enforced on EVERY invoke?), capability segregation / fail-closed service grants, isolate-is-NOT-a-sandbox, bun/jsc FFI watchdog (prior C3-1: does the bun/jsc path drop the execution-timeout watchdog so a runaway has no timeout?).' },
  { key: 'server-transport-auth', paths: ['crates/nimbus-server/src', ':(exclude)crates/nimbus-server/src/service_manager', 'crates/nimbus-server/tests'],
    focus: 'HTTP/WS transport + auth. SECURITY-CRITICAL — be adversarial about auth bypass. Audit: auth gate on EVERY route (find any unauthenticated mutating route), X-Nimbus-Api-Key handling, admin-token rotation gate, configurable CORS (NO wildcard-origin-with-credentials), TLS termination, deploy admin handshake (NIMBUS_DEPLOY_TOKEN Bearer), localhost server hardening, rest.ts parity, the mongodb/firestore/dynamodb listener wiring under adapters/.' },
  { key: 'server-service-manager', paths: ['crates/nimbus-server/src/service_manager'],
    focus: 'Server-side service manager wiring (SRM resource model surface). service_manager/tests.rs is ~1729 lines — judge test quality hard. Audit: correct wiring of Service->ServiceBackend, no adapter ctx shortcuts leaking through, session/sandbox semantics, no banned durable names.' },
  { key: 'bin-dev-start', paths: ['crates/nimbus-bin/src/dev.rs', 'crates/nimbus-bin/src/start.rs', 'crates/nimbus-bin/src/dev', 'crates/nimbus-bin/src/autodetect', 'crates/nimbus-bin/src/serve.rs'],
    focus: 'CLI dev/start/autodetect (adapters always-on with --no-* opt-outs). Audit: correctness of autodetect, opt-out flags actually disable, banner/daemon shape, NO secret leakage in logs, no panics on missing config.' },
  { key: 'bin-machine-node', paths: ['crates/nimbus-bin/src', ':(exclude)crates/nimbus-bin/src/dev.rs', ':(exclude)crates/nimbus-bin/src/dev', ':(exclude)crates/nimbus-bin/src/start.rs', ':(exclude)crates/nimbus-bin/src/autodetect', ':(exclude)crates/nimbus-bin/src/serve.rs'],
    focus: 'CLI machine/node/daemon/compose/backup commands + walk-up boundaries. machine/client.rs is ~1748 lines and HAS uncommitted changes (~214 lines) — audit that fresh code hard. Audit: daemon canonicalization, machine lifecycle correctness, machine-image integrity check on fetch (prior I1-1: is an HTTP-fetched machine image verified by hash/signature before use?), backup command, systemd unit wiring, no hardcoded credentials (prior E1-5 admin/admin).' },
  { key: 'sandbox', paths: ['crates/nimbus-sandbox'],
    focus: 'Sandbox seam. HAS large uncommitted changes (krun/vm/launch.rs DELETED -> start.rs NEW; container + krun bundle/runtime edits) — audit the fresh code hard. Audit: canonical chain SandboxBackend::start(SandboxSpec) is the SINGLE start path; SandboxSpec carries owner: SandboxOwnerSpec + root: SandboxRootSpec; SandboxRootSpec = Rootfs|OciImage; SandboxOciImageSpec.source = Reference|Build (Build policy-gated, fails closed in prod); disk_limit_bytes rejected as InvalidSpec (SBR C3 fail-closed). Verify NONE of the banned names survive. SandboxOwnerSpec is metadata only (no name lookup). Egress proxy wiring correctness.' },
  { key: 'services', paths: ['crates/nimbus-services'],
    focus: 'Service/sandbox/session resource manager (SRM). HAS large uncommitted changes (manager.rs, activation.rs, launch.rs->service_start.rs rename, sandboxes.rs, handles.rs) — audit the fresh code hard. Audit: Service->ServiceBackend(Sandbox|BuiltIn|External) modeling, BuiltInServiceSpec/ExternalServiceSpec declarative payloads, NO adapter ctx shortcuts, runtime isolates are non-resource, session target semantics, futures executor::block_on inside sync trait methods (prior M1: Tokio-worker deadlock/panic risk — check registry/manager).' },
  { key: 'core-bridge-system', paths: ['crates/nimbus-core', 'crates/nimbus-core/Cargo.toml', 'crates/nimbus-bridge', 'crates/nimbus-system', 'crates/nimbus-auth', 'crates/nimbus-license', 'crates/nimbus-artifacts', 'crates/nimbus-assets', 'crates/nimbus/src', 'crates/nimbus-machine/src'],
    focus: 'Core + bridge + system + small crates. Audit: nimbus-core ZERO-I/O invariant (READ Cargo.toml + grep for std::fs/std::net/reqwest/tokio::net — any real I/O is a violation), NumericValue/typed-scalar panics on non-finite f64 from adapter input (prior H3 typed_scalar.rs), bridge HostBridge impl correctness, auth primitives, license/provenance verification (artifacts admission predicate_types handling — prior M3), nimbus-machine live fs/env I/O despite record-type framing (prior M4), facade re-exports.' },
  { key: 'tenant-node-operator', paths: ['crates/nimbus-tenant', 'crates/nimbus-node', 'crates/nimbus-operator'],
    focus: 'Tenant admission + node reconciliation + operator. Audit: tenant policy/workload admission decisions, systemd D-Bus reconciliation (signal-correlated job completion via JobRemoved established BEFORE StartTransientUnit, NOT polling), node workload executor caller wiring, trust boundary between tenant policy and node enforcement, fail-closed admission.' },
  { key: 'adapters-auth-gates', paths: ['crates/nimbus-mongodb', 'crates/nimbus-firebase', 'crates/nimbus-cloud-functions', 'crates/nimbus-convex'],
    focus: 'CRITICAL SECURITY SLICE. Adapter crates mongodb/firebase/cloud-functions/convex. A prior CRITICAL (E1-1/E1-2) found nimbus-mongodb dispatch() had NO auth gate (conn.authenticated never read) and hardcoded PrincipalContext::system() bypassing engine authz. VERIFY the CURRENT state of crates/nimbus-mongodb/src/commands/mod.rs: does dispatch() now ENFORCE authentication (authenticated_principal reading conn.authenticated) and pass a REAL principal (not system())? Is the gate applied to ALL mutating/admin commands, not just some? Audit every adapter for: enforced auth gate on the request path, trust-boundary enforcement, NO ctx.services/sandboxes/sessions shortcuts, secret handling, fail-closed in production.' },
  { key: 'adapter-dynamodb', paths: ['crates/nimbus-dynamodb'],
    focus: 'DynamoDB adapter (query/scan/stream/ttl/control_plane commands, large files). HAS uncommitted changes (item.rs, query.rs, stream.rs, ttl.rs +166) — audit the fresh code hard. Audit: command correctness (query/scan/stream/batch/ttl), expression parsing safety (no injection via condition/filter/projection expressions), auth gate on the request path, parity with the DynamoDB contract, test quality on the changed command test files.' },
  { key: 'js-firebase', paths: ['packages/firebase', 'packages/mongodb', 'packages/dynamodb', 'demos'],
    focus: 'JS drop-in SDKs (firebase, mongodb, dynamodb) + demos. Audit: API parity/correctness vs the real Firebase/Mongo/DynamoDB client surface, NO leaked internal Nimbus APIs, type correctness, demo accuracy, no secrets committed in demo config.' },
  { key: 'js-sdk-ui', paths: ['packages/nimbus', 'packages/convex', 'packages/codegen', 'packages/nimbus-ui/src'],
    focus: 'Nimbus JS SDK + convex compat wrapper + codegen + UI. Audit: packages/nimbus is canonical; packages/convex MUST be a THIN compat wrapper (adapters/aliases/re-exports), NOT copy-forwarded parallel logic — flag duplicated logic as a finding. Codegen correctness; UI lens path-prefix correctness (match full pathname not leaf segment).' },
  { key: 'docs-content', paths: ['docs/concepts', 'docs/developers', 'docs/reference', 'docs/guides', 'docs/operations', 'docs/source-map.md', 'ARCHITECTURE.md', 'DESIGN.md', 'README.md'],
    focus: 'Public docs prose + front-door docs. Audit: accuracy vs CURRENT code (claims that contradict shipped behavior — spot-check API names, flags, types, crate names against the crates), ARCHITECTURE.md Code Map completeness (prior drift: it documented only 9 of 27 crates and had stale nimbus-server/src/convex/ paths — verify the crate list + invariants match the 27 actual crates), broken internal links, stale SBR/SRM naming. Do NOT touch docs/private (untracked, local-only).' },
  { key: 'website-brand', paths: ['website', 'packages/nimbus-ui/public', 'docs/brand'],
    focus: 'Marketing website + brand assets. Audit: landing messaging matches canon ("your cloud, one binary"; leads with `nimbus dev`), no overclaiming vs shipped features, theme-aware favicon/brand assets present and referenced, wrangler/deploy config sanity, no secrets/tokens in committed config. Never self-describe docs as "honest".' },
  { key: 'build-ci-infra', paths: ['Makefile', 'Cargo.toml', '.github', 'scripts', 'packaging', 'deny.toml', 'package.json', 'AGENTS.md', 'NOTICE', '.gitignore'],
    focus: 'Build graph + CI + packaging + front-door config. Audit: Makefile UI dependency graph correctness, CI workflow correctness (SHA-pinned non-actions/* uses, runs-on pinned to ubuntu-24.04, sccache/cache keys, no secret leakage in logs), packaging (systemd unit, apt channel, install script safety — no curl|sh of unverified content), deny.toml + NOTICE attribution completeness, AGPL exclusions (Garage/MinIO must NOT be vendored/linked). AGENTS.md has large uncommitted edits (~297 lines) — sanity-check it is coherent and routing references resolve.' },
  { key: 'tests-quality', paths: ['crates/nimbus-server/src/service_manager/tests.rs', 'crates/nimbus-dynamodb/src/commands/query/tests.rs', 'crates/nimbus-dynamodb/src/commands/stream/tests.rs', 'crates/nimbus-sandbox/tests', 'crates/nimbus-sandbox/src/backends/krun/vm/tests.rs', 'crates/nimbus-services/src/manager/tests', 'crates/nimbus-engine/src/engine/execution_units/tests.rs'],
    focus: 'CROSS-CUT TEST-QUALITY LENS. Sample the largest / freshly-changed test files and judge ONLY test quality: does each test assert a specific behavioral outcome (happy + edge + error), or does it merely check "does not panic"/"constructs"/"compiles"? Flag assertion-free or tautological tests, over-mocked tests that prove nothing, and missing error-path coverage. Defer correctness/arch findings to the owning slice.' },
  { key: 'regression-prior-findings', paths: ['crates/nimbus-mongodb', 'crates/nimbus-engine/src', 'crates/nimbus-storage/src', 'crates/nimbus-runtime/src', 'crates/nimbus-services/src', 'crates/nimbus-core', 'crates/nimbus-bin/src', 'crates/nimbus-artifacts'],
    focus: `REGRESSION-VERIFICATION SLICE. The prior 2026-06-09 full review found these critical/high/notable issues. For EACH, open the current code and determine: FIXED (no finding — note in summary), STILL-OPEN (emit a finding at the listed severity), or REGRESSED. Be rigorous; do not assume fixed.
- E1-1/E1-2 (CRITICAL security): nimbus-mongodb dispatch() routed CRUD/admin with NO auth gate (conn.authenticated never read) + hardcoded PrincipalContext::system() bypassing engine authz. Check crates/nimbus-mongodb/src/commands/mod.rs — is authentication now enforced (authenticated_principal reading conn.authenticated) on ALL mutating/admin commands AND a real principal passed (not system())?
- A2-1 (HIGH correctness): range scan cross-type leak on the default redb backend — a range/scan returns documents of a different key type. Check storage range-scan/index code.
- B1-1 (HIGH correctness): stale read-after-write — applied_head bumped BEFORE cache invalidation. Check engine applied-visibility / materialized-read path.
- B1-2 (HIGH correctness): OCC check performed OUTSIDE the seq lock (TOCTOU). Check engine mutation commit path.
- B3-1 (HIGH correctness): lost wakeup in begin_delete_blocking. Check engine tenant delete + Notify usage. (A recent commit closed a lost-wakeup race in applied-visibility wait — verify THIS one too.)
- B4-1 (HIGH correctness): Running triggers not re-enqueued after crash. Check scheduler/trigger recovery.
- C3-1 (HIGH correctness): bun/jsc FFI drops the watchdog so there is no execution timeout. Check runtime bun_jsc backend.
- I1-1 (HIGH security): HTTP-fetched machine image has NO integrity check. Check machine-image fetch path (nimbus-bin/nimbus-machine).
- E1-5 (HIGH security): hard-coded admin/admin credential. Check for default credentials.
- H3 (MEDIUM correctness): NumericValue::Double panics on non-finite f64 from adapter input (typed_scalar.rs ~194,207). Check.
- M1 (MEDIUM correctness): executor block_on in sync trait methods in nimbus-services (registry/manager). Check.
- M3 (MEDIUM security): nimbus-artifacts admission provenance builder/source match skipped when predicate_types non-empty. Check.
Emit a finding ONLY for issues that are STILL-OPEN or REGRESSED, with current file:line evidence. Summarize the fixed ones.` },
]

// ---- run ------------------------------------------------------------------
log(`Fresh whole-repo audit across ${SLICES.length} slices (current working-tree state; gates already green).`)

const auditResults = await pipeline(
  SLICES,
  // stage 1: find
  (slice) => agent(auditPrompt(slice), { label: `audit:${slice.key}`, phase: 'Audit', schema: FINDINGS_SCHEMA, agentType: 'general-purpose' }),
  // stage 2: adversarially verify each non-trivial finding (per-slice, as soon as it lands)
  (res, slice) => {
    if (!res || !res.findings || res.findings.length === 0) return res
    const toVerify = res.findings.filter(f => ['critical', 'high', 'medium'].includes(f.severity))
    if (toVerify.length === 0) return res
    return parallel(toVerify.map(f => () =>
      agent(
        `Adversarially verify this audit finding from slice "${slice.key}". Default to REFUTED if you cannot prove it from the actual code — we want zero false positives in the final report.

Finding: ${f.title}
Severity claimed: ${f.severity}
Category: ${f.category}
Location: ${f.location}
Evidence claimed: ${f.evidence}
Impact claimed: ${f.impact}

Open the cited file(s) at the cited lines (read the working tree; use git show ${HEAD}:<path> for committed state, git diff -- <path> to see uncommitted changes). Confirm the code actually says what the evidence claims AND that it is genuinely a problem given the pre-launch posture (breaking changes preferred; missing-backcompat is NOT a bug; leftover compat shims ARE; gates are already green so no fmt/clippy/deny findings). Re-rate severity.`,
        { label: `verify:${slice.key}:${f.severity}`, phase: 'Verify', schema: VERDICT_SCHEMA, agentType: 'general-purpose' }
      ).then(v => ({ ...f, slice: slice.key, verdict: v }))
    )).then(verified => {
      const byLoc = new Map(verified.filter(Boolean).map(v => [v.location + '|' + v.title, v]))
      return {
        ...res,
        findings: res.findings.map(f => {
          const v = byLoc.get(f.location + '|' + f.title)
          return v ? v : { ...f, slice: slice.key, verdict: { verdict: 'unverified', reasoning: 'low/info — not adversarially verified', corrected_severity: f.severity } }
        }),
      }
    })
  }
)

// flatten
const slices = auditResults.filter(Boolean)
const allFindings = slices.flatMap(s => (s.findings || []).map(f => ({ ...f, slice: f.slice || s.slice })))
const confirmed = allFindings.filter(f => {
  const v = f.verdict?.verdict
  return v === 'confirmed' || v === 'unverified' // unverified = low/info, kept but flagged
})
const refuted = allFindings.filter(f => f.verdict?.verdict === 'refuted')
const uncertain = allFindings.filter(f => f.verdict?.verdict === 'uncertain')

log(`Raw findings: ${allFindings.length}. Confirmed/kept: ${confirmed.length}. Refuted: ${refuted.length}. Uncertain: ${uncertain.length}.`)

// ---- synthesize -----------------------------------------------------------
phase('Synthesize')
const SYNTH_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['overall', 'ranked', 'by_severity', 'by_slice_health'],
  properties: {
    overall: { type: 'string', description: 'Executive read on overall codebase health.' },
    by_severity: {
      type: 'object', additionalProperties: false,
      required: ['critical', 'high', 'medium', 'low', 'info'],
      properties: {
        critical: { type: 'integer' }, high: { type: 'integer' }, medium: { type: 'integer' },
        low: { type: 'integer' }, info: { type: 'integer' },
      },
    },
    by_slice_health: {
      type: 'array', description: 'Per-slice RED/YELLOW/GREEN health read.',
      items: {
        type: 'object', additionalProperties: false,
        required: ['slice', 'health', 'note'],
        properties: { slice: { type: 'string' }, health: { enum: ['RED', 'YELLOW', 'GREEN'] }, note: { type: 'string' } },
      },
    },
    ranked: {
      type: 'array',
      description: 'Deduped, merged, severity-ranked findings (highest first).',
      items: {
        type: 'object', additionalProperties: false,
        required: ['severity', 'category', 'slice', 'title', 'location', 'evidence', 'impact', 'fix'],
        properties: {
          severity: { enum: ['critical', 'high', 'medium', 'low', 'info'] },
          category: { type: 'string' },
          slice: { type: 'string' },
          title: { type: 'string' },
          location: { type: 'string' },
          evidence: { type: 'string' },
          impact: { type: 'string' },
          fix: { type: 'string' },
        },
      },
    },
  },
}

const synthesis = await agent(
  `You are the synthesis lead for an ENTIRE FRESH whole-repo audit. Below are the per-slice findings that survived adversarial verification (verdict confirmed) plus low/info findings (verdict unverified). Refuted findings are excluded already.

Produce the final report object:
1. Dedupe and merge findings that describe the same root issue across slices (same file/concern). Keep the strongest evidence.
2. Use the verdict's corrected_severity when present (it overrides the original severity). Drop any whose corrected severity is 'none'.
3. Rank highest-severity first.
4. Count by severity.
5. Give each audited slice a RED/YELLOW/GREEN health read (GREEN = no real issues, YELLOW = medium/low only, RED = critical/high present).
6. Write a tight executive "overall" read of codebase health.

Do not introduce new findings here — only consolidate what is given.

CONFIRMED/KEPT FINDINGS (JSON):
${JSON.stringify(confirmed.map(f => ({ severity: f.verdict?.corrected_severity && f.verdict.corrected_severity !== 'none' ? f.verdict.corrected_severity : f.severity, category: f.category, slice: f.slice, title: f.title, location: f.location, evidence: f.evidence, impact: f.impact, fix: f.fix, verdict: f.verdict?.verdict, verify_note: f.verdict?.reasoning })), null, 1)}`,
  { label: 'synthesize', phase: 'Synthesize', schema: SYNTH_SCHEMA, agentType: 'general-purpose' }
)

// ---- completeness critic --------------------------------------------------
phase('Critic')
const CRITIC_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['gaps', 'extra_findings'],
  properties: {
    gaps: { type: 'array', items: { type: 'string' }, description: 'Areas/dimensions/files in scope that were NOT adequately audited.' },
    extra_findings: {
      type: 'array',
      description: 'Concrete additional findings the critic discovered while checking coverage.',
      items: {
        type: 'object', additionalProperties: false,
        required: ['severity', 'category', 'title', 'location', 'evidence', 'impact', 'fix'],
        properties: {
          severity: { enum: ['critical', 'high', 'medium', 'low', 'info'] },
          category: { type: 'string' },
          title: { type: 'string' }, location: { type: 'string' },
          evidence: { type: 'string' }, impact: { type: 'string' }, fix: { type: 'string' },
        },
      },
    },
  },
}

const critic = await agent(
  `You are the completeness critic for an ENTIRE FRESH whole-repo audit. The audit ran ${SLICES.length} slices: ${SLICES.map(s => s.key).join(', ')}.

The synthesized report found ${synthesis.ranked?.length || 0} issues (by severity: ${JSON.stringify(synthesis.by_severity)}).

Your job: find what the audit MISSED. Specifically check:
- Cross-cutting concerns no single slice owned (e.g. a rename that should ripple across crates + docs + JS SDK but only landed in one; an invariant that needs checking across ALL adapters at once).
- The mongodb auth question: is it DEFINITIVELY resolved (fixed vs open) with current file:line evidence? If ambiguous, that's a gap.
- Any in-scope crate with zero findings that seems too clean — spot-check it (git ls-files + read the riskiest file).
- Migration/shim leftovers: grep the working tree for the banned names (ServiceImplementation, SandboxBackedServiceImplementation, SandboxImageLaunchSpec, SandboxBuildLaunchSpec, start_from_image, start_from_build, tenant_workload_identity_) — confirm 0 in crates/ production code.
- Architecture-invariant spot checks done directly: grep nimbus-core + nimbus-runtime Cargo.toml + source for I/O and workspace deps; confirm the single mutation path; confirm bundle SHA-256 enforcement.
- Doc/code drift between ARCHITECTURE.md's stated crate list/invariants and the actual 27 crates.

Run real commands (git ls-files, git diff, grep, read). Return gaps (dimensions/files not adequately covered) and any concrete extra_findings you can prove with current-code evidence.`,
  { label: 'completeness-critic', phase: 'Critic', schema: CRITIC_SCHEMA, agentType: 'general-purpose' }
)

return {
  mode: 'fresh-whole-repo',
  head: HEAD,
  slices_audited: slices.length,
  raw_finding_count: allFindings.length,
  refuted_count: refuted.length,
  uncertain: uncertain.map(f => ({ slice: f.slice, title: f.title, location: f.location })),
  synthesis,
  critic,
}

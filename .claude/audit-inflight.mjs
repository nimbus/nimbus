export const meta = {
  name: 'audit-inflight',
  description: 'Focused review of the uncommitted in-flight working-tree refactor that the whole-repo audit under-reviewed (krun vm start-path split, container backend, services-manager rename + LocalBuildAdmission, adapter changes, machine client)',
  phases: [
    { title: 'Review' },
    { title: 'Verify' },
    { title: 'Synthesize' },
  ],
}

const INVARIANTS = `Architecture invariants (violations are findings):
- nimbus-core ZERO I/O; nimbus-runtime ZERO workspace deps.
- SINGLE mutation path (engine apply_mutation_with_mode* + queued journal). No second path.
- Storage atomicity: doc write + index effects + commit-log append = ONE txn.
- Runtime bundles SHA-256 integrity-checked before invocation; host ops via the Service/Engine path (no bypass).
- Schema optional. Adapters MUST NOT expose ctx.services/sandboxes/sessions shortcuts. Isolates are NOT sandboxes.

Pre-launch posture: breaking renames PREFERRED over compat shims; missing-backcompat is NOT a finding; a LEFTOVER compat shim/alias that should have been deleted IS (compat-shim). Banned durable names (must be 0 in prod code): ServiceImplementation, SandboxBackedServiceImplementation, SandboxImageLaunchSpec, SandboxBuildLaunchSpec, SandboxImageProcessOverrides, start_from_image, start_from_build, tenant_workload_identity_*.
Naming: Backend=executor seam, Spec=declarative payload, Kind=discriminant; no Launch/Backing in durable types; concept-owned filenames.

Gates already GREEN (fmt/clippy/deny pass) — do NOT report formatting/lint/dependency issues. A focused fact: the refactor compiles (cargo check green). Your job is BEHAVIOR/correctness/security review of the change itself: did the refactor PRESERVE behavior, is the new code correct, are new security-relevant types fail-closed, did the rename leave drift?`

const FINDINGS_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['cluster', 'summary', 'behavior_preserved', 'findings'],
  properties: {
    cluster: { type: 'string' },
    summary: { type: 'string' },
    behavior_preserved: { enum: ['yes', 'no', 'unclear'], description: 'Did the refactor preserve prior behavior where it should have?' },
    findings: {
      type: 'array',
      items: {
        type: 'object', additionalProperties: false,
        required: ['severity', 'category', 'title', 'location', 'evidence', 'impact', 'fix'],
        properties: {
          severity: { enum: ['critical', 'high', 'medium', 'low', 'info'] },
          category: { enum: ['correctness', 'security', 'architecture-invariant', 'test-quality', 'naming', 'modularity', 'docs-accuracy', 'dead-code', 'compat-shim', 'behavior-drift'] },
          title: { type: 'string' }, location: { type: 'string' },
          evidence: { type: 'string' }, impact: { type: 'string' }, fix: { type: 'string' },
        },
      },
    },
  },
}

const VERDICT_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['verdict', 'reasoning', 'corrected_severity'],
  properties: {
    verdict: { enum: ['confirmed', 'refuted', 'uncertain'] },
    reasoning: { type: 'string' },
    corrected_severity: { enum: ['critical', 'high', 'medium', 'low', 'info', 'none'] },
  },
}

const CLUSTERS = [
  { key: 'krun-vm-start-split', paths: ['crates/nimbus-sandbox/src/backends/krun'],
    focus: 'The krun VM start-path refactor: vm/launch.rs (621 lines) was DELETED and replaced by a NEW vm/start.rs, with edits to vm.rs, bundle.rs, mod.rs, vm/lifecycle.rs, vm/readiness.rs. This is a behavior-critical refactor of the microVM start path. Read the FULL new start.rs and diff it conceptually against the deleted launch.rs. Audit: did the split preserve the exact start/boot/readiness sequence? Any dropped error handling, changed ordering, lost timeout/watchdog, or altered TSI/network wiring? Is the canonical SandboxBackend::start(SandboxSpec) chain intact?' },
  { key: 'container-backend', paths: ['crates/nimbus-sandbox/src/backends/container', 'crates/nimbus-sandbox/src/egress_proxy.rs'],
    focus: 'Container backend changes (bundle.rs, runtime.rs, runtime/planning.rs, runtime/support.rs, mod.rs). Audit the diff: correct rootfs/oci bundle construction, egress-proxy wiring (note: a separate audit found egress_proxy.rs truncates plain-HTTP request bodies — confirm whether these changes touch or fix that), no banned names, build-vs-reference image source policy still fails closed in prod.' },
  { key: 'services-manager-localbuild', paths: ['crates/nimbus-services/src', 'crates/nimbus/src/lib.rs', 'crates/nimbus-server/src/lib.rs'],
    focus: 'The nimbus-services manager refactor: manager/launch.rs was RENAMED to manager/service_start.rs, with edits to manager.rs, activation.rs, definitions.rs, handles.rs, sandboxes.rs, and a BRAND-NEW public security-relevant type LocalBuildAdmission rippled through nimbus/src/lib.rs + nimbus-server/src/lib.rs re-exports. Audit: is LocalBuildAdmission fail-closed by DEFAULT (Denied unless explicitly Allowed)? Is the admission actually ENFORCED on the build path, not just defined? Did the launch->service_start rename leave any Launch/banned-name drift? Service->ServiceBackend modeling intact? No ctx.services/sandboxes shortcuts? block_on in sync trait methods?' },
  { key: 'adapter-changes', paths: ['crates/nimbus-mongodb/src/lib.rs', 'crates/nimbus-server/src/adapters/mongodb/listener.rs', 'crates/nimbus-dynamodb/src/commands/item.rs', 'crates/nimbus-dynamodb/src/commands/query.rs', 'crates/nimbus-dynamodb/src/commands/stream.rs', 'crates/nimbus-dynamodb/src/commands/ttl.rs'],
    focus: 'Adapter diffs. MongoDB lib.rs (+14, a new AuthConfig doc comment about one-credential-reaches-every-tenant) and adapters/mongodb/listener.rs (+9, loopback guard). DynamoDB item.rs (-55), query.rs, stream.rs (+/-55), ttl.rs (+166). Audit: correctness of the dynamodb command changes (especially ttl.rs +166 — TTL expiry semantics), no auth/tenant regression, no injection via expressions, the mongodb loopback guard is load-bearing and correct.' },
  { key: 'bin-machine-compose', paths: ['crates/nimbus-bin/src/machine/client.rs', 'crates/nimbus-bin/src/compose'],
    focus: 'CLI machine/client.rs (+214 lines, the largest bin change) and compose execution.rs/tests. Audit the diff: machine lifecycle/API-forwarding correctness, machine-image integrity verification on fetch (prior I1-1 — is a fetched image hash/signature checked?), no secret leakage in logs, compose execution correctness, no panics on missing config.' },
]

function reviewPrompt(c) {
  return `You are reviewing ONE cluster of an uncommitted in-flight refactor in the Nimbus repo. Your cluster: "${c.key}".

${c.focus}

This is UNCOMMITTED working-tree change. See exactly what changed, then judge the RESULT:
  git status --short -- ${c.paths.join(' ')}
  git diff -- ${c.paths.join(' ')}          (the uncommitted change — read it fully)
  git diff --stat -- ${c.paths.join(' ')}
For deleted+added file pairs (e.g. launch.rs deleted, start.rs added), read BOTH the deleted content (git show HEAD:<old path>) and the new file in full, and compare behavior.
Then read the CURRENT state of the changed files and their callers/tests where correctness depends on them.

${INVARIANTS}

Report ONLY real, defensible findings with exact file:line + quoted evidence. An empty findings array is a good answer if the refactor is clean. Judge behavior_preserved honestly. Be specific in fixes.

Return {cluster, summary, behavior_preserved, findings[]}.`
}

log(`Focused in-flight refactor review across ${CLUSTERS.length} clusters.`)

const results = await pipeline(
  CLUSTERS,
  (c) => agent(reviewPrompt(c), { label: `review:${c.key}`, phase: 'Review', schema: FINDINGS_SCHEMA, agentType: 'general-purpose' }),
  (res, c) => {
    if (!res || !res.findings || !res.findings.length) return res
    const toVerify = res.findings.filter(f => ['critical', 'high', 'medium'].includes(f.severity))
    if (!toVerify.length) return res
    return parallel(toVerify.map(f => () =>
      agent(
        `Adversarially verify this in-flight-refactor finding from cluster "${c.key}". Default to REFUTED if you cannot prove it from the actual working-tree code — zero false positives wanted.

Finding: ${f.title}
Severity: ${f.severity} | Category: ${f.category}
Location: ${f.location}
Evidence: ${f.evidence}
Impact: ${f.impact}

Open the cited files (git diff -- <path> for the change, git show HEAD:<path> for prior state, read working tree for current). Confirm the code says what is claimed AND it is a genuine problem given pre-launch posture (missing-backcompat NOT a bug; leftover shim IS; gates already green). Re-rate severity.`,
        { label: `verify:${c.key}`, phase: 'Verify', schema: VERDICT_SCHEMA, agentType: 'general-purpose' }
      ).then(v => ({ ...f, cluster: c.key, verdict: v }))
    )).then(verified => {
      const byLoc = new Map(verified.filter(Boolean).map(v => [v.location + '|' + v.title, v]))
      return { ...res, findings: res.findings.map(f => byLoc.get(f.location + '|' + f.title) || ({ ...f, cluster: c.key, verdict: { verdict: 'unverified', reasoning: 'low/info', corrected_severity: f.severity } })) }
    })
  }
)

const clusters = results.filter(Boolean)
const all = clusters.flatMap(c => (c.findings || []).map(f => ({ ...f, cluster: f.cluster || c.cluster })))
const kept = all.filter(f => { const v = f.verdict?.verdict; return v === 'confirmed' || v === 'unverified' })
const refuted = all.filter(f => f.verdict?.verdict === 'refuted')

log(`In-flight review: ${all.length} raw, ${kept.length} kept, ${refuted.length} refuted.`)

phase('Synthesize')
const SYNTH_SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['overall', 'behavior_verdict', 'ranked'],
  properties: {
    overall: { type: 'string' },
    behavior_verdict: { type: 'string', description: 'Did the refactor as a whole preserve behavior? Call out any cluster that did not.' },
    ranked: {
      type: 'array',
      items: {
        type: 'object', additionalProperties: false,
        required: ['severity', 'category', 'cluster', 'title', 'location', 'evidence', 'impact', 'fix'],
        properties: {
          severity: { enum: ['critical', 'high', 'medium', 'low', 'info'] },
          category: { type: 'string' }, cluster: { type: 'string' },
          title: { type: 'string' }, location: { type: 'string' },
          evidence: { type: 'string' }, impact: { type: 'string' }, fix: { type: 'string' },
        },
      },
    },
  },
}

const synthesis = await agent(
  `Synthesis lead for the focused in-flight refactor review. Below are the kept findings (refuted already excluded) plus each cluster's behavior_preserved verdict. Dedupe/merge same-root issues, apply each verdict's corrected_severity (drop 'none'), rank highest-severity first, and give an overall + behavior_verdict.

CLUSTER BEHAVIOR VERDICTS: ${JSON.stringify(clusters.map(c => ({ cluster: c.cluster, behavior_preserved: c.behavior_preserved, summary: c.summary })), null, 1)}

KEPT FINDINGS: ${JSON.stringify(kept.map(f => ({ severity: f.verdict?.corrected_severity && f.verdict.corrected_severity !== 'none' ? f.verdict.corrected_severity : f.severity, category: f.category, cluster: f.cluster, title: f.title, location: f.location, evidence: f.evidence, impact: f.impact, fix: f.fix, verdict: f.verdict?.verdict })), null, 1)}`,
  { label: 'synthesize-inflight', phase: 'Synthesize', schema: SYNTH_SCHEMA, agentType: 'general-purpose' }
)

return { mode: 'in-flight-refactor', clusters_reviewed: clusters.length, raw: all.length, refuted: refuted.length, synthesis }

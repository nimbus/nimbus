# Research: Agent Browser Service Prior Art

North-star research and prior-art survey for adding a first-class
**browser session** resource to Nimbus that agent workloads can use
ergonomically. Pairs with the deferred execution plan at
`docs/plans/agent-browser-service-plan.md`.

This document captures durable findings only. It is not progress state and
does not own execution sequencing.

---

## Why This Question Exists For Nimbus

Nimbus already runs untrusted user code inside `nimbus-runtime` (V8 today,
wasmtime in flight) behind the `nimbus-sandbox` boundary. Agent workloads
increasingly need a real, stateful browser — to log into a vendor portal,
extract a structured page, fill a multi-step form, or screenshot a
rendered chart for an LLM. The current options inside a Nimbus function
are:

1. `fetch()` to the target URL. Works for plain HTML/JSON; fails on
   anything JS-rendered, anything behind login, anything that needs
   click/scroll, anything that fingerprints non-browser clients.
2. Ad-hoc Chrome subprocess spawned by user code. Bypasses the sandbox
   and the engine's audit/journal path. Multiple agents step on each
   other's cookies. Cold-start cost is paid every call.
3. External managed service (Browserbase, Browserless, Cloudflare Browser
   Rendering, Steel). Works today, costs money per session-minute, and
   sits outside Nimbus's trust and audit boundary.

The architectural question is whether a **BrowserService** belongs inside
Nimbus alongside `nimbus-storage`, `nimbus-runtime`, and `nimbus-sandbox`
— and if so, what its boundaries look like.

This is research only. Activation depends on agent-capability product
direction landing first (see relationship section).

---

## Architectural Primitives In The Space

Every system in this space composes the same few primitives differently.
Naming them once here lets the rest of the survey stay short.

### Wire protocols

| Protocol | Origin | What it is | Where it shows up |
|---|---|---|---|
| **CDP** (Chrome DevTools Protocol) | Chromium | JSON-over-WebSocket control plane for Chromium-family browsers. Targets, sessions, domains (`Page`, `DOM`, `Network`, `Runtime`, `Input`, `Accessibility`). | Direct in tools like `chrome-devtools-mcp`; underneath Puppeteer; underneath Playwright's Chromium driver. |
| **WebDriver BiDi** | W3C | Cross-browser bidirectional automation protocol; supersedes classic WebDriver for modern automation. | Playwright's Firefox driver, recent WebKit, increasingly Chromium too. |
| **WebDriver (classic)** | W3C | Synchronous HTTP-over-JSON automation. Older, less expressive. | Selenium; long tail of legacy automation. |

### Library abstractions

| Library | What it gives you | What it hides |
|---|---|---|
| **Puppeteer** (Node, Python via `pyppeteer`) | High-level Chromium control via CDP. | Single-vendor; CDP version coupling. |
| **Playwright** (Node, Python, .NET, Java) | High-level cross-browser control via CDP + BiDi. Owns the canonical `BrowserContext` abstraction adopted by every service since. | Browser binary download + lifecycle; protocol differences. |
| **Selenium** | WebDriver-classic across browsers and languages. | Synchronous WebDriver model. |
| **chromedp** (Go) | Direct CDP client in Go. | Higher-level ergonomics. |

### The `BrowserContext` abstraction

The single most important concept Playwright/Puppeteer popularised:

- A **browser process** has cookies, cache, GPU, network stack.
- A **`BrowserContext`** inside the process has its own cookies, local
  storage, IndexedDB, service workers, permissions, and origin
  trust state. Two contexts in the same browser cannot see each other's
  cookies.
- A **page** (tab) belongs to one context. Multiple pages in a context
  share storage.
- Creating a context is cheap (milliseconds). Creating a browser process
  is expensive (hundreds of ms to a few seconds).

CDP exposes this as `Target.createBrowserContext` and
`Target.disposeBrowserContext`. Playwright's
`browser.newContext({ storageState, viewport, userAgent, proxy, ... })`
is the canonical ergonomic wrapper.

**Every serious browser-as-a-service uses BrowserContexts as the unit of
session isolation, not whole browser processes.**

### Site isolation and renderer processes

Chrome itself spawns one renderer subprocess per origin (or per
site-instance under site isolation). This is invisible to CDP clients
and unrelated to BrowserContexts. The browser-as-a-service layer does
not manage renderer processes; Chrome does.

### Profile vs context vs storage state

Confusing terms used inconsistently across the industry:

- **Profile** (`--user-data-dir` in Chrome): on-disk directory holding
  cookies, history, extensions, preferences, etc. Tied to a browser
  process invocation. Persistent.
- **Storage state**: Playwright's serialised cookie + localStorage blob
  (`storageState.json`). Portable across contexts and browsers.
- **BrowserContext**: in-memory isolation unit. Ephemeral by default;
  durable if hydrated from storage state at create time and snapshotted
  on close.

The Nimbus "profile" concept in this research means *the durable storage
state bound to a named session*, not Chrome's `--user-data-dir`.

---

## Survey Of Prior Art

Coverage as of this document's authoring (mid-2026). All facts here are
load-bearing claims about how these systems work; verify before basing
implementation decisions on any single line.

### Browserless (v2)

- **Shape:** Open source (commercial hosted offering). Docker image runs
  one Chrome plus a Node service that fronts it.
- **Protocol surface:** Speaks both CDP (WebSocket proxy) and Playwright
  Server protocol, plus HTTP convenience endpoints (`/screenshot`,
  `/pdf`, `/scrape`).
- **Resource model:** One Chrome process per container; many
  BrowserContexts per Chrome. A "session" maps to a context.
- **Pool model:** Each container is one slot; horizontal scale by
  container count behind a load balancer. Their "hive" mode adds
  session-affinity routing so reconnects land on the right container.
- **Sandbox:** Docker container. No microVM.
- **Lessons for Nimbus:** Proves single-Chrome-many-contexts at
  production load. Demonstrates that CDP WebSocket proxying is a
  workable boundary even when the orchestrator is in a different
  process.

### Browserbase

- **Shape:** Commercial managed service. Closed source. Pairs with
  open-source **Stagehand** SDK that adds LLM-friendly DOM actions on
  top of Playwright.
- **Protocol surface:** Playwright-compatible. Sessions accessed by URL;
  CDP connect string published per session.
- **Resource model:** Stateful sessions are the headline feature.
  Cookies, localStorage, downloaded files, and the live tab tree
  persist across reconnects and across SDK calls. Sessions can run for
  hours.
- **Sandbox:** Kubernetes pods per session (their public talks describe
  this). Hardware isolation at the K8s layer; Chrome's own sandbox
  inside.
- **Extras:** Built-in residential/datacenter proxy routing, CAPTCHA
  handling, session video replay, fingerprint randomisation per session,
  file download capture.
- **Lessons for Nimbus:** This is the closest analog to what a Nimbus
  BrowserService would look like from the agent's perspective. Stateful
  named sessions are the abstraction that matters. The "session video
  replay" feature is interesting for audit parity with Nimbus's
  mutation journal.

### Cloudflare Browser Rendering

- **Shape:** Workers binding (`env.BROWSER`). Closed source; runs on
  Cloudflare's edge.
- **Protocol surface:** Puppeteer-compatible (`@cloudflare/puppeteer`).
- **Resource model:** Pre-warmed Chrome pool. A Worker calls
  `puppeteer.launch(env.BROWSER)` and gets a pre-warmed instance back
  in tens of milliseconds. Sessions are short by default but can be
  kept alive via session IDs.
- **Sandbox:** Their multi-tenant Workers isolate model plus whatever
  pod/VM tier holds the Chrome fleet. Not publicly documented in
  detail.
- **Lessons for Nimbus:** Demonstrates a runtime binding (`env.BROWSER`)
  rather than a separate API as the right ergonomic shape when the
  runtime owner already trusts the browser layer. Maps cleanly to a
  Nimbus `ctx.browser` host-bridge op.

#### Containers retrospective (2026)

Cloudflare's later post `https://blog.cloudflare.com/browser-run-containers/`
documents the v2 architecture as a failure-mode retrospective on the
original BISO-shared infrastructure. Findings worth borrowing:

| Cloudflare finding | What it tells Nimbus |
|---|---|
| "Once we assign a browser to a user, it's exclusively theirs. Browsers are not shared resources." | The right isolation unit is the **trust boundary**, not a fixed level of the browser hierarchy. For Cloudflare that boundary is the user; for Nimbus it is the tenant. One Chrome per trust unit; many BrowserContexts within. |
| Abandoned Workers KV for session-claim state because the "eventual consistency of around 30 seconds was becoming a bottleneck on our critical request path." | Hot-path session claiming must be strongly consistent. Nimbus's engine mutation path is already serialized — no KV equivalent is needed. |
| Moved authoritative claim and live-coordinator state into Durable Objects, "a DO-container pair closest to the user within that region." | DOs solve three Cloudflare-specific problems: strongly-consistent claim writes, per-instance live coordinator process, and affinity routing across thousands of edge locations. A single-process Nimbus instance has none of those problems; a `SessionRegistry` + `SessionHandle` in `BrowserService` plus normal engine mutations replicate all three guarantees. |
| Added direct HTTP "Quick Actions" path: "we send all parameters in a single HTTP request directly to the container, and the entire flow executes internally." | Round-trips dominate latency in any agent-browser stack. The host-bridge surface must expose composite ops (`snapshot`, `screenshot`, `run(steps[])`) alongside primitives. One host-bridge crossing for canonical flows, not N. |
| Regional pre-warmed DO-container pools to keep RTT low between DO and container. | Nimbus is single-process; loopback or vsock. Warm pools still matter to hide Chrome cold-start, but the geography lesson does not transfer. |
| D1 batched writes (100 rows, 1s window) scaled session-state updates "in orders of magnitude" from 1k/sec single-row to 5k+ containers sustained. | Browser-op journal entries should coalesce in transit. The existing engine mutation batching is the natural place for this; no new batching layer needed. |
| Soft limits: 60 browser spinups/minute and 120 concurrent browsers per Worker binding. | Edge-multiplexing budgets specific to Cloudflare; not directly transferable. Use as a reminder that per-tenant quotas (`BrowserQuota`) must exist from day one. |
| "Browsers are not shared resources" combined with renderer-per-origin site isolation means a Chrome 0-day is contained at the user boundary. | Confirms the "one Chrome per trust unit" rule: a Chrome 0-day breaks past BrowserContext but not past the sandbox VM. Cross-tenant sharing is unsafe regardless of context isolation. |

### Steel.dev

- **Shape:** Open source. Single-binary service that supervises a
  Chrome and exposes CDP plus a REST API.
- **Protocol surface:** CDP, Playwright, Selenium, plus REST.
- **Resource model:** Session-first. Each session is a BrowserContext.
  Sessions can be created, listed, and resumed by ID. Optional
  `proxy_url` and `solve_captcha` per session.
- **Sandbox:** Docker container.
- **Lessons for Nimbus:** The clearest open-source reference for the
  shape Nimbus would build. Session lifecycle, named sessions,
  cookie persistence between sessions, and the multi-protocol
  surface are all directly applicable patterns.

### Anchor Browser, Hyperbrowser, others

- A small fleet of mid-2024–2025 startups all converging on the same
  shape: stateful sessions, Playwright/CDP compatibility, hosted
  proxy + CAPTCHA + fingerprint, per-session-minute billing.
- Differentiation is mostly anti-detection sophistication, proxy
  inventory, and SDK ergonomics — not architecture.
- **Lessons for Nimbus:** The market has converged on "stateful
  Playwright-compatible sessions" as the user-facing primitive.
  Whatever Nimbus exposes should be at least implementable in those
  terms.

### Anthropic Computer Use

- **Shape:** Reference Docker image. The model controls a virtual
  desktop, not a browser. X11 server + Firefox + screenshot capture +
  `xdotool` for mouse/keyboard.
- **Resource model:** Whole desktop session. Browser is just one app
  inside it.
- **Lessons for Nimbus:** Computer Use is a different abstraction layer
  — pixels + keystrokes rather than a structured DOM. Slower per step,
  more general (works for non-browser apps). A Nimbus BrowserService
  would sit *below* a Computer-Use-style agent, not replace it.

### OpenAI Operator

- **Shape:** Hosted product, browser-in-VM. Visible headed Chrome
  controlled by their model.
- **Resource model:** One session per user task. Stateful within a task.
- **Sandbox:** Cloud VM per session, by their public description.
- **Lessons for Nimbus:** Confirms VM-per-session is acceptable when
  sessions are long-running and few. Not the right shape for many short
  agent calls.

### Browser Use (Python library)

- **Shape:** Open source. Wraps Playwright with an LLM-friendly DOM
  serialisation (numbered interactive elements) and a step loop that
  feeds the DOM + a goal to the model.
- **Resource model:** One Playwright `BrowserContext` per agent run;
  no service layer.
- **Lessons for Nimbus:** The DOM serialisation strategy
  (numbered-element clickable index over a cleaned a11y tree) is the
  dominant LLM-readable format. The library is consumer of a browser
  resource, not a provider of one — directly compatible with a Nimbus
  BrowserService underneath.

### Skyvern, MultiOn, Magma, others

- Skyvern: open-source agent runner on top of Playwright with vision
  fallback for unstable DOMs.
- MultiOn: consumer-facing agentic browser, opaque architecture.
- Magma (Microsoft Research): browser-based agent with novel
  set-of-mark visual grounding.
- **Lessons for Nimbus:** All are *consumers* of a browser resource.
  Their value-add is in the agent loop, not in browser supervision.
  Nimbus's BrowserService should be agent-loop-agnostic.

### Bright Data, Zyte, ScrapingBee, Apify (incumbents)

- Old-school scraping-focused vendors. Headless browser + proxy network
  + CAPTCHA-solving. Headless-Chrome-as-a-service was their playbook
  long before agents existed.
- **Lessons for Nimbus:** Anti-detection, proxy routing, and
  geo-targeting are deep wells if Nimbus ever needs them. Out of scope
  for the agent BrowserService MVP, but worth modelling as an optional
  per-session policy from day one so adding them later is not a
  rewrite.

### Arc, Dia (consumer agentic browsers)

- Different shape: a consumer browser with agent features grafted in,
  not an agent service that happens to run a browser.
- **Lessons for Nimbus:** Out of scope. Mentioned only to disambiguate
  — Nimbus is not building a consumer browser.

---

## Patterns Common To All

Every serious implementation in the survey above shares the following
properties. Nimbus should match them unless there is a specific reason
not to.

1. **BrowserContext is the session unit.** Not a process, not a tab.
2. **Named, addressable sessions.** Sessions have IDs that survive
   reconnects, restarts, and (where supported) host failover.
3. **Storage state is durable and serialisable.** Cookies + localStorage
   + IndexedDB can be snapshotted to a blob and rehydrated into a fresh
   context.
4. **A small warm pool hides cold-start.** Even systems that don't
   advertise pooling do it.
5. **The wire protocol is Playwright- or CDP-compatible.** Custom
   protocols lose; everyone who tried one ended up implementing the
   compatibility shim.
6. **Network egress is policy.** Proxy, allowlist, denylist, and TLS
   pinning per session — all systems with serious customers expose
   these knobs.
7. **The service supervises the browser; it does not embed it.** No
   serious system links Chromium into its own process via CEF for
   agent-service purposes. The reasons are universal: binary bloat,
   security-update cadence, sandbox-model collision.

## Patterns That Diverge

The interesting design choices where systems disagree:

| Choice | Options seen | Tradeoff |
|---|---|---|
| Browsers per node | One Chrome with many contexts (Browserless, Steel) vs many short-lived Chromes (some older Apify shapes) | Single-Chrome is cheaper; many-Chromes gives stronger crash isolation. Current consensus: single-Chrome until evidence forces splitting. |
| Sandbox tier | Container (Browserless, Steel), K8s pod (Browserbase, Cloudflare), VM-per-session (Operator) | Stronger isolation costs more memory and cold-start. |
| Session affinity | Required for stateful (Browserbase, Steel) vs stateless reissue from storage state (Cloudflare patterns) | Affinity is simpler for the user; stateless reissue scales better. |
| Headed vs headless | Headless default (Browserless, Cloudflare) vs headed default (Operator, Computer Use) | Headed is needed for some anti-detection scenarios; headless is cheaper. |
| Anti-detection depth | Stock Chrome (Cloudflare, Steel basic tier) vs patched/Brave/Camoufox (Browserbase, paid tiers) | Anti-detection adds operational complexity and a maintenance burden. |
| Audit | Network log only (most) vs full session video (Browserbase) | Video is invaluable for debugging agent failures; expensive to store. |

---

## Sandbox-Boundary Choices

Three viable tiers, ordered by isolation strength and cost:

1. **Process-level sandbox** (macOS Seatbelt, Linux seccomp + namespaces,
   Windows AppContainer). Chrome already uses one internally for its
   renderer processes. Cheapest. Acceptable for trusted single-tenant
   deployments. Probably wrong for Nimbus's multi-tenant story unless
   layered.
2. **Container** (OCI / Docker / Podman). What Browserless and Steel
   use. Strong filesystem isolation, weaker kernel isolation. Easy on
   Linux; awkward to nothing on macOS/Windows.
3. **MicroVM / VM** (firecracker, cloud-hypervisor, krun on Linux;
   Virtualization.framework on macOS; Hyper-V on Windows). What
   Browserbase, Cloudflare, and Operator effectively use. Strongest
   isolation; highest memory floor (~256–512MB per VM before Chrome);
   awkward GPU.

Nimbus already standardised on the microVM tier for `nimbus-sandbox`
via krun on Linux and Apple's Virtualization framework on macOS (see
`docs/architecture/sandbox/microvm-service-baseline.md`). The
BrowserService should live behind the same boundary, not invent a new
one.

**Open question:** does the browser get its own microVM per
deployment, or share the existing microVM that already hosts user
functions? Sharing is simpler and matches Cloudflare's pattern.
Splitting is more secure and matches Browserbase's pattern. Decision
deferred to plan promotion time.

---

## Resource Model: Process vs Context vs Tab

Restated explicitly because this is the most common architectural
mistake in agent-browser designs.

| Unit | What it isolates | Cost | When it is the right unit |
|---|---|---|---|
| **Browser process** | Chrome version, command-line flags, binary (Chrome vs Brave), the whole sandbox | High (hundreds of ms to seconds + ~150–300MB) | Different binary, different policy bundle, hard crash-isolation tier |
| **BrowserContext** | Cookies, localStorage, IndexedDB, service workers, permissions, origin trust | Low (~milliseconds, low MBs) | Per-session isolation for agents |
| **Tab (page)** | Nothing meaningful from a security standpoint | Trivial | Multi-tab work *inside* a single session |

Nimbus's default should be: **one Chrome per sandbox VM, one
BrowserContext per agent session, tabs as needed within a session.**

Second Chrome appears only when the agent needs:

- A different binary (Brave, Chromium-with-flags, Camoufox-style fork).
- A different command-line policy that cannot be applied per context
  (`--lang`, `--proxy-server` if not using context-level proxy,
  `--disable-features=...`).
- A crash-isolation tier for a high-value profile.

---

## Profile Durability

A named Nimbus session needs to survive:

- The agent function ending.
- The Chrome process restarting.
- The sandbox VM restarting.
- A host failover.

The durable artefact is **storage state**, not Chrome's
`--user-data-dir`. Concretely:

```text
session_id → blob {
  cookies: [...],
  origins: [
    { origin: "https://github.com", localStorage: [...], indexedDb: [...] }
  ],
  browser_context_options: { viewport, userAgent, locale, timezone,
                             geolocation, permissions, proxy }
}
```

The blob lives in `nimbus-storage`. On session open, the BrowserService
materialises a fresh `BrowserContext` and hydrates it from the blob. On
session close (or periodic checkpoint), it serialises the context back
to the blob.

Snapshot strategy:

- **On every page navigation** is too expensive.
- **On session close only** loses state if Chrome crashes.
- **Periodic + on close + on `commit()` host call** is the right
  middle: agents that care about durability call `commit()` after a
  successful flow; everyone else gets best-effort.

This matches Playwright's `context.storageState()` API exactly. Nimbus
can use Playwright's serialised format as the on-disk schema or define
its own — the Playwright format is well-specified and stable.

---

## DOM Extraction For LLMs

Agents do not consume raw HTML well. Every system in the survey
converges on one of two extraction strategies:

1. **Cleaned accessibility tree.** Walk the a11y tree, drop
   presentational nodes, number interactive elements, attach role +
   name + bounds. This is what Browser Use, Skyvern, and Stagehand
   produce. Token-cheap, structurally faithful, LLM-friendly.
2. **Annotated screenshot ("set of marks").** Render the page, overlay
   numbered boxes on interactive elements, hand the model both the
   image and the index. Computer Use and Magma use variants of this.
   Token-expensive but robust to weird DOMs.

A Nimbus BrowserService should expose both as opt-in extractors and
cache the result keyed by `(session_id, page_url, dom_revision)` so
multiple agent turns on the same page don't re-extract.

The extraction layer is the single biggest LLM cost lever in this whole
design. Worth treating as a first-class subsystem, not an afterthought.

---

## Anti-Detection And Network Policy

Out of scope for an MVP, but worth scaffolding for:

- **Proxy per session.** Plain HTTP/HTTPS/SOCKS5 proxy URL applied at
  context creation time. Cheap, ubiquitous.
- **TLS / `User-Agent` / locale / timezone / viewport pinning.**
  Per-context. Cheap.
- **Fingerprint randomisation.** Canvas, WebGL, audio context, font
  list. Requires either a forked Chrome (Camoufox) or a CDP-level
  spoof script (`Page.addScriptToEvaluateOnNewDocument`). Real work.
- **CAPTCHA solving.** External service integration. Out of scope.
- **Residential proxy inventory.** Out of scope; vendor problem.

Day-one position: expose proxy + `User-Agent` + viewport + locale +
timezone as per-session context options. Everything else deferred.

---

## Where Nimbus Is Different

The survey makes clear that the *shape* of a BrowserService is
well-understood. What makes a Nimbus BrowserService non-redundant is
the integration story:

1. **Engine-owned, not a separate service.** Browser ops flow through
   the same `apply_mutation` path as DB writes. Replay, scheduler,
   audit all "just work."
2. **`ctx.browser` is a runtime host-bridge op.** Agents inside
   `nimbus-runtime` reach it the same way they reach `ctx.db`. No
   separate auth, no separate URL.
3. **Storage is `nimbus-storage`.** Session blobs live next to user
   documents, with the same atomicity guarantees.
4. **Sandbox is `nimbus-sandbox`.** Same isolation tier as user
   functions, not a parallel infrastructure.
5. **Audit is the mutation journal.** Browser ops are observable,
   schedulable, and replayable through the existing journal.
6. **No per-session-minute billing.** Self-hosted by default.

These are *integration* wins, not *browser* wins. The browser layer
itself looks like everyone else's: one Chrome behind the sandbox, many
BrowserContexts, Playwright-compatible wire format.

---

## Open Questions

Decisions to make before promoting an active plan:

1. **Chrome or Brave as the default binary?** Brave gives anti-fingerprint and
   ad-blocking defaults but pins Nimbus to Brave's release train. Chrome is
   safer.
2. **Playwright-compatible API or Nimbus-native API?** Playwright
   compatibility lets every agent library "just work" but couples
   Nimbus to Playwright's protocol surface area (which is large).
   Nimbus-native is simpler but a porting cost for users.
3. **One sandbox VM for all sessions, or one VM per session?**
   Cloudflare-style sharing vs Browserbase-style per-session. Sharing
   is the MVP path; per-session is a later isolation tier.
4. **A11y tree extractor or set-of-marks screenshot extractor first?**
   Both eventually; pick the MVP. A11y tree is cheaper and broadly
   sufficient.
5. **Where does the BrowserService live in the workspace?** Most
   natural: a new `crates/nimbus-browser/` crate sitting between
   `nimbus-engine` and `nimbus-sandbox`, with a thin JS facade in
   `packages/nimbus/`.
6. **Is this part of `nimbus:agent` WIT capabilities or a separate
   capability?** Probably a separate capability (`nimbus:browser` or
   `nimbus:agent/browser`) gated on the same tenant admission flag.

These belong in the plan's promotion criteria, not here.

---

## References

External systems referenced above. Treat as starting points; verify
current behavior before basing decisions on any single source.

- Browserless: `https://www.browserless.io/`, GitHub
  `browserless/browserless`.
- Browserbase: `https://www.browserbase.com/`. Stagehand SDK:
  `browserbase/stagehand`.
- Cloudflare Browser Rendering:
  `https://developers.cloudflare.com/browser-rendering/`.
- Steel: `https://steel.dev/`, GitHub `steel-dev/steel-browser`.
- Anthropic Computer Use reference image:
  `https://github.com/anthropics/anthropic-quickstarts/tree/main/computer-use-demo`.
- OpenAI Operator: product page only; architecture from public talks.
- Browser Use: GitHub `browser-use/browser-use`.
- Skyvern: GitHub `Skyvern-AI/skyvern`.
- Chrome DevTools Protocol:
  `https://chromedevtools.github.io/devtools-protocol/`.
- Playwright BrowserContext API:
  `https://playwright.dev/docs/api/class-browsercontext`.
- WebDriver BiDi: `https://w3c.github.io/webdriver-bidi/`.

Internal references:

- `docs/architecture/sandbox/microvm-service-baseline.md` — the
  sandbox tier the BrowserService would sit behind.
- `docs/architecture/runtime/adapter-boundary.md` — the runtime
  boundary `ctx.browser` would cross.
- `docs/plans/wasi-agent-capabilities-plan.md` — adjacent agent
  capability work; the browser is the next obvious capability after
  filesystem/process/http.
- `docs/plans/wasmtime-backend-plan.md` — the wasmtime substrate the
  browser bindings would extend if delivered as a WIT capability.

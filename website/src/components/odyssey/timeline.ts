// The timeline owns the scroll → scene-time contract: acts, chapter windows,
// the request's narrative state, and the camera path for each viewport class.
// Everything here is pure data and pure math so the world renderer and the
// static storyboard can share it.

export type Align = 'left' | 'right';
export type Viewport = 'wide' | 'medium' | 'compact';
export type ActId = 'start' | 'network' | 'compute' | 'storage' | 'agents' | 'workloads' | 'run';

export type Snippet = {
  file: string;
  lines: string[];
  // Shorter lines for a phone column, where a long line would wrap.
  compact?: string[];
};

export type Chapter = {
  id: string;
  act: ActId;
  label: string;
  eyebrow: string;
  title: string;
  copy: string;
  proof: string;
  snippet: Snippet;
  start: number;
  end: number;
  align: Align;
};

export type Act = { id: ActId; label: string };

export const acts: Act[] = [
  { id: 'network', label: 'Network' },
  { id: 'compute', label: 'Compute' },
  { id: 'storage', label: 'Storage' },
  { id: 'agents', label: 'Agents' },
  { id: 'workloads', label: 'Workloads' },
  { id: 'run', label: 'Run' },
];

export type CameraKeyframe = {
  at: number;
  x: number;
  y: number;
  zoom: number;
  // Screen-space anchor (0..1) where the camera target lands. Alternating the
  // anchor keeps the world in the negative space beside the copy.
  anchorX: number;
  anchorY: number;
};

export type Camera = Omit<CameraKeyframe, 'at'>;

// Nineteen chapters. Each window is 0.042 of the scroll, with a 0.0112 gap
// where the camera travels and the copy crossfades.
export const chapters: Chapter[] = [
  {
    id: 'code',
    act: 'start',
    label: 'CODE',
    eyebrow: 'Nimbus · beta',
    title: 'Keep the SDK. Change the address.',
    copy: 'Your Convex, Firestore, MongoDB, or DynamoDB code does not change. Point it at one Rust binary on your machine.',
    proof: 'CONVEX · FIRESTORE · CLOUD FUNCTIONS · MONGODB · DYNAMODB',
    snippet: {
      file: 'app/messages.ts',
      lines: [
        'import { ConvexHttpClient } from "convex/browser";',
        'import { api } from "./convex/_generated/api";',
        '',
        '// http://localhost:3210/convex/demo',
        'const client = new ConvexHttpClient(process.env.NEXT_PUBLIC_CONVEX_URL);',
        '',
        'await client.action(api.messages.send, { body: "hello" });',
      ],
      compact: [
        'import { ConvexHttpClient } from "convex/browser";',
        'import { api } from "./convex/_generated/api";',
        '// http://localhost:3210/convex/demo',
        'const client = new ConvexHttpClient(',
        '  process.env.NEXT_PUBLIC_CONVEX_URL);',
        'await client.action(api.messages.send,',
        '  { body: "hello" });',
      ],
    },
    start: 0,
    end: 0.0399,
    align: 'left',
  },
  {
    id: 'protocols',
    act: 'network',
    label: 'CLIENT PROTOCOLS',
    eyebrow: 'Network · 01 / Client protocols',
    title: 'Every SDK has its own endpoint on one host.',
    copy: 'Each SDK keeps its own wire format and port. One process on one host answers all of them.',
    proof: ':3210 HTTP + WS · :27017 MONGODB · :8000 DYNAMODB',
    snippet: {
      file: '.env.local',
      lines: [
        '# written by nimbus dev',
        'NIMBUS_DEPLOYMENT=http://localhost:3210/convex/demo',
        'NIMBUS_MONGODB_URL=mongodb://127.0.0.1:27017/…',
        'NIMBUS_DYNAMODB_ENDPOINT=http://127.0.0.1:8000',
        '# you set the variable your Convex client reads:',
        'NEXT_PUBLIC_CONVEX_URL=http://localhost:3210/convex/demo',
      ],
      compact: [
        '# written by nimbus dev',
        'NIMBUS_DEPLOYMENT=http://localhost:3210/convex/demo',
        'NIMBUS_MONGODB_URL=mongodb://127.0.0.1:27017/…',
        'NIMBUS_DYNAMODB_ENDPOINT=http://127.0.0.1:8000',
        '# you set the variable your client reads:',
        'NEXT_PUBLIC_CONVEX_URL=',
        '  http://localhost:3210/convex/demo',
      ],
    },
    start: 0.0505,
    end: 0.0905,
    align: 'right',
  },
  {
    id: 'adapters',
    act: 'network',
    label: 'CLIENT ADAPTERS',
    eyebrow: 'Network · 02 / Client adapters',
    title: 'Every SDK protocol is an adapter.',
    copy: 'Each adapter turns its protocol into engine operations and passes an authenticated identity, not a raw token. Every SDK meets the same rules and the same database.',
    proof: 'nimbus-server · nimbus-adapters · nimbus-engine',
    snippet: {
      file: 'convex/agent.ts',
      lines: [
        'export const send = action({',
        '  args: { body: v.string() },',
        '  handler: async (ctx, { body }) => {',
        '    await ctx.runMutation(internal.messages.write, { body });   // chapter 05',
        '    await nimbus.sessions.open({                                // chapter 13',
        '      target: { service: { name: "agent" } }, channels: ["stdio"],',
        '    });',
        '  },',
        '});',
      ],
      compact: [
        'export const send = action({',
        '  args: { body: v.string() },',
        '  handler: async (ctx, { body }) => {',
        '    await ctx.runMutation(   // chapter 05',
        '      internal.messages.write, { body });',
        '  },',
        '});',
      ],
    },
    start: 0.1011,
    end: 0.1411,
    align: 'left',
  },
  {
    id: 'functions',
    act: 'compute',
    label: 'FUNCTIONS',
    eyebrow: 'Compute · 03 / Functions',
    title: 'Functions run next to the data.',
    copy: 'The handler runs on V8 inside the binary. Queries and mutations reach the database with no network hop. Only actions reach the network.',
    proof: 'V8 IN-PROCESS · ONLY ACTIONS REACH THE NETWORK',
    snippet: {
      file: 'convex/agent.ts',
      lines: [
        'import { Nimbus } from "@nimbus/nimbus";',
        '',
        '// the SDK is one more HTTP client, so only an action may hold it',
        'const nimbus = new Nimbus({',
        '  endpoint: process.env.NIMBUS_URL,',
        '  tenantId: "demo",',
        '  token: process.env.NIMBUS_TOKEN,',
        '});',
      ],
      compact: [
        'import { Nimbus } from "@nimbus/nimbus";',
        '',
        '// one more HTTP client: actions only',
        'const nimbus = new Nimbus({',
        '  endpoint: process.env.NIMBUS_URL,',
        '  tenantId: "demo",',
        '  token: process.env.NIMBUS_TOKEN,',
        '});',
      ],
    },
    start: 0.1517,
    end: 0.1916,
    align: 'right',
  },
  {
    id: 'node',
    act: 'compute',
    label: 'NODE',
    eyebrow: 'Compute · 04 / Node',
    title: 'One directive moves an action to Node.',
    copy: 'Add "use node" to an action to run it on Node 22, 24, or 26 with npm packages. Native addons and subprocesses need a sandbox.',
    proof: 'NODE 22 · 24 · 26 · ACTIONS ONLY · NO NATIVE ADDONS',
    snippet: {
      file: 'convex/agent.ts',
      lines: [
        '"use node";',
        'import OpenAI from "openai";',
        'import { action } from "./_generated/server";',
        'import { v } from "convex/values";',
        '',
        'const openai = new OpenAI();   // fetch, inside the tenant\'s egress policy',
      ],
      compact: [
        '"use node";',
        'import OpenAI from "openai";',
        '',
        'const openai = new OpenAI();',
        '// fetch, inside the tenant\'s egress policy',
      ],
    },
    start: 0.2023,
    end: 0.2422,
    align: 'right',
  },
  {
    id: 'commit',
    act: 'storage',
    label: 'WRITES',
    eyebrow: 'Storage · 05 / Writes',
    title: 'Every write is one database transaction.',
    copy: 'A mutation and a driver insertOne take the same path. One transaction writes the document, its indexes, and the commit log. Nimbus acknowledges only durable writes.',
    proof: 'DOCUMENT + INDEX + COMMIT LOG = ONE TRANSACTION',
    snippet: {
      file: 'convex/messages.ts',
      lines: [
        'export const write = internalMutation({',
        '  args: { body: v.string() },',
        '  handler: async (ctx, args) => {',
        '    await ctx.db.insert("messages", args);',
        '  },',
        '});',
      ],
    },
    start: 0.2528,
    end: 0.2927,
    align: 'right',
  },
  {
    id: 'live',
    act: 'storage',
    label: 'REALTIME',
    eyebrow: 'Storage · 06 / Realtime',
    title: 'Every transaction updates live queries.',
    copy: 'A MongoDB write updates a live Convex query the moment it commits. There is no polling and no pub/sub service to run.',
    proof: 'CONVEX WS · onSnapshot · runAfter AT-LEAST-ONCE',
    snippet: {
      file: 'app/Chat.tsx',
      lines: [
        'const messages = useQuery(api.messages.list, {});',
        '',
        '// re-renders the moment the commit lands',
        'return messages?.map((m) => <li key={m._id}>{m.body}</li>);',
      ],
      compact: [
        'const messages =',
        '  useQuery(api.messages.list, {});',
        '// re-renders the moment the commit lands',
        'return messages?.map((m) =>',
        '  <li key={m._id}>{m.body}</li>);',
      ],
    },
    start: 0.3033,
    end: 0.3433,
    align: 'left',
  },
  {
    id: 'backends',
    act: 'storage',
    label: 'DATABASE',
    eyebrow: 'Storage · 07 / Database',
    title: 'Pick the database. SQLite is the default.',
    copy: 'SQLite runs inside the process with zero setup. Point one flag at Postgres, MySQL, or libSQL when you already run one. Every API works on every backend.',
    proof: 'SQLITE DEFAULT · POSTGRES · MYSQL · LIBSQL · REDB',
    snippet: {
      file: 'shell',
      lines: [
        '$ nimbus start                       # SQLite in ./data',
        '$ nimbus start --tenant-provider postgres \\',
        '    --postgres-url postgresql://db:5432/nimbus',
        '$ nimbus kv                          # RESP on 127.0.0.1:6380',
      ],
      compact: [
        '$ nimbus start      # SQLite in ./data',
        '$ nimbus start \\',
        '    --tenant-provider postgres \\',
        '    --postgres-url postgresql://…/nimbus',
        '$ nimbus kv         # RESP on 127.0.0.1:6380',
      ],
    },
    start: 0.3539,
    end: 0.3939,
    align: 'right',
  },
  {
    id: 'files',
    act: 'storage',
    label: 'FILES',
    eyebrow: 'Storage · 08 / Files',
    title: 'One blob store for S3 objects, files, and volumes.',
    copy: 'Each tenant has one encrypted, content-addressed store. Uploads, the S3 endpoint, the function filesystem, and sandbox volumes read the same bytes.',
    proof: 'BLAKE3 · ENCRYPTED · S3 API :9000',
    snippet: {
      file: 'compose.yaml',
      lines: [
        'services:',
        '  agent:',
        '    image: python:3.12',
        '    volumes:',
        '      - scratch:/work   # a named tenant volume',
        'volumes:',
        '  scratch: {}           # no host bind mounts · S3 API on :9000',
      ],
      compact: [
        'services:',
        '  agent:',
        '    image: python:3.12',
        '    volumes: [scratch:/work]',
        'volumes: { scratch: {} }',
      ],
    },
    start: 0.4045,
    end: 0.4444,
    align: 'right',
  },
  {
    id: 'box',
    act: 'agents',
    label: 'SANDBOX',
    eyebrow: 'Agents · 09 / Sandbox',
    title: 'Agent sandboxes run outside the Nimbus process.',
    copy: 'Compose gives each agent an OCI image, a microVM or container, and a volume at /work. No daemon runs. The sandbox has no path back to the engine.',
    proof: 'LIKE A POD · OCI IMAGE · NO DAEMON · CRUN · LIBKRUN',
    snippet: {
      file: 'compose.yaml',
      lines: [
        'services:',
        '  agent:',
        '    image: docker.io/library/python:3.12   # pulled and unpacked, no daemon',
        '    command: ["python", "agent.py"]',
        '    volumes: ["scratch:/work"]              # the volume from chapter 08',
        '    deploy: { resources: { limits: { cpus: "1", memory: 512M } } }',
        '    x-nimbus: { backend: krun, egress: { allow: [] } }   # microVM · chapter 10',
      ],
      compact: [
        'services:',
        '  agent:',
        '    image: docker.io/library/python:3.12',
        '    command: ["python", "agent.py"]',
        '    volumes: ["scratch:/work"]',
        '    x-nimbus: { backend: krun }   # microVM',
      ],
    },
    start: 0.455,
    end: 0.495,
    align: 'left',
  },
  {
    id: 'egress',
    act: 'agents',
    label: 'EGRESS',
    eyebrow: 'Agents · 10 / Egress',
    title: 'Nimbus denies agent network access by default.',
    copy: 'The egress proxy denies every sandbox request by default. An allow rule names one protocol, one host, one port, and its paths, with no wildcards.',
    proof: 'nimbus-egress DECIDES · nimbus-proxy ENFORCES',
    snippet: {
      file: 'compose.yaml',
      lines: [
        'services:',
        '  agent:',
        '    x-nimbus:',
        '      egress:',
        '        allow:',
        '          - { name: stripe-api, protocol: https, host: api.stripe.com,',
        '              port: 443, methods: [POST], path_prefixes: [/v1/] }',
      ],
      compact: [
        'x-nimbus:                  # services.agent',
        '  egress:',
        '    allow:',
        '      - { name: stripe-api, protocol: https,',
        '          host: api.stripe.com, port: 443,',
        '          methods: [POST], path_prefixes: [/v1/] }',
      ],
    },
    start: 0.5056,
    end: 0.5455,
    align: 'right',
  },
  {
    id: 'services',
    act: 'workloads',
    label: 'SERVICES',
    eyebrow: 'Workloads · 11 / Services',
    title: 'Code depends on a name, not a sandbox.',
    copy: 'A service is a name that other code depends on. A sandbox runs it. Replace the sandbox and the name still resolves.',
    proof: 'LIKE A K8S SERVICE · SAME VERBS IN COMPOSE AND API',
    snippet: {
      file: 'app/agent.ts',
      lines: [
        'const agent = await nimbus.services.get({ name: "agent" });',
        '',
        '// every write fences on the generation it read',
        'await nimbus.services.restart({',
        '  name: "agent", sourceGeneration: agent.metadata.generation,',
        '});',
      ],
      compact: [
        'const agent = await nimbus.services.get({',
        '  name: "agent" });',
        'await nimbus.services.restart({',
        '  name: "agent",',
        '  sourceGeneration: agent.metadata.generation,',
        '});',
      ],
    },
    start: 0.5561,
    end: 0.5961,
    align: 'left',
  },
  {
    id: 'workloads',
    act: 'workloads',
    label: 'COMPOSE',
    eyebrow: 'Workloads · 12 / Compose',
    title: 'Run an app as services on one host.',
    copy: 'A compose file names each service. Each one runs in its own microVM or container next to the engine. One service runs one sandbox today. Replicas are on the roadmap.',
    proof: 'ONE SANDBOX PER SERVICE · REPLICAS PLANNED',
    snippet: {
      file: 'compose.yaml',
      lines: [
        'services:',
        '  web:',
        '    image: ghcr.io/acme/web:1.4',
        '    ports: ["127.0.0.1:3000:8080"]   # host listener → guest port',
        '  agent:',
        '    image: ghcr.io/acme/agent:1.4',
        '    command: ["python", "agent.py"]  # a microVM by default',
        '  worker:',
        '    image: ghcr.io/acme/worker:1.4',
        '    x-nimbus: { backend: crun }      # a container instead',
      ],
      compact: [
        'services:',
        '  web:',
        '    image: ghcr.io/acme/web:1.4',
        '    ports: ["127.0.0.1:3000:8080"]',
        '  agent:',
        '    image: ghcr.io/acme/agent:1.4',
        '    command: ["python", "agent.py"]',
        '  worker:',
        '    image: ghcr.io/acme/worker:1.4',
        '    x-nimbus: { backend: crun }',
      ],
    },
    start: 0.6067,
    end: 0.6467,
    align: 'left',
  },
  {
    id: 'sessions',
    act: 'workloads',
    label: 'SESSIONS',
    eyebrow: 'Workloads · 13 / Sessions',
    title: 'A session is an audited connection to a running service.',
    copy: 'Plain service use needs no session. Open one for stdio, file exchange, or browser control. Nimbus authorizes the lease at open, and it expires at its TTL.',
    proof: 'LIKE KUBECTL EXEC · TTL 15 MIN · NO BYTES YET',
    snippet: {
      file: 'app/agent.ts',
      lines: [
        'const shell = await nimbus.sessions.open({',
        '  target: { service: { name: "agent" } }, channels: ["stdio"],',
        '});',
        'console.log(shell.spec.targetSnapshot.service);',
        '// { name: "agent", generation: 3, backend: "sandbox" }',
        'console.log(shell.spec.expiresAt);   // 15 min out',
      ],
      compact: [
        'const shell = await nimbus.sessions.open({',
        '  target: { service: { name: "agent" } },',
        '  channels: ["stdio"],',
        '});',
        'console.log(shell.spec.expiresAt);   // 15 min out',
      ],
    },
    start: 0.6573,
    end: 0.6972,
    align: 'right',
  },
  {
    id: 'binary',
    act: 'run',
    label: 'BINARY',
    eyebrow: 'Run · 14 / Binary',
    title: 'You run one process, not a platform.',
    copy: 'Network, compute, storage, and agents ship in one Rust binary with one trust model. There is no sidecar and no queue to run.',
    proof: 'ONE BINARY · NO DOCKER · NO KUBERNETES · NO QUEUE',
    snippet: {
      file: 'shell',
      lines: ['$ ls', 'nimbus            # one file', '', '$ ./nimbus start  # :8080 · network, compute, storage'],
      compact: [
        '$ ls',
        'nimbus            # one file',
        '',
        '$ ./nimbus start',
        '# :8080 · network, compute, storage',
      ],
    },
    start: 0.7078,
    end: 0.7477,
    align: 'right',
  },
  {
    id: 'tenants',
    act: 'run',
    label: 'TENANTS',
    eyebrow: 'Run · 15 / Tenants',
    title: 'Tenants cannot see each other.',
    copy: 'Each tenant has its own database, file store, key, and budget. Nimbus admits a request once. No API can express a cross-tenant read.',
    proof: 'OWN DATABASE · KEY · BUDGET · 429 OVER BUDGET',
    snippet: {
      file: 'shell',
      lines: [
        '$ curl -s -X POST http://localhost:8080/api/tenants \\',
        '    -H "Authorization: Bearer $NIMBUS_TOKEN" \\',
        "    -d '{\"id\": \"acme\"}'",
        '{"id": "acme"}   # own SQLite file · own blob store + key',
        '$ nimbus start --runtime-max-active-per-tenant 8',
      ],
      compact: [
        '$ curl -s -X POST \\',
        '    http://localhost:8080/api/tenants \\',
        '    -H "Authorization: Bearer $NIMBUS_TOKEN" \\',
        "    -d '{\"id\": \"acme\"}'",
        '{"id": "acme"}   # own file · own store + key',
        '$ nimbus start \\',
        '    --runtime-max-active-per-tenant 8',
      ],
    },
    start: 0.7584,
    end: 0.7983,
    align: 'right',
  },
  {
    id: 'cloud',
    act: 'run',
    label: 'DEPLOYMENT',
    eyebrow: 'Run · 16 / Deployment',
    title: 'Deploy to any Linux host you own.',
    copy: 'Install on any Linux host and run it as a service. Deploy from a laptop with a dry run first.',
    proof: 'NO TELEMETRY · NO METERING · NO CLUSTERING YET',
    snippet: {
      file: 'shell',
      lines: [
        '$ curl -fsSL https://github.com/nimbus/nimbus/…/install.sh | sh',
        '# write /etc/systemd/system/nimbus.service, then',
        '$ sudo systemctl enable --now nimbus',
        '',
        '$ nimbus deploy https://nimbus.example.com --dry-run',
      ],
      compact: [
        '$ curl -fsSL \\',
        '    https://github.com/nimbus/…/install.sh | sh',
        '# write /etc/systemd/system/nimbus.service, then',
        '$ sudo systemctl enable --now nimbus',
        '',
        '$ nimbus deploy https://nimbus.example.com \\',
        '    --dry-run',
      ],
    },
    start: 0.8089,
    end: 0.8489,
    align: 'left',
  },
  {
    id: 'cluster',
    act: 'run',
    label: 'CLUSTER',
    eyebrow: 'Run · 17 / Cluster · planned',
    title: 'Scale out to more hosts later.',
    copy: 'Planned cluster mode joins hosts over a QUIC mesh. Each node is an Ed25519 key, not an IP address. A host joins with a token as a learner, and each tenant gets one owner node. None of this ships today.',
    proof: 'IROH + OPENRAFT · QUIC UDP/7842 · RELAY TCP/443',
    snippet: {
      file: 'shell · planned',
      lines: [
        '# planned · not in any release yet',
        '$ nimbus cluster init                  # the first host',
        '$ nimbus cluster join-token create     # on a member',
        '$ nimbus cluster join <token>          # on the new host · a learner first',
        '$ nimbus cluster promote <id>          # then a voter',
        '$ nimbus cluster members               # nodes by key · role · tenant owner',
      ],
      compact: [
        '# planned · not in any release yet',
        '$ nimbus cluster init',
        '$ nimbus cluster join-token create',
        '$ nimbus cluster join <token>   # a learner',
        '$ nimbus cluster promote <id>   # a voter',
        '$ nimbus cluster members        # by key',
      ],
    },
    start: 0.8595,
    end: 0.8995,
    align: 'left',
  },
  {
    id: 'operator',
    act: 'run',
    label: 'OPERATOR',
    eyebrow: 'Run · 18 / Operator · planned',
    title: 'Reach the cluster from a laptop.',
    copy: 'The planned operator plane is the same mesh. The CLI dials any node by key, through NAT, with an operator key that has admin and port-forward scopes only. It is never a voter.',
    proof: 'OPERATOR KEY · ADMIN · OP-FORWARD · NEVER A VOTER',
    snippet: {
      file: 'shell · planned',
      lines: [
        '# planned · not in any release yet',
        '$ nimbus cluster members     # from a laptop · by key',
        '$ nimbus cluster status',
        '$ nimbus forward …           # a local TCP port to a port on one node',
        '# an agent takes the same path with a scoped key',
      ],
      compact: [
        '# planned · not in any release yet',
        '$ nimbus cluster members   # by key',
        '$ nimbus cluster status',
        '$ nimbus forward …   # a local port',
        '# an agent takes the same path',
      ],
    },
    start: 0.9101,
    end: 0.95,
    align: 'left',
  },
  {
    id: 'laptop',
    act: 'run',
    label: 'LAPTOP',
    eyebrow: 'Run · 19 / Laptop',
    title: 'Start local with three commands.',
    copy: 'Install the binary and point the app at localhost:3210. Nimbus is in beta. APIs can break between releases. Do not use it in production yet.',
    proof: 'BREW INSTALL · NIMBUS INIT · NIMBUS DEV · LOCALHOST:3210',
    snippet: {
      file: 'shell',
      lines: ['$ brew install nimbus/tap/nimbus', '$ nimbus init convex my-app', '$ cd my-app', '$ nimbus dev', '  Local:  http://localhost:3210'],
    },
    start: 0.9606,
    end: 1,
    align: 'left',
  },
];

export function actFor(chapter: Chapter) {
  return acts.find((act) => act.id === chapter.act) ?? null;
}

// Progress span of each act, from its first chapter's start to its last
// chapter's end. The hero has no act and owns the start of the rail.
export function actSpans() {
  return acts.map((act) => {
    const own = chapters.filter((chapter) => chapter.act === act.id);
    return { act, start: own[0].start, end: own[own.length - 1].end };
  });
}

// The request's narrative state, read out in the status bar and drawn beside
// the travelling object. Each entry starts at `at` and holds until the next.
export const requestStates: { at: number; label: string }[] = [
  { at: 0, label: 'CLIENT CALL · api.messages.send' },
  { at: 0.0505, label: 'WIRE FORMAT · :3210 /convex' },
  { at: 0.1011, label: 'ENGINE OPERATION' },
  { at: 0.1517, label: 'RUNTIME LANE' },
  { at: 0.1657, label: 'HANDLER RUNS · V8' },
  { at: 0.2023, label: 'SAME ISOLATE · NODE 24 TARGET' },
  { at: 0.2485, label: 'ctx.runMutation · HOST OP' },
  { at: 0.2577, label: 'AUTHORIZED' },
  { at: 0.2699, label: 'ONE TRANSACTION' },
  { at: 0.2829, label: 'COMMITTED' },
  { at: 0.3033, label: 'PUBLISHED' },
  { at: 0.3265, label: 'LIVE QUERIES UPDATED' },
  { at: 0.3539, label: 'DURABLE · SQLITE' },
  { at: 0.4045, label: 'VOLUME SNAPSHOT · SANDBOX' },
  { at: 0.455, label: 'nimbus.sessions.open · SDK CALL' },
  { at: 0.4665, label: 'ADMITTED · SANDBOX READY' },
  { at: 0.5056, label: 'BEHIND THE EGRESS PROXY' },
  { at: 0.5561, label: 'RESOLVED BY NAME · agent' },
  { at: 0.6067, label: 'ONE SANDBOX PER SERVICE' },
  { at: 0.6573, label: 'LEASED · SESSION OPEN' },
  { at: 0.6793, label: 'IN THE SANDBOX · stdio' },
  { at: 0.7078, label: 'ONE PROCESS' },
  { at: 0.7584, label: 'ISOLATED · TENANT demo' },
  { at: 0.828, label: 'ON A LINUX HOST' },
  { at: 0.8595, label: 'ON ONE HOST TODAY' },
  { at: 0.9013, label: 'DOWN TO THE LAPTOP' },
  { at: 0.9101, label: 'DIALS THE CLUSTER · PLANNED' },
  { at: 0.9606, label: 'ON A LAPTOP' },
];

export function requestStateFor(progress: number) {
  let state = requestStates[0].label;
  for (const entry of requestStates) {
    if (progress >= entry.at) state = entry.label;
  }
  return state;
}

export function clamp(value: number, min = 0, max = 1) {
  return Math.min(max, Math.max(min, value));
}

export function mix(from: number, to: number, amount: number) {
  return from + (to - from) * amount;
}

export function smoothstep(value: number) {
  const t = clamp(value);
  return t * t * (3 - 2 * t);
}

export function range(value: number, start: number, end: number) {
  return clamp((value - start) / Math.max(0.0001, end - start));
}

// 0 outside [start, end], 1 inside, eased over `edge` at both ends.
export function visibilityWindow(value: number, start: number, end: number, edge = 0.03) {
  const fadeIn = smoothstep(range(value, start, start + edge));
  const fadeOut = 1 - smoothstep(range(value, end - edge, end));
  return clamp(fadeIn * fadeOut);
}

// A chapter's copy fades over this much of the journey at each end.
export const COPY_EDGE = 0.0114;

// Where chapter travel lands: the point where the chapter's copy is fully on
// stage and its scene has played. The first stop is the top of the page,
// where the first chapter is already on stage.
export function chapterStop(index: number) {
  return index === 0 ? 0 : chapters[index].end - COPY_EDGE;
}

// A chapter's rest: from the point where its copy is fully on stage to the
// point where it starts to leave. The reader can stop anywhere inside it, and
// the scene's beats play under the scroll. The first chapter rests from the
// top of the page; the last rests to the end of the journey.
export function chapterRest(index: number) {
  const chapter = chapters[index];
  return {
    from: index === 0 ? 0 : chapter.start + COPY_EDGE,
    to: index === chapters.length - 1 ? 1 : chapter.end - COPY_EDGE,
  };
}

export function chapterIndexFor(progress: number) {
  let active = 0;
  for (let index = 1; index < chapters.length; index += 1) {
    const threshold = (chapters[index - 1].end + chapters[index].start) / 2;
    if (progress >= threshold) active = index;
  }
  return active;
}

export function viewportFor(width: number): Viewport {
  if (width < 760) return 'compact';
  if (width < 1100) return 'medium';
  return 'wide';
}

// Monotone cubic (Fritsch–Carlson) interpolation through keyframes. Velocity
// is continuous across keyframes, repeated values hold with zero velocity,
// and nothing overshoots. Camera moves and the request's route both use it,
// so neither stops dead at every waypoint.
export function monotoneCubic(times: number[], values: number[], t: number) {
  const count = times.length;
  if (count === 0) return 0;
  if (count === 1 || t <= times[0]) return values[0];
  if (t >= times[count - 1]) return values[count - 1];
  let index = 0;
  while (index < count - 2 && t > times[index + 1]) index += 1;
  const h = times[index + 1] - times[index];
  if (h <= 0) return values[index + 1];
  const slopes = (k: number) => (values[k + 1] - values[k]) / (times[k + 1] - times[k]);
  const secant = slopes(index);
  const tangentAt = (k: number) => {
    if (k === 0 || k === count - 1) return 0;
    const left = slopes(k - 1);
    const right = slopes(k);
    if (left * right <= 0) return 0;
    const m = (left + right) / 2;
    const bound = 3 * Math.min(Math.abs(left), Math.abs(right));
    return Math.sign(m) * Math.min(Math.abs(m), bound);
  };
  let m0 = tangentAt(index);
  let m1 = tangentAt(index + 1);
  if (secant === 0) {
    m0 = 0;
    m1 = 0;
  }
  const s = (t - times[index]) / h;
  const s2 = s * s;
  const s3 = s2 * s;
  const h00 = 2 * s3 - 3 * s2 + 1;
  const h10 = s3 - 2 * s2 + s;
  const h01 = -2 * s3 + 3 * s2;
  const h11 = s3 - s2;
  return h00 * values[index] + h10 * h * m0 + h01 * values[index + 1] + h11 * h * m1;
}

// Wide: copy sits in a left or right column, the world takes the other side.
// Chapters 04, 06, 08 and 14 dip: the camera drops from the rail to the Node
// card, the shelf, the byte plane and the tenant deck beneath it. Chapters 09 to
// 12 walk the agent plane: the box carved from the host, its proxy, the service
// that names it, the sessions that lease it.
const wideCamera: CameraKeyframe[] = [
  { at: 0, x: 40, y: 0, zoom: 1, anchorX: 0.7, anchorY: 0.5 },
  { at: 0.0399, x: 160, y: 0, zoom: 1, anchorX: 0.7, anchorY: 0.5 },
  { at: 0.0505, x: 1000, y: 0, zoom: 0.86, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.0905, x: 1060, y: 0, zoom: 0.86, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.1011, x: 1900, y: 0, zoom: 0.72, anchorX: 0.68, anchorY: 0.5 },
  { at: 0.1411, x: 1920, y: 0, zoom: 0.72, anchorX: 0.68, anchorY: 0.5 },
  { at: 0.1517, x: 2720, y: 0, zoom: 0.96, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.1916, x: 2740, y: 0, zoom: 0.96, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.2023, x: 2700, y: 100, zoom: 0.9, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.2422, x: 2700, y: 100, zoom: 0.9, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.2528, x: 3680, y: 0, zoom: 1.1, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.2927, x: 3780, y: 0, zoom: 1.1, anchorX: 0.34, anchorY: 0.5 },
  { at: 0.3033, x: 4180, y: 0, zoom: 0.85, anchorX: 0.7, anchorY: 0.5 },
  { at: 0.3433, x: 4180, y: 0, zoom: 0.85, anchorX: 0.7, anchorY: 0.5 },
  { at: 0.3539, x: 4300, y: 1020, zoom: 0.63, anchorX: 0.29, anchorY: 0.5 },
  { at: 0.3939, x: 4300, y: 1020, zoom: 0.63, anchorX: 0.29, anchorY: 0.5 },
  { at: 0.4045, x: 5330, y: 960, zoom: 0.64, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.4444, x: 5330, y: 960, zoom: 0.64, anchorX: 0.36, anchorY: 0.5 },
  { at: 0.455, x: 6450, y: 1450, zoom: 0.59, anchorX: 0.73, anchorY: 0.5 },
  { at: 0.495, x: 6450, y: 1450, zoom: 0.59, anchorX: 0.73, anchorY: 0.5 },
  { at: 0.5056, x: 6760, y: 1300, zoom: 0.58, anchorX: 0.43, anchorY: 0.5 },
  { at: 0.5455, x: 6760, y: 1300, zoom: 0.58, anchorX: 0.43, anchorY: 0.5 },
  { at: 0.5561, x: 6420, y: 936, zoom: 0.53, anchorX: 0.68, anchorY: 0.48 },
  { at: 0.5961, x: 6420, y: 936, zoom: 0.53, anchorX: 0.68, anchorY: 0.48 },
  { at: 0.6067, x: 7760, y: 1610, zoom: 0.56, anchorX: 0.68, anchorY: 0.5 },
  { at: 0.6467, x: 7760, y: 1610, zoom: 0.56, anchorX: 0.68, anchorY: 0.5 },
  { at: 0.6573, x: 6250, y: 780, zoom: 0.6, anchorX: 0.3, anchorY: 0.5 },
  { at: 0.6972, x: 6250, y: 780, zoom: 0.6, anchorX: 0.3, anchorY: 0.5 },
  { at: 0.7078, x: 4000, y: 200, zoom: 0.108, anchorX: 0.32, anchorY: 0.5 },
  { at: 0.7477, x: 4000, y: 210, zoom: 0.112, anchorX: 0.32, anchorY: 0.5 },
  { at: 0.7584, x: 4000, y: 210, zoom: 0.118, anchorX: 0.3, anchorY: 0.5 },
  { at: 0.7983, x: 4000, y: 210, zoom: 0.118, anchorX: 0.3, anchorY: 0.5 },
  { at: 0.8089, x: 8300, y: -1300, zoom: 0.78, anchorX: 0.68, anchorY: 0.5 },
  { at: 0.8489, x: 8300, y: -1300, zoom: 0.78, anchorX: 0.68, anchorY: 0.5 },
  { at: 0.8595, x: 8330, y: -1060, zoom: 0.53, anchorX: 0.7, anchorY: 0.5 },
  { at: 0.8995, x: 8330, y: -1060, zoom: 0.53, anchorX: 0.7, anchorY: 0.5 },
  { at: 0.9101, x: 8400, y: 1400, zoom: 0.78, anchorX: 0.68, anchorY: 0.5 },
  { at: 0.95, x: 8400, y: 1400, zoom: 0.78, anchorX: 0.68, anchorY: 0.5 },
  { at: 0.9606, x: 8300, y: 1500, zoom: 0.8, anchorX: 0.68, anchorY: 0.5 },
];

// Medium (tablet, compact laptop): the copy column is wider, so the world is
// framed tighter and sits lower to clear the headline.
const mediumCamera: CameraKeyframe[] = [
  { at: 0, x: 40, y: 0, zoom: 0.82, anchorX: 0.74, anchorY: 0.56 },
  { at: 0.0399, x: 80, y: 0, zoom: 0.82, anchorX: 0.74, anchorY: 0.56 },
  { at: 0.0505, x: 1000, y: 0, zoom: 0.68, anchorX: 0.28, anchorY: 0.56 },
  { at: 0.0905, x: 1040, y: 0, zoom: 0.68, anchorX: 0.28, anchorY: 0.56 },
  { at: 0.1011, x: 1880, y: 0, zoom: 0.54, anchorX: 0.74, anchorY: 0.58 },
  { at: 0.1411, x: 1900, y: 0, zoom: 0.55, anchorX: 0.74, anchorY: 0.58 },
  { at: 0.1517, x: 2700, y: 0, zoom: 0.8, anchorX: 0.28, anchorY: 0.58 },
  { at: 0.1916, x: 2720, y: 0, zoom: 0.8, anchorX: 0.28, anchorY: 0.58 },
  { at: 0.2023, x: 2700, y: 120, zoom: 0.72, anchorX: 0.26, anchorY: 0.56 },
  { at: 0.2422, x: 2700, y: 120, zoom: 0.72, anchorX: 0.26, anchorY: 0.56 },
  { at: 0.2528, x: 3560, y: 0, zoom: 0.72, anchorX: 0.24, anchorY: 0.58 },
  { at: 0.2927, x: 3640, y: 0, zoom: 0.74, anchorX: 0.24, anchorY: 0.58 },
  { at: 0.3033, x: 4190, y: 0, zoom: 0.52, anchorX: 0.725, anchorY: 0.58 },
  { at: 0.3433, x: 4190, y: 0, zoom: 0.52, anchorX: 0.725, anchorY: 0.58 },
  { at: 0.3539, x: 4400, y: 1020, zoom: 0.4, anchorX: 0.28, anchorY: 0.56 },
  { at: 0.3939, x: 4400, y: 1020, zoom: 0.4, anchorX: 0.28, anchorY: 0.56 },
  { at: 0.4045, x: 5330, y: 940, zoom: 0.44, anchorX: 0.28, anchorY: 0.58 },
  { at: 0.4444, x: 5330, y: 940, zoom: 0.44, anchorX: 0.28, anchorY: 0.58 },
  { at: 0.455, x: 6500, y: 1450, zoom: 0.38, anchorX: 0.77, anchorY: 0.56 },
  { at: 0.495, x: 6500, y: 1450, zoom: 0.38, anchorX: 0.77, anchorY: 0.56 },
  { at: 0.5056, x: 6450, y: 1300, zoom: 0.36, anchorX: 0.3, anchorY: 0.58 },
  { at: 0.5455, x: 6450, y: 1300, zoom: 0.36, anchorX: 0.3, anchorY: 0.58 },
  { at: 0.5561, x: 6560, y: 950, zoom: 0.32, anchorX: 0.76, anchorY: 0.58 },
  { at: 0.5961, x: 6560, y: 950, zoom: 0.32, anchorX: 0.76, anchorY: 0.58 },
  { at: 0.6067, x: 7760, y: 1650, zoom: 0.4, anchorX: 0.76, anchorY: 0.58 },
  { at: 0.6467, x: 7760, y: 1650, zoom: 0.4, anchorX: 0.76, anchorY: 0.58 },
  { at: 0.6573, x: 6150, y: 830, zoom: 0.4, anchorX: 0.2, anchorY: 0.55 },
  { at: 0.6972, x: 6150, y: 830, zoom: 0.4, anchorX: 0.2, anchorY: 0.55 },
  { at: 0.7078, x: 4000, y: 200, zoom: 0.0766, anchorX: 0.26, anchorY: 0.6 },
  { at: 0.7477, x: 4000, y: 210, zoom: 0.0797, anchorX: 0.26, anchorY: 0.6 },
  { at: 0.7584, x: 4000, y: 210, zoom: 0.073, anchorX: 0.255, anchorY: 0.6 },
  { at: 0.7983, x: 4000, y: 210, zoom: 0.073, anchorX: 0.255, anchorY: 0.6 },
  { at: 0.8089, x: 8300, y: -1280, zoom: 0.6, anchorX: 0.74, anchorY: 0.58 },
  { at: 0.8489, x: 8300, y: -1280, zoom: 0.6, anchorX: 0.74, anchorY: 0.58 },
  { at: 0.8595, x: 8330, y: -1060, zoom: 0.34, anchorX: 0.72, anchorY: 0.58 },
  { at: 0.8995, x: 8330, y: -1060, zoom: 0.34, anchorX: 0.72, anchorY: 0.58 },
  { at: 0.9101, x: 8300, y: 1600, zoom: 0.56, anchorX: 0.74, anchorY: 0.58 },
  { at: 0.95, x: 8300, y: 1600, zoom: 0.56, anchorX: 0.74, anchorY: 0.58 },
  { at: 0.9606, x: 8300, y: 1500, zoom: 0.62, anchorX: 0.74, anchorY: 0.58 },
];

// Compact (phones): portrait composition. Copy owns the upper band, the world
// lives in the lower band, centred, and the camera never pans with the copy.
const compactCamera: CameraKeyframe[] = [
  { at: 0, x: 0, y: 0, zoom: 0.62, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.0399, x: 60, y: 0, zoom: 0.62, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.0505, x: 980, y: 0, zoom: 0.5, anchorX: 0.5, anchorY: 0.79 },
  { at: 0.0905, x: 1000, y: 0, zoom: 0.5, anchorX: 0.5, anchorY: 0.79 },
  { at: 0.1011, x: 1835, y: 0, zoom: 0.37, anchorX: 0.5, anchorY: 0.835 },
  { at: 0.1411, x: 1835, y: 0, zoom: 0.37, anchorX: 0.5, anchorY: 0.835 },
  { at: 0.1517, x: 2700, y: 0, zoom: 0.62, anchorX: 0.5, anchorY: 0.8 },
  { at: 0.1916, x: 2700, y: 0, zoom: 0.62, anchorX: 0.5, anchorY: 0.8 },
  { at: 0.2023, x: 2700, y: 60, zoom: 0.48, anchorX: 0.5, anchorY: 0.785 },
  { at: 0.2422, x: 2700, y: 60, zoom: 0.48, anchorX: 0.5, anchorY: 0.785 },
  { at: 0.2528, x: 3620, y: 0, zoom: 0.5, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.2927, x: 3690, y: 0, zoom: 0.5, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.3033, x: 4420, y: 0, zoom: 0.46, anchorX: 0.5, anchorY: 0.785 },
  { at: 0.3433, x: 4420, y: 0, zoom: 0.46, anchorX: 0.5, anchorY: 0.785 },
  { at: 0.3539, x: 4300, y: 1040, zoom: 0.26, anchorX: 0.5, anchorY: 0.78 },
  { at: 0.3939, x: 4300, y: 1040, zoom: 0.26, anchorX: 0.5, anchorY: 0.78 },
  { at: 0.4045, x: 5350, y: 1140, zoom: 0.225, anchorX: 0.5, anchorY: 0.845 },
  { at: 0.4444, x: 5350, y: 1140, zoom: 0.225, anchorX: 0.5, anchorY: 0.845 },
  { at: 0.455, x: 6450, y: 1560, zoom: 0.23, anchorX: 0.5, anchorY: 0.86 },
  { at: 0.495, x: 6450, y: 1560, zoom: 0.23, anchorX: 0.5, anchorY: 0.86 },
  { at: 0.5056, x: 6480, y: 1000, zoom: 0.24, anchorX: 0.5, anchorY: 0.71 },
  { at: 0.5455, x: 6480, y: 1000, zoom: 0.24, anchorX: 0.5, anchorY: 0.71 },
  { at: 0.5561, x: 6540, y: 1115, zoom: 0.18, anchorX: 0.5, anchorY: 0.85 },
  { at: 0.5961, x: 6540, y: 1115, zoom: 0.18, anchorX: 0.5, anchorY: 0.85 },
  { at: 0.6067, x: 7760, y: 1610, zoom: 0.2, anchorX: 0.5, anchorY: 0.8 },
  { at: 0.6467, x: 7760, y: 1610, zoom: 0.2, anchorX: 0.5, anchorY: 0.8 },
  { at: 0.6573, x: 6300, y: 845, zoom: 0.175, anchorX: 0.5, anchorY: 0.87 },
  { at: 0.6972, x: 6300, y: 845, zoom: 0.175, anchorX: 0.5, anchorY: 0.87 },
  { at: 0.7078, x: 4000, y: 200, zoom: 0.05, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.7477, x: 4000, y: 210, zoom: 0.053, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.7584, x: 4000, y: 0, zoom: 0.054, anchorX: 0.5, anchorY: 0.8 },
  { at: 0.7983, x: 4000, y: 0, zoom: 0.054, anchorX: 0.5, anchorY: 0.8 },
  { at: 0.8089, x: 8300, y: -1355, zoom: 0.4, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.8489, x: 8300, y: -1355, zoom: 0.4, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.8595, x: 8300, y: -1040, zoom: 0.22, anchorX: 0.5, anchorY: 0.82 },
  { at: 0.8995, x: 8300, y: -1040, zoom: 0.22, anchorX: 0.5, anchorY: 0.82 },
  { at: 0.9101, x: 8300, y: 1500, zoom: 0.36, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.95, x: 8300, y: 1500, zoom: 0.36, anchorX: 0.5, anchorY: 0.74 },
  { at: 0.9606, x: 8300, y: 1500, zoom: 0.42, anchorX: 0.5, anchorY: 0.74 },
];

const cameras: Record<Viewport, CameraKeyframe[]> = {
  wide: wideCamera,
  medium: mediumCamera,
  compact: compactCamera,
};

type Track = { times: number[]; values: Record<keyof Camera, number[]> };
const tracks = new Map<Viewport, Track>();

function trackFor(viewport: Viewport): Track {
  let track = tracks.get(viewport);
  if (!track) {
    const keyframes = cameras[viewport];
    track = {
      times: keyframes.map((key) => key.at),
      values: {
        x: keyframes.map((key) => key.x),
        y: keyframes.map((key) => key.y),
        zoom: keyframes.map((key) => key.zoom),
        anchorX: keyframes.map((key) => key.anchorX),
        anchorY: keyframes.map((key) => key.anchorY),
      },
    };
    tracks.set(viewport, track);
  }
  return track;
}

export function interpolateCamera(progress: number, viewport: Viewport): Camera {
  const track = trackFor(viewport);
  return {
    x: monotoneCubic(track.times, track.values.x, progress),
    y: monotoneCubic(track.times, track.values.y, progress),
    zoom: monotoneCubic(track.times, track.values.zoom, progress),
    anchorX: monotoneCubic(track.times, track.values.anchorX, progress),
    anchorY: monotoneCubic(track.times, track.values.anchorY, progress),
  };
}

// Tonal arc: the acts alternate. Night for the client and the network, paper
// for compute, night for storage, paper for the agents, night for the binary
// and its tenants, the gold wash for the payoff on your machines.
export function paperMixFor(progress: number) {
  // Every crossfade sits in the middle third of the gap between two
  // chapters, so no copy is read while the background crosses and the
  // frame spends only a short beat between the two tones. The ink and the
  // chrome swap in one step at the midpoint (see paletteFor), so nothing
  // washes to grey on grey.
  const computeIn = smoothstep(range(progress, 0.1446, 0.1481));
  const computeOut = smoothstep(range(progress, 0.2458, 0.2492));
  const agentsIn = smoothstep(range(progress, 0.448, 0.4514));
  const agentsOut = smoothstep(range(progress, 0.7008, 0.7042));
  return computeIn * (1 - computeOut) + agentsIn * (1 - agentsOut);
}

export function washMixFor(progress: number) {
  return smoothstep(range(progress, 0.8018, 0.8054));
}

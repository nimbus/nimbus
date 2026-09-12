import type { Metadata } from 'next';
import type { TableOfContents } from 'fumadocs-core/toc';
import { Card, Cards } from 'fumadocs-ui/components/card';
import { Tab, Tabs } from 'fumadocs-ui/components/tabs';
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
} from 'fumadocs-ui/layouts/docs/page';

import { Code } from '@/components/code';

export const metadata: Metadata = {
  title: 'Documentation',
  description:
    'Every path through the Nimbus documentation: quickstarts, adapter guides, agent sandboxes, operations, and the reference.',
  alternates: { canonical: '/docs/' },
};

// Every MDX page hands `DocsPage` a table of contents extracted from its
// headings. This page has no MDX pipeline behind it, so it carries the same
// list by hand; the ids below are the ones the headings declare.
const TOC: TableOfContents = [
  {
    title: 'The whole backend in one command',
    url: '#the-whole-backend-in-one-command',
    depth: 2,
  },
  {
    title: 'One binary, your choice of client',
    url: '#one-binary-your-choice-of-client',
    depth: 2,
  },
  {
    title: 'Production is one more verb',
    url: '#production-is-one-more-verb',
    depth: 2,
  },
  { title: 'Built for agents', url: '#built-for-agents', depth: 2 },
  {
    title: 'Scale with tenants and services',
    url: '#scale-with-tenants-and-services',
    depth: 2,
  },
  { title: 'Find your path', url: '#find-your-path', depth: 2 },
  { title: 'By surface', url: '#by-surface', depth: 2 },
];

// The seven front doors, in the order a reader is most likely to want them:
// the five drop-in protocols, then the native API, then the sandbox surface.
const ADAPTERS = [
  'Convex',
  'Firebase',
  'Cloud Functions',
  'MongoDB',
  'DynamoDB',
  'HTTP API',
  'Sandboxes',
];

const CONVEX_TS = `// convex/messages.ts — server-side functions with reactive queries
import { query, mutation } from "./_generated/server";
import { v } from "convex/values";

export const list = query({
  args: {},
  handler: async (ctx) => await ctx.db.query("messages").take(50),
});

export const send = mutation({
  args: { author: v.string(), body: v.string() },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});`;

const FIREBASE_TS = `// Your Firebase code, unchanged — stock imports, served by Nimbus
import { initializeApp } from "firebase/app";
import {
  addDoc,
  collection,
  connectFirestoreEmulator,
  getFirestore,
  onSnapshot,
} from "firebase/firestore";

const db = getFirestore(initializeApp({ projectId: "demo" }));
connectFirestoreEmulator(db, "127.0.0.1", 3210);

const messages = collection(db, "messages");
await addDoc(messages, { body: "hello from nimbus" });
onSnapshot(messages, (live) => console.log("live size", live.size));`;

const FUNCTIONS_TS = `// functions/src/index.ts — Firebase-style HTTP + Firestore triggers
import { onRequest } from "firebase-functions/v2/https";
import { onDocumentCreated } from "firebase-functions/v2/firestore";

export const hello = onRequest(async (req, res) => {
  res.json({ message: "Hello from Nimbus Cloud Functions!" });
});

export const onMessageCreated = onDocumentCreated(
  "messages/{messageId}",
  async (event) => {
    console.log("New message:", event.data?.data());
  },
);`;

const MONGO_JS = `// Official MongoDB driver, unchanged — Nimbus speaks the wire protocol
import { MongoClient } from "mongodb";

const client = new MongoClient(process.env.NIMBUS_MONGODB_URL);
await client.connect();

const messages = client.db("myapp").collection("messages");
await messages.insertOne({ author: "Ada", body: "Hello from Nimbus" });
const docs = await messages.find({ author: "Ada" }).toArray();`;

const DYNAMO_JS = `// Official AWS SDK v3, unchanged — pointed at Nimbus's endpoint
import { DynamoDBClient, PutItemCommand, GetItemCommand } from "@aws-sdk/client-dynamodb";

const client = new DynamoDBClient({
  endpoint: process.env.NIMBUS_DYNAMODB_ENDPOINT,
  region: "us-east-1",
  credentials: {
    accessKeyId: process.env.NIMBUS_DYNAMODB_ACCESS_KEY_ID,
    secretAccessKey: process.env.NIMBUS_DYNAMODB_SECRET_ACCESS_KEY,
  },
});

await client.send(new PutItemCommand({
  TableName: "orders",
  Item: { pk: { S: "order-1" }, total: { N: "42" } },
}));
const { Item } = await client.send(new GetItemCommand({
  TableName: "orders",
  Key: { pk: { S: "order-1" } },
}));`;

const NATIVE_SH = `# the same engine over plain REST
curl -s -X POST http://localhost:8080/api/tenants \\
  -H "Authorization: Bearer $NIMBUS_TOKEN" \\
  -H "Content-Type: application/json" -d '{"id": "demo"}'

curl -s -X POST http://localhost:8080/api/tenants/demo/documents \\
  -H "Authorization: Bearer $NIMBUS_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{"table": "messages", "fields": {"text": "hello world"}}'`;

const SANDBOX_TS = `// @nimbus/nimbus SDK — give an agent its own isolated process
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({
  endpoint: "http://localhost:8080",
  tenantId: "demo",
  token: process.env.NIMBUS_TOKEN,
});

const sandbox = await nimbus.sandboxes.create({
  profile: "worker",
  spec: {
    owner: { kind: "standalone", displayName: "agent-task" },
    backend: "container",
    root: {
      kind: "oci_image",
      source: { kind: "reference", reference: "docker.io/library/node:22-alpine" },
    },
    process: { argv: ["node", "-e", "setInterval(() => {}, 1000)"] },
  },
});`;

// The landing page of the documentation. The scroll-driven pitch is the site
// home at `/`; this page is the written entrance to the same story — what the
// one command does, which client you point at it, and where each reader goes
// next.
export default function Page() {
  return (
    <DocsPage toc={TOC}>
      <DocsTitle>Documentation</DocsTitle>
      <DocsDescription>
        Nimbus is one binary that speaks Convex, Firestore, Cloud Functions,
        MongoDB, and DynamoDB. Start where your work starts.
      </DocsDescription>
      <DocsBody>
        <h2 id="the-whole-backend-in-one-command">
          The whole backend in one command
        </h2>
        <Code lang="bash" code="nimbus dev" />
        <p>
          Run it in your project. One process — a single Rust binary — serves
          storage, TypeScript functions, realtime subscriptions, scheduling,
          and agent sandboxes, and reloads your functions as you save. No
          Compose file, no sidecar services, no managed account.
        </p>
        <p>Install it first:</p>
        <Code
          lang="bash"
          code={
            'brew trust --cask nimbus/tap/nimbus\nbrew install --cask nimbus/tap/nimbus'
          }
        />
        <p>
          Other platforms ship via the install script and release binaries — see
          the{' '}
          <a href="https://github.com/nimbus/nimbus#install">install options</a>.
          The <a href="/get-started/quickstart/">5-minute quickstart</a>{' '}
          scaffolds an app around <code>nimbus dev</code>; the{' '}
          <a href="/get-started/self-host/">self-host quickstart</a> runs the
          same binary as a long-lived server with <code>nimbus start</code>.
        </p>

        <h2 id="one-binary-your-choice-of-client">
          One binary, your choice of client
        </h2>
        <Tabs items={ADAPTERS}>
          <Tab value="Convex">
            <Code
              lang="bash"
              code={'# migrate an existing Convex app\nnimbus dev'}
            />
            <Code lang="typescript" code={CONVEX_TS} />
            <p>
              One command in your project root: <code>nimbus dev</code> detects
              the <code>convex/</code> directory and repoints the{' '}
              <code>convex</code> dependency at the package inside the binary.
              See <a href="/developers/convex/">the Convex guide</a> — or
              scaffold a fresh app in the{' '}
              <a href="/get-started/quickstart/">5-minute quickstart</a>.
            </p>
          </Tab>
          <Tab value="Firebase">
            <Code
              lang="bash"
              code={'# migrate an existing Firebase app\nnimbus dev'}
            />
            <Code lang="typescript" code={FIREBASE_TS} />
            <p>
              Firestore is Firebase&apos;s database — Nimbus speaks its wire
              protocol and ships a drop-in <code>firebase</code> package inside
              the binary. <code>nimbus dev</code> detects the{' '}
              <code>firebase</code> dependency, checks every Firebase import in
              your sources is covered, and rewires the dependency at the
              drop-in — your imports stay exactly as they are. See{' '}
              <a href="/developers/firebase/">the Firestore guide</a>.
            </p>
          </Tab>
          <Tab value="Cloud Functions">
            <Code
              lang="bash"
              code={'# migrate an existing Functions project\nnimbus dev'}
            />
            <Code lang="typescript" code={FUNCTIONS_TS} />
            <p>
              Run <code>nimbus dev</code> in your existing Functions project —{' '}
              <code>firebase.json</code> marks the root. See{' '}
              <a href="/developers/cloud-functions/">
                the Cloud Functions guide
              </a>{' '}
              for the project layout and deploy flow.
            </p>
          </Tab>
          <Tab value="MongoDB">
            <Code
              lang="bash"
              code={'# no migration — use your existing driver\nnimbus dev'}
            />
            <Code lang="javascript" code={MONGO_JS} />
            <p>
              <code>nimbus dev</code> sees the <code>mongodb</code> dependency
              and writes <code>NIMBUS_MONGODB_URL</code> — the local endpoint
              plus generated credentials — to your app&apos;s{' '}
              <code>.env.local</code>. See{' '}
              <a href="/developers/mongodb/">the MongoDB guide</a> for
              credential setup and the supported command surface.
            </p>
          </Tab>
          <Tab value="DynamoDB">
            <Code
              lang="bash"
              code={'# no migration — use your existing AWS SDK\nnimbus dev'}
            />
            <Code lang="javascript" code={DYNAMO_JS} />
            <p>
              <code>nimbus dev</code> sees the{' '}
              <code>@aws-sdk/client-dynamodb</code> dependency and writes the
              endpoint and a generated access key to <code>.env.local</code> as{' '}
              <code>NIMBUS_DYNAMODB_*</code> keys. See{' '}
              <a href="/developers/dynamodb/">the DynamoDB guide</a> for
              registered-key credentials and the supported operation surface.
            </p>
          </Tab>
          <Tab value="HTTP API">
            <Code
              lang="bash"
              code={'# native API — nothing to migrate\nnimbus start'}
            />
            <Code lang="bash" code={NATIVE_SH} />
            <p>
              See the <a href="/developers/native/">native API guide</a> and the{' '}
              <a href="/get-started/self-host/">self-host quickstart</a> for the
              token setup.
            </p>
          </Tab>
          <Tab value="Sandboxes">
            <Code
              lang="bash"
              code={
                '# isolated compute for agents — same binary\nnimbus start'
              }
            />
            <Code lang="typescript" code={SANDBOX_TS} />
            <p>
              Sandboxes, services, and sessions live in the same binary as your
              data — no separate sandbox vendor. Execution runs on Linux hosts;{' '}
              <code>nimbus machine</code> hosts them on macOS. Start with the{' '}
              <a href="/agents/sandbox-quickstart/">
                agent sandbox quickstart
              </a>
              .
            </p>
          </Tab>
        </Tabs>
        <p>
          Every tab is the same engine and the same data — five drop-in
          protocols, the native API, and the sandbox surface. See{' '}
          <a href="/developers/">Developers</a> for the adapters side by side.
        </p>

        <h2 id="production-is-one-more-verb">Production is one more verb</h2>
        <Code
          lang="bash"
          code={`nimbus dev      # laptop: watch files, run codegen, reload functions
nimbus deploy   # production: package, validate, activate atomically`}
        />
        <p>
          You have already run the first one. Both are the same binary with the
          same configuration surface — no Terraform, no Helm chart, no
          Dockerfile between them. <code>nimbus deploy</code> packages your
          functions, previews a diff with <code>--dry-run</code>, and activates
          the new generation atomically on whatever server you point it at.{' '}
          <a href="/get-started/deploy/">Deploy to production</a> walks the
          whole flow.
        </p>

        <h2 id="built-for-agents">Built for agents</h2>
        <p>
          Agents need a backend they can <strong>run</strong> (one binary,
          started in one command), <strong>inspect</strong> (docs shipped as{' '}
          <a href="/llms.txt">llms.txt</a>, structured errors, and a{' '}
          <a href="/reference/current-capabilities/">
            plain table of what works today
          </a>
          ), and get <strong>isolated compute</strong> from. That last part is
          first-class: <strong>sandboxes</strong> are isolated worlds addressed
          by id, <strong>services</strong> are named workloads other code can
          depend on, and <strong>sessions</strong> are expiring leases for
          stdio, file, and browser-control access — all managed through the{' '}
          <code>@nimbus/nimbus</code> SDK against the same server that holds
          your data.
        </p>
        <Cards>
          <Card
            href="/agents/sandbox-quickstart/"
            title="Agent sandbox quickstart"
            description="Create a sandbox, lease a session, tear it down — about 5 minutes."
          />
          <Card
            href="/agents/"
            title="Agents overview"
            description="Sandboxes, services, and sessions — the guides and the design rationale."
          />
        </Cards>

        <h2 id="scale-with-tenants-and-services">
          Scale with tenants and services
        </h2>
        <p>
          The unit of isolation is also the unit of growth. Every tenant gets
          its own storage namespace, its own write queue, and its own function
          budgets — load on one tenant cannot degrade another, and adding load
          means adding tenants. Heavier compute fans out into sandbox-backed
          services that Nimbus launches and supervises next to your data, and
          storage can move onto PostgreSQL, MySQL, or libSQL infrastructure
          that grows independently. One deployment is one process today — no
          cluster mode; <a href="/concepts/scaling/">scaling</a> spells out the
          limits and the partitioning pattern for growing past one machine.
          Self-hosted, source-available, and protocol-portable by design: no
          lock-in.
        </p>

        <h2 id="find-your-path">Find your path</h2>
        <Cards>
          <Card
            href="/get-started/quickstart/"
            title="Developer quickstart"
            description="Run nimbus dev, write a function, and call it from an app."
          />
          <Card
            href="/get-started/self-host/"
            title="Self-host quickstart"
            description="Put one binary on one machine and keep it running."
          />
          <Card
            href="/agents/"
            title="Agent sandboxes"
            description="Give an agent a filesystem, a shell, and a network policy."
          />
          <Card
            href="/get-started/deploy/"
            title="Deploy to production"
            description="Promote the same project from nimbus dev to nimbus deploy."
          />
          <Card
            href="/get-started/from-convex/"
            title="Coming from Convex"
            description="What carries over, what changes, and what to check first."
          />
          <Card
            href="/concepts/"
            title="Concepts and architecture"
            description="How the engine, the adapters, and the runtime fit together."
          />
        </Cards>

        <h2 id="by-surface">By surface</h2>
        <Cards>
          <Card
            href="/developers/"
            title="Developers"
            description="Build on Nimbus through the adapter your client already speaks."
          />
          <Card
            href="/operators/"
            title="Operators"
            description="Deploy, secure, back up, scale, and observe a Nimbus server."
          />
          <Card
            href="/reference/"
            title="Reference"
            description="The CLI, the configuration, the SDKs, and per-adapter coverage."
          />
        </Cards>
      </DocsBody>
    </DocsPage>
  );
}

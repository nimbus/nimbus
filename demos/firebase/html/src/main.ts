import "./app.css";

import { deleteApp, initializeApp, type FirebaseApp } from "@nimbus/firebase/app";
import {
  addDoc,
  arrayUnion,
  collection,
  connectFirestoreEmulator,
  deleteDoc,
  doc,
  getDocs,
  increment,
  initializeFirestore,
  limit,
  onSnapshot,
  orderBy,
  query,
  runTransaction,
  serverTimestamp,
  terminate,
  writeBatch,
  type DocumentData,
  type Firestore,
  type FirestoreUnaryTransport,
} from "@nimbus/firebase/firestore";

const COLLECTION_NAME = "demoMessages";
const LOCAL_STORAGE_KEY = "nimbus.firebase.demo.settings";

type DemoSettings = {
  author: string;
  baseUrl: string;
  message: string;
  tag: string;
  unaryTransport: FirestoreUnaryTransport;
};

type DemoState = {
  app: FirebaseApp | null;
  firestore: Firestore | null;
  unsubscribe: (() => void) | null;
  watchStatus: "idle" | "watching" | "stopped";
  lastSnapshotCount: number;
  settings: DemoSettings;
};

type FeedMessage = {
  id: string;
  author?: string;
  body?: string;
  tags?: string[];
  likes?: number;
  createdAt?: string;
  updatedAt?: string;
};

const defaultSettings: DemoSettings = {
  author: "Nimbus Demo",
  baseUrl: "http://127.0.0.1:8080",
  message: "Hello from @nimbus/firebase",
  tag: "demo",
  unaryTransport: "rest",
};

const state: DemoState = {
  app: null,
  firestore: null,
  unsubscribe: null,
  watchStatus: "idle",
  lastSnapshotCount: 0,
  settings: loadSettings(),
};

const root = document.querySelector<HTMLDivElement>("#app");
if (!root) {
  throw new Error("Firebase demo root element is missing.");
}

function mustElement<T>(value: T | null, name: string): T {
  if (!value) {
    throw new Error(`Missing demo element: ${name}`);
  }
  return value;
}

root.innerHTML = `
  <main class="shell">
    <section class="hero">
      <div>
        <span class="eyebrow">Firebase / Firestore demo</span>
        <h1>Exercise the first-party SDK over REST, gRPC-Web, and live Listen.</h1>
        <p class="lede">
          This demo talks to a local Nimbus server with <code>@nimbus/firebase</code>,
          using REST or gRPC-Web for unary calls and the browser WebSocket Listen path
          for live query updates. It is the concrete runnable reference for the current
          supported Firebase data-path tier.
        </p>
      </div>
      <aside class="hero-card">
        <div class="hero-card-grid">
          <div class="status-pill">Unary transport <strong id="transport-status">rest</strong></div>
          <div class="status-pill">Watch status <strong id="watch-status">idle</strong></div>
          <div class="status-pill">Last snapshot <strong id="snapshot-count">0 docs</strong></div>
          <pre id="connection-summary"></pre>
        </div>
      </aside>
    </section>

    <section class="grid">
      <article class="panel panel-small">
        <div class="panel-header">
          <div>
            <h2>Connection</h2>
            <p>Point the demo at a local Nimbus server and choose the unary transport.</p>
          </div>
        </div>
        <div class="field-grid">
          <label>
            Nimbus base URL
            <input id="base-url" type="url" />
          </label>
          <label>
            Unary transport
            <select id="transport-select">
              <option value="rest">REST</option>
              <option value="grpc-web">gRPC-Web</option>
            </select>
          </label>
        </div>
        <div class="button-row">
          <button id="connect-button" class="button accent">Reconnect</button>
          <button id="refresh-button" class="button ghost">Refresh once</button>
          <button id="watch-button" class="button">Start watch</button>
          <button id="stop-watch-button" class="button ghost">Stop watch</button>
        </div>
        <p class="transport-note">
          Unary operations follow the selected transport. Live updates always use the documented
          WebSocket <code>Listen</code> bridge.
        </p>
      </article>

      <article class="panel panel-small">
        <div class="panel-header">
          <div>
            <h2>Compose message</h2>
            <p>Add a document with server timestamps so the feed updates immediately.</p>
          </div>
        </div>
        <div class="field-grid">
          <label>
            Author
            <input id="author-input" type="text" />
          </label>
          <label>
            Message
            <textarea id="message-input"></textarea>
          </label>
          <label>
            Tag
            <input id="tag-input" type="text" />
          </label>
        </div>
        <div class="button-row">
          <button id="send-button" class="button warm">addDoc()</button>
          <button id="batch-button" class="button">writeBatch()</button>
        </div>
      </article>

      <article class="panel panel-small">
        <div class="panel-header">
          <div>
            <h2>Mutate live data</h2>
            <p>Run one transaction and one delete path against the newest message.</p>
          </div>
        </div>
        <div class="button-row">
          <button id="like-button" class="button accent">runTransaction()</button>
          <button id="delete-button" class="button ghost">delete latest</button>
        </div>
        <div class="meta-list">
          <div class="meta-row">
            <span>Collection</span>
            <strong>${COLLECTION_NAME}</strong>
          </div>
          <div class="meta-row">
            <span>Demo writes</span>
            <strong>serverTimestamp + increment + arrayUnion</strong>
          </div>
        </div>
      </article>

      <article class="panel panel-wide">
        <div class="panel-header">
          <div>
            <h2>Live feed</h2>
            <p>Ordered by <code>createdAt desc</code> with a <code>limit(12)</code> query.</p>
          </div>
        </div>
        <div id="feed" class="feed"></div>
      </article>

      <article class="panel panel-wide">
        <div class="panel-header">
          <div>
            <h2>Activity log</h2>
            <p>Connection setup, write results, query refreshes, and watch events.</p>
          </div>
        </div>
        <pre id="log" class="log"></pre>
      </article>
    </section>
  </main>
`;

const elements = {
  authorInput: mustElement(document.querySelector<HTMLInputElement>("#author-input"), "authorInput"),
  baseUrl: mustElement(document.querySelector<HTMLInputElement>("#base-url"), "baseUrl"),
  batchButton: mustElement(document.querySelector<HTMLButtonElement>("#batch-button"), "batchButton"),
  connectButton: mustElement(
    document.querySelector<HTMLButtonElement>("#connect-button"),
    "connectButton",
  ),
  connectionSummary: mustElement(
    document.querySelector<HTMLElement>("#connection-summary"),
    "connectionSummary",
  ),
  deleteButton: mustElement(
    document.querySelector<HTMLButtonElement>("#delete-button"),
    "deleteButton",
  ),
  feed: mustElement(document.querySelector<HTMLElement>("#feed"), "feed"),
  likeButton: mustElement(document.querySelector<HTMLButtonElement>("#like-button"), "likeButton"),
  log: mustElement(document.querySelector<HTMLElement>("#log"), "log"),
  messageInput: mustElement(
    document.querySelector<HTMLTextAreaElement>("#message-input"),
    "messageInput",
  ),
  refreshButton: mustElement(
    document.querySelector<HTMLButtonElement>("#refresh-button"),
    "refreshButton",
  ),
  sendButton: mustElement(document.querySelector<HTMLButtonElement>("#send-button"), "sendButton"),
  snapshotCount: mustElement(
    document.querySelector<HTMLElement>("#snapshot-count"),
    "snapshotCount",
  ),
  stopWatchButton: mustElement(
    document.querySelector<HTMLButtonElement>("#stop-watch-button"),
    "stopWatchButton",
  ),
  tagInput: mustElement(document.querySelector<HTMLInputElement>("#tag-input"), "tagInput"),
  transportSelect: mustElement(
    document.querySelector<HTMLSelectElement>("#transport-select"),
    "transportSelect",
  ),
  transportStatus: mustElement(
    document.querySelector<HTMLElement>("#transport-status"),
    "transportStatus",
  ),
  watchButton: mustElement(document.querySelector<HTMLButtonElement>("#watch-button"), "watchButton"),
  watchStatus: mustElement(document.querySelector<HTMLElement>("#watch-status"), "watchStatus"),
};

hydrateInputs();
renderConnectionSummary();
renderWatchStatus();
renderFeed([]);
log(
  "Demo ready. Start a local server with `npm run firebase:server:html`, then connect and watch the feed.",
);

elements.baseUrl.addEventListener("change", () => {
  state.settings.baseUrl = normalizeBaseUrl(elements.baseUrl.value);
  persistSettings();
  renderConnectionSummary();
});

elements.transportSelect.addEventListener("change", () => {
  state.settings.unaryTransport = elements.transportSelect.value as FirestoreUnaryTransport;
  persistSettings();
  renderConnectionSummary();
});

elements.authorInput.addEventListener("change", () => {
  state.settings.author = elements.authorInput.value.trim() || defaultSettings.author;
  persistSettings();
});

elements.messageInput.addEventListener("change", () => {
  state.settings.message = elements.messageInput.value.trim() || defaultSettings.message;
  persistSettings();
});

elements.tagInput.addEventListener("change", () => {
  state.settings.tag = elements.tagInput.value.trim() || defaultSettings.tag;
  persistSettings();
});

elements.connectButton.addEventListener("click", () => {
  void withTask("reconnect firestore", async () => {
    await rebuildFirestore();
    await refreshFeed();
  }).catch(() => {});
});

elements.refreshButton.addEventListener("click", () => {
  void withTask("refresh query", async () => {
    await ensureFirestore();
    await refreshFeed();
  }).catch(() => {});
});

elements.watchButton.addEventListener("click", () => {
  void withTask("start watch", async () => {
    await ensureFirestore();
    startWatch();
  }).catch(() => {});
});

elements.stopWatchButton.addEventListener("click", () => {
  stopWatch("Watch stopped by user.");
});

elements.sendButton.addEventListener("click", () => {
  void withTask("add message", async () => {
    const messages = messagesCollection(await ensureFirestore());
    const payload = {
      author: readAuthor(),
      body: readMessageBody(),
      createdAt: serverTimestamp(),
      likes: 0,
      tags: [readTag()],
      updatedAt: serverTimestamp(),
    };
    const reference = await addDoc(messages, payload);
    log(`addDoc() wrote ${reference.id}`);
    elements.messageInput.value = "Another live update from the Firebase demo";
    state.settings.message = elements.messageInput.value;
    persistSettings();
  }).catch(() => {});
});

elements.batchButton.addEventListener("click", () => {
  void withTask("seed write batch", async () => {
    const firestore = await ensureFirestore();
    const messages = messagesCollection(firestore);
    const batch = writeBatch(firestore);
    const tag = readTag();
    const first = doc(messages, `seed-${Date.now()}-a`);
    const second = doc(messages, `seed-${Date.now()}-b`);
    batch.set(first, {
      author: "Batch seed",
      body: "First document written via writeBatch()",
      createdAt: serverTimestamp(),
      likes: 1,
      tags: [tag, "batch"],
      updatedAt: serverTimestamp(),
    });
    batch.set(second, {
      author: "Batch seed",
      body: "Second document written via writeBatch()",
      createdAt: serverTimestamp(),
      likes: 2,
      tags: [tag, "batch"],
      updatedAt: serverTimestamp(),
    });
    await batch.commit();
    log("writeBatch() committed two demo documents.");
  }).catch(() => {});
});

elements.likeButton.addEventListener("click", () => {
  void withTask("transactional like", async () => {
    const firestore = await ensureFirestore();
    const updatedId = await runTransaction(firestore, async (transaction) => {
      const snapshot = await transaction.get(messagesQuery(firestore));
      const latest = snapshot.docs[0];
      if (!latest) {
        throw new Error("No demo message exists yet.");
      }
      transaction.update(latest.ref, {
        likes: increment(1),
        tags: arrayUnion("liked"),
        updatedAt: serverTimestamp(),
      });
      return latest.id;
    });
    log(`runTransaction() liked ${updatedId}.`);
  }).catch(() => {});
});

elements.deleteButton.addEventListener("click", () => {
  void withTask("delete latest message", async () => {
    const firestore = await ensureFirestore();
    const snapshot = await getDocs(messagesQuery(firestore));
    const latest = snapshot.docs[0];
    if (!latest) {
      throw new Error("No demo message exists yet.");
    }
    await deleteDoc(latest.ref);
    log(`deleteDoc() removed ${latest.id}.`);
  }).catch(() => {});
});

void withTask("initial connection", async () => {
  await rebuildFirestore();
  await refreshFeed();
  startWatch();
}).catch(() => {});

function messagesCollection(firestore: Firestore) {
  return collection(firestore, COLLECTION_NAME);
}

function messagesQuery(firestore: Firestore) {
  return query(messagesCollection(firestore), orderBy("createdAt", "desc"), limit(12));
}

async function ensureFirestore(): Promise<Firestore> {
  if (state.firestore) {
    return state.firestore;
  }
  await rebuildFirestore();
  if (!state.firestore) {
    throw new Error("Firestore connection did not initialize.");
  }
  return state.firestore;
}

async function rebuildFirestore(): Promise<void> {
  stopWatch("Restarting watch for a new connection.");
  if (state.firestore) {
    await terminate(state.firestore);
    state.firestore = null;
  }
  if (state.app) {
    await deleteApp(state.app);
    state.app = null;
  }

  const app = initializeApp(
    {
      apiKey: "nimbus-demo",
      projectId: "nimbus-demo",
    },
    { name: `firebase-demo-${Date.now()}` },
  );
  const firestore = initializeFirestore(app, {
    experimentalUnaryTransport: state.settings.unaryTransport,
  });
  const target = emulatorTarget(state.settings.baseUrl);
  connectFirestoreEmulator(firestore, target.host, target.port);

  state.app = app;
  state.firestore = firestore;
  renderConnectionSummary();
  log(
    `Connected to ${state.settings.baseUrl} using ${state.settings.unaryTransport.toUpperCase()} unary calls.`,
  );
}

async function refreshFeed(): Promise<void> {
  const firestore = await ensureFirestore();
  const snapshot = await getDocs(messagesQuery(firestore));
  const documents = snapshot.docs.map((document) => normalizeFeedMessage(document.id, document.data()));
  state.lastSnapshotCount = documents.length;
  renderFeed(documents);
  renderWatchStatus();
  log(`getDocs() loaded ${documents.length} document(s).`);
}

function startWatch(): void {
  if (!state.firestore) {
    throw new Error("Firestore is not connected.");
  }
  stopWatch();
  state.unsubscribe = onSnapshot(messagesQuery(state.firestore), (snapshot) => {
    const documents = snapshot.docs.map((document) =>
      normalizeFeedMessage(document.id, document.data()),
    );
    state.lastSnapshotCount = documents.length;
    state.watchStatus = "watching";
    renderFeed(documents);
    renderWatchStatus();
    log(`onSnapshot() pushed ${documents.length} document(s).`);
  });
  state.watchStatus = "watching";
  renderWatchStatus();
  log("Live watch started.");
}

function stopWatch(reason?: string): void {
  if (state.unsubscribe) {
    state.unsubscribe();
    state.unsubscribe = null;
  }
  state.watchStatus = "stopped";
  renderWatchStatus();
  if (reason) {
    log(reason);
  }
}

function normalizeFeedMessage(id: string, data: DocumentData): FeedMessage {
  return {
    id,
    author: typeof data.author === "string" ? data.author : undefined,
    body: typeof data.body === "string" ? data.body : undefined,
    tags: Array.isArray(data.tags)
      ? data.tags.filter((entry): entry is string => typeof entry === "string")
      : undefined,
    likes: typeof data.likes === "number" ? data.likes : undefined,
    createdAt: typeof data.createdAt === "string" ? data.createdAt : undefined,
    updatedAt: typeof data.updatedAt === "string" ? data.updatedAt : undefined,
  };
}

function renderFeed(documents: FeedMessage[]): void {
  if (documents.length === 0) {
    elements.feed.innerHTML = `
      <div class="empty-state">
        No demo messages yet. Add one with <code>addDoc()</code> or seed two with
        <code>writeBatch()</code> to watch the query update live.
      </div>
    `;
    return;
  }

  elements.feed.innerHTML = documents
    .map(
      (document) => `
        <article class="feed-card">
          <header>
            <div>
              <h3>${escapeHtml(document.author ?? "Unknown author")}</h3>
              <div class="tag-row">
                <span class="tag">id: ${escapeHtml(document.id)}</span>
                <span class="tag">likes: ${escapeHtml(String(document.likes ?? 0))}</span>
                ${
                  document.createdAt
                    ? `<span class="tag">created: ${escapeHtml(document.createdAt)}</span>`
                    : ""
                }
              </div>
            </div>
          </header>
          <p>${escapeHtml(document.body ?? "No body")}</p>
          <div class="tag-row">
            ${(document.tags ?? []).map((tag) => `<span class="tag">${escapeHtml(tag)}</span>`).join("")}
          </div>
        </article>
      `,
    )
    .join("");
}

function renderConnectionSummary(): void {
  elements.transportStatus.textContent = state.settings.unaryTransport;
  elements.connectionSummary.textContent = [
    `Base URL: ${state.settings.baseUrl}`,
    `Collection: ${COLLECTION_NAME}`,
    `Unary transport: ${state.settings.unaryTransport}`,
    "Listen transport: browser WebSocket",
  ].join("\n");
}

function renderWatchStatus(): void {
  elements.watchStatus.textContent = state.watchStatus;
  elements.snapshotCount.textContent = `${state.lastSnapshotCount} doc${state.lastSnapshotCount === 1 ? "" : "s"}`;
  elements.watchStatus.style.color =
    state.watchStatus === "watching"
      ? "var(--ok)"
      : state.watchStatus === "stopped"
        ? "var(--warn)"
        : "var(--ink)";
}

async function withTask(label: string, task: () => Promise<void>): Promise<void> {
  try {
    await task();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    log(`${label} failed: ${message}`);
    throw error;
  }
}

function readAuthor(): string {
  return elements.authorInput.value.trim() || defaultSettings.author;
}

function readMessageBody(): string {
  return elements.messageInput.value.trim() || defaultSettings.message;
}

function readTag(): string {
  return elements.tagInput.value.trim() || defaultSettings.tag;
}

function log(message: string): void {
  const timestamp = new Date().toLocaleTimeString();
  elements.log.textContent = `[${timestamp}] ${message}\n${elements.log.textContent}`;
}

function hydrateInputs(): void {
  elements.authorInput.value = state.settings.author;
  elements.baseUrl.value = state.settings.baseUrl;
  elements.messageInput.value = state.settings.message;
  elements.tagInput.value = state.settings.tag;
  elements.transportSelect.value = state.settings.unaryTransport;
}

function normalizeBaseUrl(value: string): string {
  const url = new URL(value.trim() || defaultSettings.baseUrl);
  return url.origin;
}

function emulatorTarget(baseUrl: string): { host: string; port: number } {
  const url = new URL(baseUrl);
  const port =
    url.port.length > 0 ? Number(url.port) : url.protocol === "https:" ? 443 : 80;
  return { host: url.hostname, port };
}

function loadSettings(): DemoSettings {
  try {
    const raw = localStorage.getItem(LOCAL_STORAGE_KEY);
    if (!raw) {
      return { ...defaultSettings };
    }
    const parsed = JSON.parse(raw) as Partial<DemoSettings>;
    return {
      author:
        typeof parsed.author === "string" && parsed.author.length > 0
          ? parsed.author
          : defaultSettings.author,
      baseUrl:
        typeof parsed.baseUrl === "string" && parsed.baseUrl.length > 0
          ? normalizeBaseUrl(parsed.baseUrl)
          : defaultSettings.baseUrl,
      message:
        typeof parsed.message === "string" && parsed.message.length > 0
          ? parsed.message
          : defaultSettings.message,
      tag:
        typeof parsed.tag === "string" && parsed.tag.length > 0
          ? parsed.tag
          : defaultSettings.tag,
      unaryTransport:
        parsed.unaryTransport === "grpc-web" ? "grpc-web" : defaultSettings.unaryTransport,
    };
  } catch {
    return { ...defaultSettings };
  }
}

function persistSettings(): void {
  localStorage.setItem(LOCAL_STORAGE_KEY, JSON.stringify(state.settings));
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

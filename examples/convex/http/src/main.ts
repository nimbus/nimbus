import { ConvexHttpClient } from "convex/browser";

import { api } from "../convex/_generated/api";
import type { Doc } from "../convex/_generated/dataModel";

type Message = Doc<"messages">;

const nativeUrl = import.meta.env.VITE_NIMBUS_NATIVE_URL ?? window.location.origin;
const convexUrl =
  import.meta.env.VITE_NIMBUS_CONVEX_URL ?? `${window.location.origin}/convex/demo`;
const client = new ConvexHttpClient(convexUrl);

const app = document.querySelector<HTMLDivElement>("#app");
if (app === null) {
  throw new Error("missing #app mount");
}

app.innerHTML = `
  <main style="font-family: ui-sans-serif, system-ui, sans-serif; margin: 0 auto; max-width: 52rem; padding: 2rem 1rem 4rem;">
    <h1>Nimbus Convex HTTP Demo</h1>
    <p>This app uses <code>convex/browser</code> and generated refs over the Nimbus convex transport.</p>
    <p style="color: #555; margin-top: -0.5rem;">Composer submits through a Convex-style action that delegates to an internal mutation. You can also schedule that internal mutation with <code>ctx.scheduler.runAfter(...)</code>, or hit compiled <code>httpAction</code> routes via the buttons below. Click a message to load it again through <code>ctx.db.get(id)</code>.</p>
    <form id="composer" style="display: grid; gap: 0.75rem; margin-bottom: 1.25rem;">
      <input id="author" placeholder="Author" value="HTTP User" style="padding: 0.75rem;" />
      <input id="body" placeholder="Write a message..." style="padding: 0.75rem;" />
      <div style="display: flex; gap: 0.75rem;">
        <button type="submit">Send</button>
        <button id="schedule" type="button">Schedule +2s</button>
        <button id="sendAndSchedule" type="button">Send + Schedule</button>
        <button id="sendHttp" type="button">Send via httpAction</button>
        <button id="loadHttp" type="button">Load via httpAction</button>
      </div>
    </form>
    <form id="filter" style="display: flex; gap: 0.75rem; margin-bottom: 1.5rem;">
      <input id="authorFilter" placeholder="Filter by author" style="flex: 1; padding: 0.75rem;" />
      <button type="submit">Apply</button>
      <button id="checkUnique" type="button">Check Unique</button>
      <button id="checkExact" type="button">Check Exact</button>
      <button id="clearFilter" type="button">Clear</button>
    </form>
    <p id="status" style="color: #555;"></p>
    <ul id="messages" style="display: grid; gap: 0.75rem; list-style: none; padding: 0;"></ul>
    <section id="detail" style="border: 1px solid #ddd; border-radius: 0.75rem; margin-top: 1.5rem; padding: 1rem;">
      <h2 style="margin-top: 0;">Selected Message</h2>
      <div id="detailBody" style="color: #666;">Select a message to load it by id.</div>
    </section>
  </main>
`;

const composer = document.querySelector<HTMLFormElement>("#composer");
const filterForm = document.querySelector<HTMLFormElement>("#filter");
const authorInput = document.querySelector<HTMLInputElement>("#author");
const bodyInput = document.querySelector<HTMLInputElement>("#body");
const scheduleButton = document.querySelector<HTMLButtonElement>("#schedule");
const sendAndScheduleButton = document.querySelector<HTMLButtonElement>("#sendAndSchedule");
const sendHttpButton = document.querySelector<HTMLButtonElement>("#sendHttp");
const loadHttpButton = document.querySelector<HTMLButtonElement>("#loadHttp");
const authorFilterInput = document.querySelector<HTMLInputElement>("#authorFilter");
const checkUniqueButton = document.querySelector<HTMLButtonElement>("#checkUnique");
const checkExactButton = document.querySelector<HTMLButtonElement>("#checkExact");
const clearFilterButton = document.querySelector<HTMLButtonElement>("#clearFilter");
const status = document.querySelector<HTMLParagraphElement>("#status");
const messagesList = document.querySelector<HTMLUListElement>("#messages");
const detailBody = document.querySelector<HTMLDivElement>("#detailBody");

if (
  composer === null ||
  filterForm === null ||
  authorInput === null ||
  bodyInput === null ||
  scheduleButton === null ||
  sendAndScheduleButton === null ||
  sendHttpButton === null ||
  loadHttpButton === null ||
  authorFilterInput === null ||
  checkUniqueButton === null ||
  checkExactButton === null ||
  clearFilterButton === null ||
  status === null ||
  messagesList === null ||
  detailBody === null
) {
  throw new Error("missing expected demo elements");
}

const composerForm = composer;
const filterControls = filterForm;
const authorField = authorInput;
const bodyField = bodyInput;
const scheduleSendButton = scheduleButton;
const sendAndScheduleRuntimeButton = sendAndScheduleButton;
const sendViaHttpButton = sendHttpButton;
const loadViaHttpButton = loadHttpButton;
const authorFilterField = authorFilterInput;
const checkUnique = checkUniqueButton;
const checkExact = checkExactButton;
const clearFilter = clearFilterButton;
const statusLine = status;
const messagesView = messagesList;
const detailView = detailBody;

async function ensureTenant() {
  const listResponse = await fetch(`${nativeUrl}/api/tenants`);
  if (!listResponse.ok) {
    throw new Error(`failed to list tenants: ${listResponse.status}`);
  }
  const payload = (await listResponse.json()) as { tenants?: string[] };
  if (payload.tenants?.includes("demo")) {
    return;
  }

  const response = await fetch(`${nativeUrl}/api/tenants`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id: "demo" }),
  });
  if (!response.ok) {
    throw new Error(`failed to ensure demo tenant: ${response.status}`);
  }
}

function renderMessages(messages: Message[]) {
  messagesView.innerHTML = [...messages]
    .reverse()
    .map(
      (message) => `
        <li data-message-id="${escapeHtml(message._id)}" style="border: 1px solid #ddd; border-radius: 0.75rem; padding: 0.9rem 1rem; cursor: pointer;">
          <div style="font-weight: 600;">${escapeHtml(message.author)}</div>
          <div>${escapeHtml(message.body)}</div>
          <small style="color: #666;">${new Date(message._creationTime).toLocaleTimeString()}</small>
        </li>
      `,
    )
    .join("");
}

async function loadMessages() {
  const authorFilter = authorFilterField.value.trim();
  statusLine.textContent = authorFilter
    ? `Loading messages by ${authorFilter}...`
    : "Loading recent messages...";
  const messages = authorFilter
    ? await client.query(api.messages.maybeByAuthor, { author: authorFilter })
    : await client.query(api.messages.maybeByAuthor, { author: null });
  renderMessages(messages);
  statusLine.textContent = `${messages.length} message${messages.length === 1 ? "" : "s"} loaded`;
  return messages;
}

function describeError(error: unknown) {
  return error instanceof Error ? error.message : "Request failed";
}

async function countMatchingMessages(author: string, body: string) {
  const messages = await client.query(api.messages.maybeByAuthor, { author });
  return messages.filter((message) => message.body === body).length;
}

async function refreshUntilMessageLands(
  author: string,
  body: string,
  previousCount: number,
  timeoutMs = 5_000,
  intervalMs = 250,
) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const authorMessages = await client.query(api.messages.maybeByAuthor, { author });
    const currentCount = authorMessages.filter((message) => message.body === body).length;
    if (currentCount > previousCount) {
      const visibleMessages = await loadMessages();
      return visibleMessages.some(
        (message) => message.author === author && message.body === body,
      )
        ? "visible"
        : "hidden";
    }
    await new Promise((resolve) => window.setTimeout(resolve, intervalMs));
  }

  await loadMessages();
  return "timeout";
}

function renderMessageDetail(message: Message | null) {
  if (message === null) {
    detailView.innerHTML = `<p style="margin: 0; color: #666;">This message no longer exists.</p>`;
    return;
  }

  detailView.innerHTML = `
    <div style="font-weight: 600;">${escapeHtml(message.author)}</div>
    <div>${escapeHtml(message.body)}</div>
    <small style="color: #666;">${new Date(message._creationTime).toLocaleTimeString()}</small>
  `;
}

async function loadMessageById(messageId: Message["_id"]) {
  statusLine.textContent = "Loading selected message...";
  const message = await client.query(api.messages.byId, {
    id: messageId,
  });
  renderMessageDetail(message);
  statusLine.textContent = message === null ? "Selected message not found" : "Selected message loaded";
}

composerForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  const author = authorField.value.trim();
  const body = bodyField.value.trim();
  if (!author || !body) {
    statusLine.textContent = "Author and message body are required.";
    return;
  }

  try {
    const priorCount = await countMatchingMessages(author, body);
    statusLine.textContent = "Sending message through action...";
    await client.action(api.messages.sendViaAction, { author, body });
    bodyField.value = "";
    const result = await refreshUntilMessageLands(author, body, priorCount);
    if (result === "visible") {
      statusLine.textContent = "Action completed and the new message is now visible.";
      return;
    }
    if (result === "hidden") {
      statusLine.textContent = "Action completed, but the current filter is hiding the new message.";
      return;
    }
    statusLine.textContent = "Action completed, but the new message did not appear within 5 seconds.";
  } catch (error) {
    statusLine.textContent = describeError(error);
  }
});

scheduleSendButton.addEventListener("click", async () => {
  const author = authorField.value.trim();
  const body = bodyField.value.trim();
  if (!author || !body) {
    statusLine.textContent = "Author and message body are required.";
    return;
  }

  try {
    const priorCount = await countMatchingMessages(author, body);
    statusLine.textContent = "Scheduling message for 2 seconds from now...";
    const jobId = await client.mutation(api.messages.scheduleSend, {
      author,
      body,
      delayMs: 2_000,
    });
    bodyField.value = "";
    statusLine.textContent = `Scheduled job ${jobId}. Waiting for delayed write...`;
    const result = await refreshUntilMessageLands(author, body, priorCount);
    if (result === "visible") {
      statusLine.textContent = `Scheduled job ${jobId} executed and the delayed message is now visible.`;
      return;
    }
    if (result === "hidden") {
      statusLine.textContent = `Scheduled job ${jobId} executed, but the current filter is hiding the delayed message.`;
      return;
    }
    statusLine.textContent = `Scheduled job ${jobId} was created, but the delayed message did not appear within 5 seconds.`;
  } catch (error) {
    statusLine.textContent = describeError(error);
  }
});

sendAndScheduleRuntimeButton.addEventListener("click", async () => {
  const author = authorField.value.trim();
  const body = bodyField.value.trim();
  if (!author || !body) {
    statusLine.textContent = "Author and message body are required.";
    return;
  }

  try {
    const scheduledBody = `${body} (scheduled)`;
    const priorScheduledCount = await countMatchingMessages(author, scheduledBody);
    statusLine.textContent = "Running runtime-only multi-step mutation...";
    const id = await client.mutation(api.messages.sendAndSchedule, {
      author,
      body,
    });
    bodyField.value = "";
    await loadMessages();
    const result = await refreshUntilMessageLands(author, scheduledBody, priorScheduledCount);
    if (result === "visible") {
      statusLine.textContent = `Runtime-only mutation created ${id}, and the scheduled follow-up message is now visible.`;
      return;
    }
    if (result === "hidden") {
      statusLine.textContent = `Runtime-only mutation created ${id}, and the scheduled follow-up message landed but is hidden by the current filter.`;
      return;
    }
    statusLine.textContent = `Runtime-only mutation created ${id}, but the scheduled follow-up did not appear within 5 seconds.`;
  } catch (error) {
    statusLine.textContent = describeError(error);
  }
});

sendViaHttpButton.addEventListener("click", async () => {
  const author = authorField.value.trim();
  const body = bodyField.value.trim();
  if (!author || !body) {
    statusLine.textContent = "Author and message body are required.";
    return;
  }

  try {
    const priorCount = await countMatchingMessages(author, body);
    statusLine.textContent = "Sending message through compiled httpAction route...";
    const response = await fetch(`${convexUrl}/http/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ author, body }),
    });
    if (!response.ok) {
      throw new Error(`httpAction failed: ${response.status}`);
    }
    const payload = (await response.json()) as { id: string };
    bodyField.value = "";
    const result = await refreshUntilMessageLands(author, body, priorCount);
    if (result === "visible") {
      statusLine.textContent = `httpAction created ${payload.id}, and the new message is now visible.`;
      return;
    }
    if (result === "hidden") {
      statusLine.textContent = `httpAction created ${payload.id}, but the current filter is hiding the new message.`;
      return;
    }
    statusLine.textContent = `httpAction created ${payload.id}, but the new message did not appear within 5 seconds.`;
  } catch (error) {
    statusLine.textContent = describeError(error);
  }
});

loadViaHttpButton.addEventListener("click", async () => {
  const author = authorFilterField.value.trim() || authorField.value.trim();
  if (!author) {
    statusLine.textContent = "Enter an author or author filter before loading via httpAction.";
    return;
  }

  statusLine.textContent = "Loading messages through compiled httpAction route...";
  const response = await fetch(
    `${convexUrl}/http/messages/by-author?author=${encodeURIComponent(author)}`,
  );
  const messages = (await response.json()) as Message[];
  renderMessages(messages);
  statusLine.textContent = `${messages.length} message${messages.length === 1 ? "" : "s"} loaded via httpAction`;
});

filterControls.addEventListener("submit", async (event) => {
  event.preventDefault();
  await loadMessages();
});

clearFilter.addEventListener("click", async () => {
  authorFilterField.value = "";
  await loadMessages();
});

checkUnique.addEventListener("click", async () => {
  const author = authorFilterField.value.trim();
  if (!author) {
    statusLine.textContent = "Enter an author filter before checking unique().";
    return;
  }

  try {
    const message = await client.query(api.messages.uniqueByAuthor, {
      author,
    });
    if (message === null) {
      statusLine.textContent = `No unique message found for ${author}.`;
      return;
    }
    statusLine.textContent = `unique() matched "${message.body}" for ${author}.`;
  } catch (error) {
    statusLine.textContent =
      error instanceof Error ? error.message : "unique() failed";
  }
});

checkExact.addEventListener("click", async () => {
  const author = authorFilterField.value.trim();
  const body = bodyField.value.trim();
  if (!author || !body) {
    statusLine.textContent = "Enter both author filter and message body before checking exact unique().";
    return;
  }

  try {
    const message = await client.query(api.messages.exactByAuthorAndBody, {
      author,
      body,
    });
    if (message === null) {
      statusLine.textContent = `No exact indexed match found for ${author}.`;
      return;
    }
    statusLine.textContent = `Exact indexed match found: "${message.body}".`;
  } catch (error) {
    statusLine.textContent =
      error instanceof Error ? error.message : "exact unique() failed";
  }
});

messagesView.addEventListener("click", async (event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement)) {
    return;
  }

  const message = target.closest<HTMLElement>("[data-message-id]");
  const messageId = message?.dataset.messageId;
  if (messageId === undefined) {
    return;
  }

  await loadMessageById(messageId as Message["_id"]);
});

void ensureTenant().then(loadMessages).catch((error: Error) => {
  statusLine.textContent = error.message;
});

function escapeHtml(value: string) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

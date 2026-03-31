import { Component, useEffect, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import {
  useConvexConnectionState,
  useMutation,
  usePaginatedQuery,
  useQuery,
  useQueries,
} from "convex/react";

import { api } from "../convex/_generated/api";
import type { Doc } from "../convex/_generated/dataModel";

type Message = Doc<"messages">;

type ErrorBoundaryProps = {
  resetKey: string;
  children?: ReactNode;
};

type ErrorBoundaryState = {
  error: Error | null;
};

class QueryErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidUpdate(previousProps: ErrorBoundaryProps) {
    if (
      previousProps.resetKey !== this.props.resetKey &&
      this.state.error !== null
    ) {
      this.setState({ error: null });
    }
  }

  render() {
    if (this.state.error !== null) {
      return (
        <section
          style={{
            backgroundColor: "rgba(180, 32, 32, 0.08)",
            border: "1px solid rgba(180, 32, 32, 0.25)",
            borderRadius: "0.75rem",
            marginTop: "1rem",
            padding: "1rem",
          }}
        >
          <h2 style={{ marginTop: 0 }}>Error Boundary Fallback</h2>
          <p style={{ color: "#7a1d1d", marginBottom: 0 }}>
            {this.state.error.message}
          </p>
        </section>
      );
    }

    return this.props.children;
  }
}

function UniqueAuthorPanel(props: { author: string }) {
  const uniqueByAuthor = useQuery(api.messages.uniqueByAuthor, {
    author: props.author,
  });

  if (uniqueByAuthor === undefined) {
    return <p style={{ color: "#666", marginBottom: 0 }}>Loading unique author query...</p>;
  }

  if (uniqueByAuthor === null) {
    return (
      <p style={{ color: "#666", marginBottom: 0 }}>
        No messages for {props.author} yet.
      </p>
    );
  }

  return (
    <>
      <div style={{ fontWeight: 600 }}>{uniqueByAuthor.author}</div>
      <div>{uniqueByAuthor.body}</div>
      <small style={{ color: "#666" }}>
        Exactly one message currently exists for this author.
      </small>
    </>
  );
}

function QueryGroupPanel(props: { author: string }) {
  const results = useQueries({
    latest: {
      query: api.messages.latestByAuthor,
      args: { author: props.author },
    },
    unique: {
      query: api.messages.uniqueByAuthor,
      args: { author: props.author },
    },
  });

  const latest = results.latest;
  const unique = results.unique;

  return (
    <section
      style={{
        border: "1px solid #ddd",
        borderRadius: "0.75rem",
        marginTop: "1rem",
        padding: "1rem",
      }}
    >
      <h2 style={{ marginTop: 0 }}>useQueries Group</h2>
      <p style={{ color: "#666", marginTop: "-0.25rem" }}>
        This panel uses <code>useQueries</code>, so query errors stay local as <code>Error</code> values instead of throwing into an error boundary.
      </p>
      <div style={{ display: "grid", gap: "0.75rem" }}>
        <div>
          <strong>Latest:</strong>{" "}
          {latest === undefined
            ? "Loading..."
            : latest instanceof Error
              ? latest.message
              : latest === null
                ? "No message yet"
                : latest.body}
        </div>
        <div>
          <strong>Unique:</strong>{" "}
          {unique === undefined
            ? "Loading..."
            : unique instanceof Error
              ? unique.message
              : unique === null
                ? "No unique message yet"
                : unique.body}
        </div>
      </div>
    </section>
  );
}

export default function App() {
  const [authorFilter, setAuthorFilter] = useState("");
  const [author] = useState(() => `User ${Math.floor(Math.random() * 10000)}`);
  const trimmedAuthorFilter = authorFilter.trim();
  const maybeByAuthorMessages = useQuery(api.messages.maybeByAuthor, {
    author: trimmedAuthorFilter || null,
  });
  const latestByAuthor = useQuery(
    api.messages.latestByAuthor,
    trimmedAuthorFilter ? { author: trimmedAuthorFilter } : "skip",
  );
  const authoredMessages = useQuery(api.messages.byAuthor, { author });
  const paginatedMessages = usePaginatedQuery(
    api.messages.listPage,
    { author: trimmedAuthorFilter || null },
    { initialNumItems: 3 },
  );
  const messages = maybeByAuthorMessages ?? [];
  const [selectedId, setSelectedId] = useState<Message["_id"] | null>(null);
  const selectedMessage = useQuery(
    api.messages.byId,
    selectedId === null ? "skip" : { id: selectedId },
  );
  const sendMessage = useMutation(api.messages.send);
  const scheduleMessage = useMutation(api.messages.scheduleSend);
  const renameMessage = useMutation(api.messages.rename);
  const removeMessage = useMutation(api.messages.remove);
  const connection = useConvexConnectionState();
  const [body, setBody] = useState("");
  const [scheduledJobId, setScheduledJobId] = useState<string | null>(null);
  const boundaryResetKey = `${author}:${(authoredMessages ?? [])
    .map((message) => message._id)
    .join(",")}`;

  useEffect(() => {
    const nativeUrl = import.meta.env.VITE_NEOVEX_NATIVE_URL ?? "http://localhost:8080";
    void fetch(`${nativeUrl}/api/tenants`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: "demo" }),
    });
  }, []);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!body.trim()) {
      return;
    }
    await sendMessage({ author, body: body.trim() });
    setScheduledJobId(null);
    setBody("");
  }

  async function handleSchedule() {
    if (!body.trim()) {
      return;
    }
    const jobId = await scheduleMessage({
      author,
      body: body.trim(),
      delayMs: 2_000,
    });
    setScheduledJobId(jobId);
    setBody("");
  }

  async function handleRename(message: Message) {
    await renameMessage({
      id: message._id,
      body: `${message.body} (edited)`,
    });
  }

  async function handleRemove(message: Message) {
    await removeMessage({ id: message._id });
  }

  return (
    <main
      style={{
        fontFamily: "ui-sans-serif, system-ui, sans-serif",
        margin: "0 auto",
        maxWidth: "52rem",
        padding: "2rem 1rem 4rem",
      }}
    >
      <h1>Neovex Convex Demo</h1>
      <p>
        This app uses <code>convex/react</code>, generated named refs, and
        Neovex's Convex transport.
      </p>
      <p>
        WebSocket connected:{" "}
        <strong>{connection.isWebSocketConnected ? "yes" : "no"}</strong>
      </p>
      <p style={{ color: "#555", marginTop: "-0.25rem" }}>
        The main list now runs through a runtime-only conditional query handler, while clicking a message still opens a live <code>ctx.db.get(id)</code> detail query.
      </p>
      <p style={{ color: "#555", marginTop: "-0.5rem" }}>
        The paginated panel below now runs through a runtime-only <code>paginatedQuery</code> handler with live invalidation, so inserts, edits, and deletes refresh the currently loaded window.
      </p>
      <p style={{ color: "#555", marginTop: "-0.5rem" }}>
        The composer can also schedule a generated internal mutation through <code>ctx.scheduler.runAfter(...)</code>, and the live query updates when that delayed write lands.
      </p>
      <p style={{ color: "#555", marginTop: "-0.5rem" }}>
        This page now also proves React parity behavior: query errors throw into an error boundary, while <code>useQueries</code> surfaces per-query <code>Error</code> values without taking down sibling panels.
      </p>
      <div style={{ display: "flex", gap: "0.75rem", marginBottom: "1rem" }}>
        <input
          value={authorFilter}
          onChange={(event) => setAuthorFilter(event.target.value)}
          placeholder="Filter by author..."
          style={{ flex: 1, padding: "0.75rem" }}
        />
        <button type="button" onClick={() => setAuthorFilter("")}>
          Clear
        </button>
      </div>
      <form
        onSubmit={handleSubmit}
        style={{ display: "flex", gap: "0.75rem", marginBottom: "1.5rem" }}
      >
        <input
          value={body}
          onChange={(event) => setBody(event.target.value)}
          placeholder="Write a message..."
          style={{ flex: 1, padding: "0.75rem" }}
        />
        <button type="submit" disabled={!body.trim()}>
          Send
        </button>
        <button type="button" disabled={!body.trim()} onClick={() => void handleSchedule()}>
          Schedule +2s
        </button>
      </form>
      {scheduledJobId ? (
        <p style={{ color: "#666", marginTop: "-0.5rem" }}>
          Scheduled job <code>{scheduledJobId}</code>. It should appear in the live feed in about 2 seconds.
        </p>
      ) : null}
      <p style={{ color: "#666", marginTop: "-0.5rem" }}>
        Showing {messages.length} message{messages.length === 1 ? "" : "s"}
        {trimmedAuthorFilter ? ` for ${trimmedAuthorFilter}` : ""}.
      </p>
      {trimmedAuthorFilter ? (
        <section
          style={{
            border: "1px solid #ddd",
            borderRadius: "0.75rem",
            marginBottom: "1rem",
            padding: "1rem",
          }}
        >
          <h2 style={{ marginTop: 0 }}>Latest For Author</h2>
          {latestByAuthor == null ? (
            <p style={{ marginBottom: 0, color: "#666" }}>
              No message found for {trimmedAuthorFilter}.
            </p>
          ) : (
            <>
              <div style={{ fontWeight: 600 }}>{latestByAuthor.author}</div>
              <div>{latestByAuthor.body}</div>
              <small style={{ color: "#666" }}>
                {new Date(latestByAuthor._creationTime).toLocaleTimeString()}
              </small>
            </>
          )}
        </section>
      ) : null}
      <QueryErrorBoundary resetKey={boundaryResetKey}>
        <section
          style={{
            border: "1px solid #ddd",
            borderRadius: "0.75rem",
            marginBottom: "1rem",
            padding: "1rem",
          }}
        >
          <h2 style={{ marginTop: 0 }}>Error Boundary Query</h2>
          <p style={{ color: "#666", marginTop: "-0.25rem" }}>
            Send two messages as <code>{author}</code> and this <code>unique()</code> query will throw. The fallback below proves React error-boundary handling for query errors.
          </p>
          <p style={{ color: "#666", marginTop: "-0.25rem" }}>
            If you delete back down to one matching message, the boundary resets automatically from the live author query instead of getting stuck.
          </p>
          <UniqueAuthorPanel author={author} />
        </section>
      </QueryErrorBoundary>
      <QueryGroupPanel author={author} />
      <ul style={{ display: "grid", gap: "0.75rem", listStyle: "none", padding: 0 }}>
        {messages.map((message) => (
          <li
            key={message._id}
            onClick={() => setSelectedId(message._id)}
            style={{
              border: "1px solid #ddd",
              borderRadius: "0.75rem",
              padding: "0.9rem 1rem",
              cursor: "pointer",
              backgroundColor:
                selectedId === message._id ? "rgba(15, 76, 129, 0.08)" : "white",
            }}
          >
            <div style={{ fontWeight: 600 }}>{message.author}</div>
            <div>{message.body}</div>
            <small style={{ color: "#666" }}>
              {new Date(message._creationTime).toLocaleTimeString()}
            </small>
            <div style={{ display: "flex", gap: "0.5rem", marginTop: "0.75rem" }}>
              <button type="button" onClick={() => void handleRename(message)}>
                Edit
              </button>
              <button type="button" onClick={() => void handleRemove(message)}>
                Delete
              </button>
            </div>
          </li>
        ))}
      </ul>
      <section
        style={{
          border: "1px solid #ddd",
          borderRadius: "0.75rem",
          marginTop: "1.5rem",
          padding: "1rem",
        }}
      >
        <div
          style={{
            alignItems: "center",
            display: "flex",
            gap: "0.75rem",
            justifyContent: "space-between",
          }}
        >
          <div>
            <h2 style={{ margin: 0 }}>Paginated Feed</h2>
            <p style={{ color: "#666", margin: "0.35rem 0 0" }}>
              Loaded {paginatedMessages.results.length} message
              {paginatedMessages.results.length === 1 ? "" : "s"} with status{" "}
              <code>{paginatedMessages.status}</code>.
            </p>
          </div>
          <button
            type="button"
            disabled={paginatedMessages.status !== "CanLoadMore"}
            onClick={() => paginatedMessages.loadMore(3)}
          >
            {paginatedMessages.isLoading ? "Loading..." : "Load More"}
          </button>
        </div>
        <ul
          style={{
            display: "grid",
            gap: "0.75rem",
            listStyle: "none",
            marginBottom: 0,
            padding: 0,
          }}
        >
          {paginatedMessages.results.map((message) => (
            <li
              key={`page-${message._id}`}
              style={{
                border: "1px solid #ddd",
                borderRadius: "0.75rem",
                marginTop: "0.75rem",
                padding: "0.9rem 1rem",
              }}
            >
              <div style={{ fontWeight: 600 }}>{message.author}</div>
              <div>{message.body}</div>
              <small style={{ color: "#666" }}>
                {new Date(message._creationTime).toLocaleTimeString()}
              </small>
            </li>
          ))}
        </ul>
      </section>
      <section
        style={{
          border: "1px solid #ddd",
          borderRadius: "0.75rem",
          marginTop: "1.5rem",
          padding: "1rem",
        }}
      >
        <h2 style={{ marginTop: 0 }}>Selected Message</h2>
        {selectedId === null ? (
          <p style={{ marginBottom: 0, color: "#666" }}>
            Select a message to start a live detail query.
          </p>
        ) : selectedMessage == null ? (
          <p style={{ marginBottom: 0, color: "#666" }}>
            This message no longer exists.
          </p>
        ) : (
          <>
            <div style={{ fontWeight: 600 }}>{selectedMessage.author}</div>
            <div>{selectedMessage.body}</div>
            <small style={{ color: "#666" }}>
              {new Date(selectedMessage._creationTime).toLocaleTimeString()}
            </small>
          </>
        )}
      </section>
    </main>
  );
}

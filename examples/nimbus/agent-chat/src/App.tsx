import { useState } from "react";
import type { FormEvent } from "react";
import { useMutation, useQuery } from "@nimbus/nimbus/react";

import { api } from "../nimbus/_generated/api";
import type { Doc } from "../nimbus/_generated/dataModel";
import "./app.css";

type Message = Doc<"messages">;
type Memory = Doc<"agentMemory">;

const CONVERSATION_ID = "demo-conversation";

export default function App() {
  const messages = useQuery(api.agent.list, { conversationId: CONVERSATION_ID });
  const memory = useQuery(api.agent.listMemory, { conversationId: CONVERSATION_ID });
  const send = useMutation(api.agent.send);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalizedText = text.trim();
    if (!normalizedText || submitting) return;

    setError(null);
    setSubmitting(true);
    try {
      await send({ conversationId: CONVERSATION_ID, text: normalizedText });
      setText("");
    } catch (caught) {
      setError((caught as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  const sortedMessages = [...(messages ?? [])].sort((a, b) => a.createdAt - b.createdAt);
  const sortedMemory = [...(memory ?? [])].sort((a, b) => a.createdAt - b.createdAt);

  return (
    <main className="shell">
      <header>
        <p className="eyebrow">Native Nimbus SDK agent</p>
        <h1>An agent that runs inside your trust boundary.</h1>
        <p className="lede">
          Every reply is a plain function handler with tool-call branching. Ask
          it to <code>remember: …</code>, ask <code>what do you remember</code>,
          or ask it to <code>remind me in 3000ms: …</code> — the reminder lands
          on its own once the delay elapses, delivered by the server's own
          scheduler with no further client action.
        </p>
        <div className="connection" aria-live="polite">
          <span className={messages === undefined ? "status-dot" : "status-dot connected"} />
          <span>{messages === undefined ? "Connecting…" : "Live on demo"}</span>
        </div>
      </header>

      <section className="task-panel" aria-labelledby="conversation-heading">
        <div className="list-heading">
          <h2 id="conversation-heading">Conversation</h2>
          <span>
            {messages === undefined
              ? "Loading…"
              : `${sortedMessages.length} ${sortedMessages.length === 1 ? "turn" : "turns"}`}
          </span>
        </div>

        {sortedMessages.length === 0 ? (
          <p className="empty-state">Nothing here yet. Say hello.</p>
        ) : (
          <ul aria-live="polite">
            {sortedMessages.map((message: Message) => (
              <li key={message._id} className={message.role === "assistant" ? "assistant" : undefined}>
                <label>
                  <span>
                    <strong>{message.role === "assistant" ? "Agent" : "You"}:</strong> {message.text}
                    {message.tool ? <em className="tool-tag"> [{message.tool}]</em> : null}
                  </span>
                </label>
              </li>
            ))}
          </ul>
        )}

        <form onSubmit={handleSubmit}>
          <label className="sr-only" htmlFor="message-text">Message</label>
          <input
            id="message-text"
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder="Try: remember: my favorite color is teal"
            autoComplete="off"
            required
          />
          <button type="submit" disabled={!text.trim() || submitting}>
            {submitting ? "Sending…" : "Send"}
          </button>
        </form>
      </section>

      <section className="task-panel" aria-labelledby="memory-heading">
        <div className="list-heading">
          <h2 id="memory-heading">Memory</h2>
          <span>
            {memory === undefined
              ? "Loading…"
              : `${sortedMemory.length} ${sortedMemory.length === 1 ? "fact" : "facts"}`}
          </span>
        </div>
        {sortedMemory.length === 0 ? (
          <p className="empty-state">Nothing remembered yet.</p>
        ) : (
          <ul aria-live="polite">
            {sortedMemory.map((fact: Memory) => (
              <li key={fact._id}>
                <span>{fact.text}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <p className="error-message" role="alert">{error}</p>
    </main>
  );
}

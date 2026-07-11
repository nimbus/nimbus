import { useState } from "react";
import type { FormEvent } from "react";
import { useMutation, useQuery } from "convex/react";

import { api } from "../convex/_generated/api";
import type { Doc } from "../convex/_generated/dataModel";
import "./app.css";

type Task = Doc<"tasks">;

export default function App() {
  const tasks = useQuery(api.tasks.list, {});
  const createTask = useMutation(api.tasks.create);
  const toggleTask = useMutation(api.tasks.toggle);
  const removeTask = useMutation(api.tasks.remove);
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
      await createTask({ text: normalizedText });
      setText("");
    } catch (caught) {
      setError((caught as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  async function handleToggle(task: Task) {
    setError(null);
    try {
      await toggleTask({ id: task._id });
    } catch (caught) {
      setError((caught as Error).message);
    }
  }

  async function handleRemove(task: Task) {
    setError(null);
    try {
      await removeTask({ id: task._id });
    } catch (caught) {
      setError((caught as Error).message);
    }
  }

  return (
    <main className="shell">
      <header>
        <p className="eyebrow">Convex API on Nimbus</p>
        <h1>Tasks that stay in sync.</h1>
        <p className="lede">
          Add a task here and every open copy updates through a reactive Convex
          query—no refresh and no polling.
        </p>
        <div className="connection" aria-live="polite">
          <span className={tasks === undefined ? "status-dot" : "status-dot connected"} />
          <span>{tasks === undefined ? "Connecting…" : "Live on demo"}</span>
        </div>
      </header>

      <section className="task-panel" aria-labelledby="tasks-heading">
        <form onSubmit={handleSubmit}>
          <label className="sr-only" htmlFor="task-text">Task description</label>
          <input
            id="task-text"
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder="What needs doing?"
            autoComplete="off"
            required
          />
          <button type="submit" disabled={!text.trim() || submitting}>
            {submitting ? "Adding…" : "Add task"}
          </button>
        </form>

        <div className="list-heading">
          <h2 id="tasks-heading">Your tasks</h2>
          <span>{tasks === undefined ? "Loading…" : `${tasks.length} ${tasks.length === 1 ? "task" : "tasks"}`}</span>
        </div>

        {tasks?.length === 0 ? (
          <p className="empty-state">Nothing here yet. Add the first task.</p>
        ) : (
          <ul aria-live="polite">
            {(tasks ?? []).map((task) => (
              <li key={task._id} className={task.completed ? "completed" : undefined}>
                <label>
                  <input
                    type="checkbox"
                    checked={task.completed}
                    onChange={() => void handleToggle(task)}
                    aria-label={`Mark ${task.text} ${task.completed ? "incomplete" : "complete"}`}
                  />
                  <span>{task.text}</span>
                </label>
                <button
                  className="delete"
                  type="button"
                  onClick={() => void handleRemove(task)}
                  aria-label={`Delete ${task.text}`}
                >
                  Delete
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <p className="error-message" role="alert">{error}</p>
    </main>
  );
}

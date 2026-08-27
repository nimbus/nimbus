import "./app.css";

import { initializeApp } from "firebase/app";
import {
  addDoc,
  collection,
  connectFirestoreEmulator,
  deleteDoc,
  doc,
  initializeFirestore,
  onSnapshot,
  orderBy,
  query,
  updateDoc,
  type DocumentData,
} from "firebase/firestore";

interface Task {
  id: string;
  text: string;
  completed: boolean;
  createdAt: number;
}

const projectId = "demo";
const baseUrl = new URL(
  new URLSearchParams(window.location.search).get("server") ?? window.location.origin,
);
const app = initializeApp({ apiKey: "nimbus-tasks", projectId });
const firestore = initializeFirestore(app, { experimentalUnaryTransport: "rest" });
connectFirestoreEmulator(
  firestore,
  baseUrl.hostname,
  baseUrl.port ? Number(baseUrl.port) : baseUrl.protocol === "https:" ? 443 : 80,
  {
    mockUserToken: {
      sub: "firebase-tasks-browser",
      iss: `https://securetoken.google.com/${projectId}`,
    },
  },
);

const tasksCollection = collection(firestore, "tasks");
const tasksQuery = query(tasksCollection, orderBy("createdAt", "desc"));
const form = requiredElement(document.querySelector<HTMLFormElement>("#task-form"), "task form");
const input = requiredElement(document.querySelector<HTMLInputElement>("#task-text"), "task text");
const list = requiredElement(document.querySelector<HTMLUListElement>("#task-list"), "task list");
const count = requiredElement(document.querySelector<HTMLElement>("#task-count"), "task count");
const empty = requiredElement(document.querySelector<HTMLElement>("#empty-state"), "empty state");
const errorMessage = requiredElement(
  document.querySelector<HTMLElement>("#error-message"),
  "error message",
);
const connectionDot = requiredElement(
  document.querySelector<HTMLElement>("#connection-dot"),
  "connection dot",
);
const connectionStatus = requiredElement(
  document.querySelector<HTMLElement>("#connection-status"),
  "connection status",
);

let tasks: Task[] = [];

function requiredElement<T>(value: T | null, name: string): T {
  if (!value) throw new Error(`Missing ${name}.`);
  return value;
}

function normalizeTask(id: string, data: DocumentData): Task | null {
  if (
    typeof data.text !== "string"
    || typeof data.completed !== "boolean"
    || typeof data.createdAt !== "number"
  ) {
    return null;
  }
  return { id, text: data.text, completed: data.completed, createdAt: data.createdAt };
}

function setError(error?: unknown): void {
  errorMessage.textContent = error ? (error as Error).message : "";
}

function setConnected(connected: boolean): void {
  connectionDot.classList.toggle("connected", connected);
  connectionStatus.textContent = connected ? `Live on ${projectId}` : "Connecting…";
}

function render(): void {
  list.replaceChildren(...tasks.map((task) => {
    const item = document.createElement("li");
    const label = document.createElement("label");
    const toggle = document.createElement("input");
    const text = document.createElement("span");
    const remove = document.createElement("button");

    toggle.type = "checkbox";
    toggle.checked = task.completed;
    toggle.setAttribute(
      "aria-label",
      `Mark ${task.text} ${task.completed ? "incomplete" : "complete"}`,
    );
    toggle.addEventListener("change", () => {
      void toggleTask(task, toggle.checked);
    });
    text.textContent = task.text;
    label.append(toggle, text);

    remove.type = "button";
    remove.className = "delete";
    remove.textContent = "Delete";
    remove.setAttribute("aria-label", `Delete ${task.text}`);
    remove.addEventListener("click", () => {
      void removeTask(task);
    });

    item.classList.toggle("completed", task.completed);
    item.append(label, remove);
    return item;
  }));
  count.textContent = `${tasks.length} ${tasks.length === 1 ? "task" : "tasks"}`;
  empty.hidden = tasks.length > 0;
}

async function toggleTask(task: Task, completed: boolean): Promise<void> {
  try {
    setError();
    await updateDoc(doc(tasksCollection, task.id), { completed });
  } catch (error) {
    setError(error);
    render();
  }
}

async function removeTask(task: Task): Promise<void> {
  try {
    setError();
    await deleteDoc(doc(tasksCollection, task.id));
  } catch (error) {
    setError(error);
  }
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  const text = input.value.trim();
  if (!text) return;
  void (async () => {
    try {
      setError();
      await addDoc(tasksCollection, { text, completed: false, createdAt: Date.now() });
      form.reset();
      input.focus();
    } catch (error) {
      setError(error);
    }
  })();
});

const unsubscribe = onSnapshot(
  tasksQuery,
  (snapshot) => {
    tasks = snapshot.docs.flatMap((document) => {
      const task = normalizeTask(document.id, document.data());
      return task ? [task] : [];
    });
    setConnected(true);
    setError();
    render();
  },
  (error) => {
    setConnected(false);
    setError(error);
  },
);

window.addEventListener("beforeunload", unsubscribe);

render();

import type { AppDefinition, BackendAPI } from "tsgpu/types";

type Task = { id: string; title: string; body: string; done: boolean };
type Row = { marker: string; title: string; meta: string };
type Detail = { title: string; meta: string; body: string; action: string };

type ListState = {
  tasks: Task[];
  query: string;
  selectedId: string;
  visible: Row[];
  selected: Detail;
  shown: number;
  total: number;
};

const SEED: Task[] = [
  { id: "1", title: "Write the README", body: "Explain what the app does and how to run it.", done: true },
  { id: "2", title: "Add a settings screen", body: "Theme, window size and where data is stored.", done: false },
  { id: "3", title: "Package for Windows", body: "Run tsgpu build and test the executable on a clean machine.", done: false },
  { id: "4", title: "Sign the release", body: "Use tsgpu sign so SmartScreen does not warn.", done: false }
];

const NOTHING: Detail = { title: "Nothing selected", meta: "", body: "Use Previous and Next to pick a task.", action: "Mark done" };

function matching(api: BackendAPI<ListState>): Task[] {
  const query = api.get("query").trim().toLowerCase();
  return api.get("tasks").filter((task) => task.title.toLowerCase().includes(query));
}

// The view model (visible rows, selected detail) is recomputed from `tasks`, `query` and `selectedId`
// after every change, so the HTML only ever binds to plain values.
function refresh(api: BackendAPI<ListState>): void {
  const rows = matching(api);
  const selectedId = api.get("selectedId");
  api.set("visible", rows.map((task) => ({
    marker: task.id === selectedId ? "▶" : " ",
    title: task.title,
    meta: task.done ? "Done" : "To do"
  })));
  api.set("shown", rows.length);
  api.set("total", api.get("tasks").length);
  const picked = api.get("tasks").find((task) => task.id === selectedId);
  api.set("selected", picked
    ? { title: picked.title, meta: picked.done ? "Done" : "To do", body: picked.body, action: picked.done ? "Mark to do" : "Mark done" }
    : NOTHING);
}

// Moves the selection through the rows that match the search, wrapping at both ends.
function step(api: BackendAPI<ListState>, direction: 1 | -1): void {
  const rows = matching(api);
  if (rows.length === 0) return;
  const current = rows.findIndex((task) => task.id === api.get("selectedId"));
  const next = current === -1 ? (direction === 1 ? 0 : rows.length - 1) : (current + direction + rows.length) % rows.length;
  api.set("selectedId", rows[next].id);
  refresh(api);
}

const app: AppDefinition<ListState, "search" | "next" | "previous" | "toggleDone" | "remove"> = {
  state: { tasks: SEED, query: "", selectedId: "1", visible: [], selected: NOTHING, shown: 0, total: 0 },
  actions: {
    search(api, event) {
      api.set("query", String(event.value ?? ""));
      refresh(api);
    },
    next(api) {
      step(api, 1);
    },
    previous(api) {
      step(api, -1);
    },
    toggleDone(api) {
      const id = api.get("selectedId");
      api.update("tasks", (tasks) => tasks.map((task) => (task.id === id ? { ...task, done: !task.done } : task)));
      refresh(api);
    },
    remove(api) {
      const id = api.get("selectedId");
      api.update("tasks", (tasks) => tasks.filter((task) => task.id !== id));
      api.set("selectedId", "");
      refresh(api);
    }
  }
};

export default app;

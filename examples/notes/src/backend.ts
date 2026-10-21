import type { AppDefinition, BackendAPI } from "tsgpui/types";

// A full-stack example: the UI is HTML/CSS, and this backend uses the host's granted file access.
// Notes live in the `notes` folder granted in tsgpui.config.json. Every file name is checked here,
// and the host checks it again.

type FileEntry = { name: string; kind: "file" | "directory" | "other"; size: number };

type NotesState = {
  draftName: string;
  draftText: string;
  files: FileEntry[];
  status: string;
};

// Accepts a plain name like "ideas" or "ideas.txt", and rejects anything that could leave the folder.
function noteFileName(draft: string): string {
  const trimmed = draft.trim();
  if (trimmed === "" || /[\\/]/.test(trimmed) || trimmed.includes("..")) {
    throw new Error("Use a plain name without slashes or '..'");
  }
  return trimmed.endsWith(".txt") ? trimmed : `${trimmed}.txt`;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function listNotes(api: BackendAPI<NotesState>): Promise<void> {
  const files = await api.native.filesystem.list("");
  api.set("files", files.filter((entry) => entry.kind === "file"));
}

const app: AppDefinition<NotesState> = {
  state: { draftName: "", draftText: "", files: [], status: "Ready" },
  actions: {
    async refresh(api) {
      try {
        await listNotes(api);
        api.set("status", `${api.get("files").length} notes`);
      } catch (error) {
        api.set("status", messageOf(error));
      }
    },

    async saveNote(api) {
      try {
        const name = noteFileName(api.get("draftName"));
        await api.native.filesystem.writeText(name, api.get("draftText"));
        await listNotes(api);
        api.set("status", `Saved ${name}`);
      } catch (error) {
        api.set("status", messageOf(error));
      }
    },

    async openNote(api) {
      try {
        const name = noteFileName(api.get("draftName"));
        api.set("draftText", await api.native.filesystem.readText(name));
        api.set("status", `Opened ${name}`);
      } catch (error) {
        api.set("status", messageOf(error));
      }
    },

    async deleteNote(api) {
      try {
        const name = noteFileName(api.get("draftName"));
        await api.native.filesystem.remove(name);
        await listNotes(api);
        api.set("status", `Deleted ${name}`);
      } catch (error) {
        api.set("status", messageOf(error));
      }
    },
  },
};

export default app;

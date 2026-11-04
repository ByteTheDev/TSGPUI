// Accepts a plain name like "ideas" or "ideas.txt", and rejects anything that could leave the folder.
export function noteFileName(draft: string): string {
  const trimmed = draft.trim();
  if (trimmed === "" || /[\\/]/.test(trimmed) || trimmed.includes("..")) {
    throw new Error("Use a plain name without slashes or '..'");
  }
  return trimmed.endsWith(".txt") ? trimmed : `${trimmed}.txt`;
}

export function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}


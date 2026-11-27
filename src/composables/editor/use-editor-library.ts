/** Keeps the local Editor shortcuts and saved document previews available across routes. */
import { ref } from "vue";

const STORAGE_KEY = "wits-editor-library";
const RECENT_LIMIT = 30;

export interface EditorShortcut {
  path: string;
  name: string;
  folder: string;
  excerpt: string[];
  opened: number;
  pinned: boolean;
  missing: boolean;
}

// Treat old or invalid local metadata as optional so file actions are always available.
function readLibrary(): EditorShortcut[] {
  try {
    const entries: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(entries)
      ? entries.filter((entry): entry is EditorShortcut =>
          entry && typeof entry.path === "string" && typeof entry.name === "string" &&
          typeof entry.folder === "string" && typeof entry.opened === "number" &&
          Array.isArray(entry.excerpt) && entry.excerpt.every((line: unknown) => typeof line === "string"))
      : [];
  } catch {
    return [];
  }
}

export function useEditorLibrary() {
  const entries = ref(readLibrary());
  const storageError = ref(false);

  // Keep pins independently of the bounded recent list.
  function update(next: EditorShortcut[]) {
    entries.value = next;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
      storageError.value = false;
    } catch {
      storageError.value = true;
    }
  }

  // Record saved content only. A preview never opens or changes the active document.
  function record(path: string, markdown: string) {
    const existing = entries.value.find((entry) => entry.path === path);
    const visiblePath = path.startsWith("\\\\?\\UNC\\")
      ? "\\\\" + path.slice(8)
      : path.startsWith("\\\\?\\") ? path.slice(4) : path;
    const separator = Math.max(visiblePath.lastIndexOf("\\"), visiblePath.lastIndexOf("/"));
    const excerpt = markdown.slice(0, 2400).split(/\r?\n/)
      .map((line) => line.trim().replace(/^#{1,6}\s+/, "").slice(0, 90))
      .filter(Boolean)
      .slice(0, 5);
    const others = entries.value.filter((entry) => entry.path !== path);
    const next = [{
      path,
      name: visiblePath.slice(separator + 1),
      folder: visiblePath.slice(0, separator + 1),
      excerpt,
      opened: Date.now(),
      pinned: existing?.pinned ?? false,
      missing: false,
    }, ...others];
    update(next.filter((entry, index) => index < RECENT_LIMIT || entry.pinned));
  }

  // Repairing a shortcut transfers its pin to the valid replacement.
  function relocate(oldPath: string, newPath: string) {
    if (oldPath === newPath) return;
    const pinned = entries.value.some((entry) => entry.path === oldPath && entry.pinned);
    update(entries.value.filter((entry) => entry.path !== oldPath)
      .map((entry) => entry.path === newPath ? { ...entry, pinned: pinned || entry.pinned } : entry));
  }

  // Keep unavailable shortcuts visible until the user locates or removes them.
  function markMissing(path: string) {
    update(entries.value.map((entry) => entry.path === path ? { ...entry, missing: true } : entry));
  }

  function togglePin(path: string) {
    update(entries.value.map((entry) => entry.path === path ? { ...entry, pinned: !entry.pinned } : entry));
  }

  function remove(path: string) {
    update(entries.value.filter((entry) => entry.path !== path));
  }

  return { entries, storageError, record, relocate, markMissing, togglePin, remove };
}

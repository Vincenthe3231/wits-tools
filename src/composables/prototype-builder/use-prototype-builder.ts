/** Owns the active prototype draft and coordinates import and snapshot requests. */
import { ref } from "vue";

import sample from "../../../examples/prototype-builder/wits-starter.json";
import { prototypeBuilder, type ImportedStarter, type Variant } from "../../api/prototype-builder";
import { notify } from "../use-notifications";
import { rename, reorder } from "../../utils/prototype-builder/cards";

// Map native codes to recovery steps the user can take in the workspace.
const messages: Record<string, string> = {
  invalidUrl: "Enter a GitHub repository URL in the form https://github.com/owner/repo.",
  missingStarter: "GitHub did not return examples/prototype-builder/wits-starter.json. Use a public repo with that file on its default branch, or open the bundled sample.",
  invalidStarter: "Check that the starter uses version 1 and has titled cards with unique IDs.",
  importUnavailable: "Check your connection and retry the import.",
  invalidName: "Enter a name for this variant and retry.",
  storageUnavailable: "Check that WITS can access its app data folder and retry.",
};

export function usePrototypeBuilder() {
  // Track each request separately so failures stay beside the action that owns them.
  const source = ref("");
  const draft = ref<Variant | null>(null);
  const variants = ref<Variant[]>([]);
  const busy = ref(false);
  const loading = ref(false);
  const importError = ref("");
  const saveError = ref("");
  const listError = ref("");
  const dirty = ref(false);

  function message(error: unknown, fallback: string) {
    return typeof error === "string" && Object.prototype.hasOwnProperty.call(messages, error)
      ? messages[error]!
      : fallback;
  }

  // Replace the draft only after a successful import or an explicit open.
  function startDraft(imported: ImportedStarter) {
    draft.value = { ...imported, id: null, name: imported.starter.title };
    saveError.value = "";
    dirty.value = true;
  }

  async function importStarter() {
    if (busy.value) return;
    busy.value = true;
    importError.value = "";
    try {
      startDraft(await prototypeBuilder.import(source.value));
    } catch (error) {
      importError.value = message(error, "Try importing the starter again.");
    } finally {
      busy.value = false;
    }
  }

  // Use the same licensed fixture as GitHub import, with no network dependency.
  function openSample() {
    importError.value = "";
    startDraft({
      sourceUrl: "https://github.com/Vincenthe3231/wits-tools",
      starter: { ...sample, version: 1, cards: sample.cards.map((card) => ({ ...card })) },
    });
  }

  // Read snapshots from disk without making a GitHub request.
  async function load() {
    if (loading.value) return;
    loading.value = true;
    listError.value = "";
    try {
      variants.value = await prototypeBuilder.list();
    } catch (error) {
      listError.value = message(error, "Try loading saved variants again.");
    } finally {
      loading.value = false;
    }
  }

  function open(variant: Variant) {
    draft.value = { ...variant, starter: { ...variant.starter, cards: [...variant.starter.cards] } };
    saveError.value = "";
    dirty.value = false;
  }

  // Duplicate explicitly so Save on an existing draft updates its own snapshot.
  async function save(copy = false) {
    if (!draft.value || busy.value) return;
    const snapshot = { ...draft.value, name: draft.value.name.trim() };
    busy.value = true;
    saveError.value = "";
    try {
      const id = await prototypeBuilder.save({ ...snapshot, id: copy ? null : snapshot.id });
      draft.value = { ...snapshot, id };
      dirty.value = false;
      notify({ severity: "success", title: "Variant saved" });
      await load();
    } catch (error) {
      saveError.value = message(error, "Try saving this variant again.");
    } finally {
      busy.value = false;
    }
  }

  // Preview and editor observe the same reordered card list.
  function move(id: string, destination: number) {
    if (!draft.value || busy.value) return;
    draft.value.starter.cards = reorder(draft.value.starter.cards, id, destination);
    dirty.value = true;
  }

  function renameCard(id: string, title: string) {
    if (!draft.value || busy.value) return;
    draft.value.starter.cards = rename(draft.value.starter.cards, id, title);
    dirty.value = true;
  }

  return { source, draft, variants, busy, loading, importError, saveError, listError, dirty, importStarter, openSample, load, open, save, move, renameCard };
}

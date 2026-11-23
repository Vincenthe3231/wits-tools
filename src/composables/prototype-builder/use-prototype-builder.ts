/** Owns the active canvas and coordinates starter import and snapshot requests. */
import { ref } from "vue";

import sample from "../../../examples/prototype-builder/wits-starter.json";
import {
  prototypeBuilder,
  type BlockKind,
  type ImportedStarter,
  type PrototypeBlock,
  type SavedVariant,
  type Variant,
} from "../../api/prototype-builder";
import { notify } from "../use-notifications";
import { createBlock, fromStarter, insert, openVariant, remove, reorder, update } from "../../utils/prototype-builder/document";

// Map native codes to recovery steps the user can take in the workspace.
const messages: Record<string, string> = {
  invalidUrl: "Enter a GitHub repository URL in the form https://github.com/owner/repo.",
  missingStarter: "GitHub did not return examples/prototype-builder/wits-starter.json. Use a public repo with that file on its default branch, or open the bundled sample.",
  invalidStarter: "Check that the starter uses version 1 and has titled cards with unique IDs.",
  importUnavailable: "Check your connection and retry the import.",
  invalidDocument: "Fill in all heading text, card titles, sign-in field labels, and button labels, then try saving again.",
  invalidName: "Enter a name for this variant and retry.",
  storageUnavailable: "Check that WITS can access its app data folder and retry.",
};

export function usePrototypeBuilder() {
  // Track each request separately so failures stay beside the action that owns them.
  const source = ref("");
  const draft = ref<Variant | null>(null);
  const variants = ref<SavedVariant[]>([]);
  const busy = ref(false);
  const loading = ref(false);
  const importError = ref("");
  const saveError = ref("");
  const listError = ref("");
  const dirty = ref(false);
  let listRequest = 0;

  // Show only known action messages, keeping native diagnostics out of the UI.
  function message(error: unknown, fallback: string) {
    return typeof error === "string" && Object.prototype.hasOwnProperty.call(messages, error)
      ? messages[error]!
      : fallback;
  }

  // Replace the draft only after a successful import or a confirmed user action.
  function setDraft(variant: Variant, unsaved: boolean) {
    draft.value = variant;
    saveError.value = "";
    dirty.value = unsaved;
  }

  function startDraft({ sourceUrl, starter }: ImportedStarter) {
    setDraft({ sourceUrl, id: null, name: starter.title, document: fromStarter(starter) }, true);
  }

  // Start blank with a usable default name and no GitHub source.
  function newPrototype() {
    importError.value = "";
    setDraft({ id: null, name: "Untitled prototype", sourceUrl: null, document: { version: 2, blocks: [] } }, false);
  }

  async function importStarter() {
    if (busy.value) return false;
    busy.value = true;
    importError.value = "";
    try {
      startDraft(await prototypeBuilder.import(source.value));
      return true;
    } catch (error) {
      importError.value = message(error, "Try importing the starter again.");
      return false;
    } finally {
      busy.value = false;
    }
  }

  // Use the same licensed fixture as GitHub import, with no network dependency.
  function openSample() {
    importError.value = "";
    startDraft({
      sourceUrl: "https://github.com/Vincenthe3231/wits-tools",
      starter: { ...sample, version: 1 },
    });
  }

  // Only the newest disk read can refresh the list, including after a concurrent save.
  async function load() {
    const request = ++listRequest;
    loading.value = true;
    listError.value = "";
    try {
      const saved = await prototypeBuilder.list();
      if (request === listRequest) variants.value = saved;
    } catch (error) {
      if (request === listRequest) listError.value = message(error, "Try loading saved variants again.");
    } finally {
      if (request === listRequest) loading.value = false;
    }
  }

  // Convert legacy data in memory and leave the disk copy alone until Save.
  function open(variant: SavedVariant) {
    setDraft(openVariant(variant), false);
  }

  // Duplicate explicitly so Save on an existing draft updates its own snapshot.
  async function save(copy = false) {
    if (!draft.value || busy.value) return false;
    const snapshot = { ...draft.value, name: draft.value.name.trim() };
    busy.value = true;
    saveError.value = "";
    try {
      const id = await prototypeBuilder.save({ ...snapshot, id: copy ? null : snapshot.id });
      draft.value = { ...snapshot, id };
      dirty.value = false;
      notify({ severity: "success", title: "Variant saved" });
      await load();
      return true;
    } catch (error) {
      saveError.value = message(error, "Try saving this variant again.");
      return false;
    } finally {
      busy.value = false;
    }
  }

  // Keep the editor and preview in sync and mark only successful edits as dirty.
  function changeBlocks(transform: (blocks: PrototypeBlock[]) => PrototypeBlock[]) {
    if (!draft.value || busy.value) return false;
    const blocks = transform(draft.value.document.blocks);
    if (blocks === draft.value.document.blocks) return false;
    draft.value.document.blocks = blocks;
    dirty.value = true;
    saveError.value = "";
    return true;
  }

  // One state API serves drag gestures, property inputs and accessible block actions.
  function insertBlock(kind: BlockKind, index?: number) {
    const id = crypto.randomUUID();
    const added = changeBlocks((blocks) => insert(blocks, createBlock(kind, id), index ?? blocks.length));
    return added ? id : undefined;
  }

  function moveBlock(id: string, destination: number) {
    return changeBlocks((blocks) => reorder(blocks, id, destination));
  }

  function updateBlock(block: PrototypeBlock) {
    return changeBlocks((blocks) => update(blocks, block));
  }

  function removeBlock(id: string) {
    return changeBlocks((blocks) => remove(blocks, id));
  }

  function renameDraft(name: string) {
    if (!draft.value || busy.value || draft.value.name === name) return;
    draft.value.name = name;
    dirty.value = true;
    saveError.value = "";
  }

  return {
    source, draft, variants, busy, loading, importError, saveError, listError, dirty,
    newPrototype, importStarter, openSample, load, open, save,
    insertBlock, moveBlock, updateBlock, removeBlock, renameDraft,
  };
}

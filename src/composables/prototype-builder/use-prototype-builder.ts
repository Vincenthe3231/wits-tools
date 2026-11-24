/** Owns the active canvas and coordinates starter import and snapshot requests. */
import { computed, ref, shallowRef } from "vue";

import sample from "../../../examples/prototype-builder/wits-starter.json";
import {
  prototypeBuilder,
  type LinearDocument,
  type PrototypeDocument,
  type ImportedStarter,
  type PrototypeBlock,
  type SavedVariant,
  type Variant,
} from "../../api/prototype-builder";
import { notify } from "../use-notifications";
import { fromStarter, openVariant } from "../../utils/prototype-builder/document";
import { emptyCanvas } from "../../utils/prototype-builder/geometry";
import { applyEdit, type EditCommand } from "../../utils/prototype-builder/engine";
import { currentTheme } from "../../utils/prototype-builder/appearance";

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

export function usePrototypeBuilder(convert: (document: LinearDocument) => Promise<PrototypeDocument>,
  repository: Pick<typeof prototypeBuilder, "import" | "save" | "list"> = prototypeBuilder) {
  // Track each request separately so failures stay beside the action that owns them.
  const source = ref("");
  const draft = ref<Variant | null>(null);
  const variants = ref<SavedVariant[]>([]);
  const busy = ref(false);
  const loading = ref(false);
  const importError = ref("");
  const saveError = ref("");
  const listError = ref("");
  // Store immutable draft snapshots so ending a gesture produces a single undo step.
  const past = ref<Variant[]>([]);
  const future = ref<Variant[]>([]);
  const checkpoint = ref<string | null>(null);
  const fieldStart = shallowRef<Variant>();
  const fingerprint = (variant: Variant) => JSON.stringify([variant.name.trim(), variant.document]);
  const dirty = computed(() => !!draft.value && fingerprint(draft.value) !== checkpoint.value);
  let listRequest = 0;

  // Show only known action messages, keeping native diagnostics out of the UI.
  function message(error: unknown, fallback: string) {
    return typeof error === "string" && Object.prototype.hasOwnProperty.call(messages, error)
      ? messages[error]!
      : fallback;
  }

  // Replace the draft only after a successful import or a confirmed user action.
  function setDraft(variant: Variant, unsaved: boolean) {
    if (variant.document.version !== 4) {
      variant = { ...variant, document: { ...variant.document, version: 4, theme: currentTheme(), definitions: [] } };
    }
    draft.value = variant;
    saveError.value = "";
    checkpoint.value = unsaved ? null : fingerprint(variant);
    past.value = [];
    future.value = [];
    fieldStart.value = undefined;
  }

  async function startDraft({ sourceUrl, starter }: ImportedStarter) {
    const document = await convert(fromStarter(starter));
    setDraft({ sourceUrl, id: null, name: starter.title, document }, true);
  }

  // Start blank with a usable default name and no GitHub source.
  function newPrototype() {
    importError.value = "";
    setDraft({ id: null, name: "Untitled prototype", sourceUrl: null, document: emptyCanvas() }, false);
  }

  async function importStarter() {
    if (busy.value) return false;
    busy.value = true;
    importError.value = "";
    try {
      await startDraft(await repository.import(source.value));
      return true;
    } catch (error) {
      importError.value = message(error, "Try importing the starter again.");
      return false;
    } finally {
      busy.value = false;
    }
  }

  // Use the same licensed fixture as GitHub import, with no network dependency.
  async function openSample() {
    importError.value = "";
    busy.value = true;
    try {
      await startDraft({
        sourceUrl: "https://github.com/Vincenthe3231/wits-tools",
        starter: { ...sample, version: 1 },
      });
      return true;
    } catch {
      importError.value = "Could not lay out the bundled sample. Try opening it again.";
      return false;
    } finally {
      busy.value = false;
    }
  }

  // Only the newest disk read can refresh the list, including after a concurrent save.
  async function load() {
    const request = ++listRequest;
    loading.value = true;
    listError.value = "";
    try {
      const saved = await repository.list();
      if (request === listRequest) variants.value = saved;
    } catch (error) {
      if (request === listRequest) listError.value = message(error, "Try loading saved variants again.");
    } finally {
      if (request === listRequest) loading.value = false;
    }
  }

  // Convert legacy data in memory and leave the disk copy alone until Save.
  async function open(variant: SavedVariant) {
    busy.value = true;
    listError.value = "";
    try {
      const opened = openVariant(variant);
      const document = "nodes" in opened.document ? opened.document : await convert(opened.document);
      setDraft({ ...opened, document }, false);
      return true;
    } catch {
      listError.value = "Could not lay out this variant. Reload saved variants to try again.";
      return false;
    } finally {
      busy.value = false;
    }
  }

  // Duplicate explicitly so Save on an existing draft updates its own snapshot.
  async function save(copy = false) {
    if (!draft.value || busy.value) return false;
    finishField();
    const snapshot = { ...draft.value, name: draft.value.name.trim() };
    busy.value = true;
    saveError.value = "";
    try {
      const id = await repository.save({ ...snapshot, id: copy ? null : snapshot.id });
      draft.value = { ...snapshot, id };
      checkpoint.value = fingerprint(snapshot);
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

  // Group live text changes until the field loses focus or another command runs.
  function remember(before: Variant) {
    if (!draft.value || fingerprint(before) === fingerprint(draft.value)) return;
    past.value = [...past.value.slice(-99), before];
    future.value = [];
  }

  function finishField() {
    if (!fieldStart.value) return;
    remember(fieldStart.value);
    fieldStart.value = undefined;
  }

  function edit(change: (variant: Variant) => Variant, live = false) {
    if (!draft.value || busy.value) return false;
    if (!live) finishField();
    const before = draft.value;
    const after = change(before);
    if (after === before) return false;
    if (live) fieldStart.value ??= before;
    draft.value = after;
    if (!live) remember(before);
    saveError.value = "";
    return true;
  }

  // The save identity belongs to the active draft even when undo crosses a save.
  function travel(redo = false) {
    if (!draft.value || busy.value) return;
    finishField();
    const from = redo ? future : past;
    const to = redo ? past : future;
    const snapshot = from.value.pop();
    if (!snapshot) return;
    to.value.push(draft.value);
    draft.value = { ...snapshot, id: draft.value.id };
    saveError.value = "";
  }

  const editingField = computed(() => !!fieldStart.value && !!draft.value
    && fingerprint(fieldStart.value) !== fingerprint(draft.value));
  const canUndo = computed(() => past.value.length > 0 || editingField.value);
  const canRedo = computed(() => future.value.length > 0 && !editingField.value);
  const dispatch = (command: EditCommand, live = false) => edit((variant) => {
    const document = applyEdit(variant.document, command);
    return document === variant.document ? variant : { ...variant, document };
  }, live);
  const updateBlock = (block: PrototypeBlock) => dispatch({ type: "content", block }, true);
  const renameDraft = (name: string) => edit((variant) => ({ ...variant, name }), true);

  return {
    source, draft, variants, busy, loading, importError, saveError, listError, dirty,
    newPrototype, importStarter, openSample, load, open, save,
    dispatch, updateBlock, renameDraft, finishField, travel, canUndo, canRedo,
  };
}

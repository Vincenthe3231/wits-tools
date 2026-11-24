<!-- Coordinates the freeform workspace, its inspector, and project actions. -->
<script setup lang="ts">
import { computed, nextTick, onMounted, onScopeDispose, ref } from "vue";
import { onBeforeRouteLeave } from "vue-router";
import { DialogContent, DialogDescription, DialogOverlay, DialogPortal, DialogRoot, DialogTitle } from "reka-ui";

import { prototypeBuilder, type BlockKind, type CanvasNode, type ComponentDefinition, type ComponentReview,
  type LinearDocument, type NodeGeometry, type SavedVariant } from "../../api/prototype-builder";
import PrototypeAppearance from "../../components/prototype-builder/PrototypeAppearance.vue";
import PrototypeComponentView from "../../components/prototype-builder/PrototypeComponentView.vue";
import PrototypeCanvas from "../../components/prototype-builder/PrototypeCanvas.vue";
import PrototypeBlockProperties from "../../components/prototype-builder/PrototypeBlockProperties.vue";
import PrototypeBlockView from "../../components/prototype-builder/PrototypeBlockView.vue";
import { usePrototypeBuilder } from "../../composables/prototype-builder/use-prototype-builder";
import { createBlock, definitionKey } from "../../utils/prototype-builder/document";
import { currentTheme, themeCss } from "../../utils/prototype-builder/appearance";
import { blockWidth, coordinate, MAX_COORDINATE, MAX_DIMENSION, MIN_COORDINATE, stackCanvas, type Point } from "../../utils/prototype-builder/geometry";

type CloseChoice = "save" | "discard" | "cancel";

// Keep viewport state and measurement separate from the saved document.
const palette = [
  { kind: "heading", label: "Heading" },
  { kind: "text", label: "Text" },
  { kind: "card", label: "Card" },
  { kind: "button", label: "Button" },
  { kind: "signIn", label: "Sign-in card" },
] as const;
const canvas = ref<InstanceType<typeof PrototypeCanvas>>();
const previewBack = ref<HTMLButtonElement>();
const selectedId = ref<string | null>(null);
const preview = ref(false);
const draftKey = ref(0);
const measuring = ref<LinearDocument>();
const measureHost = ref<HTMLElement>();

async function convert(document: LinearDocument) {
  measuring.value = document;
  try {
    await nextTick();
    const rows = Array.from(measureHost.value?.children ?? [], (element) => (element as HTMLElement).offsetHeight);
    return stackCanvas(document, rows);
  } finally {
    measuring.value = undefined;
  }
}

const {
  source, draft, variants, busy, loading, importError, saveError, listError, dirty,
  importStarter, openSample, load, open, save, newPrototype, dispatch, updateBlock,
  renameDraft, finishField, travel, canUndo, canRedo,
} = usePrototypeBuilder(convert);
const nodes = computed(() => draft.value?.document.nodes ?? []);
const layers = computed(() => [...nodes.value].reverse());
const selectedNode = computed(() => nodes.value.find((node) => node.block.id === selectedId.value));
const theme = computed(() => draft.value?.document.theme ?? currentTheme());
const findDefinition = (node: CanvasNode) => {
  const block = node.block;
  return block.kind === "imported"
    ? draft.value?.document.definitions?.find(({ id, revision }) => id === block.definitionId && revision === block.revision)
    : undefined;
};
const selectedDefinition = computed(() => selectedNode.value ? findDefinition(selectedNode.value) : undefined);
const layerActions = [
  { action: "front", label: "Bring to front" },
  { action: "raise", label: "Raise" },
  { action: "lower", label: "Lower" },
  { action: "back", label: "Send to back" },
] as const;

// Pointer and inspector changes go through the same history path.
function insertNode(source: BlockKind | ComponentDefinition, point: Point) {
  const id = crypto.randomUUID();
  const definition = typeof source === "string" ? undefined : source;
  const block = typeof source === "string" ? createBlock(source, id)
    : { id, kind: "imported" as const, definitionId: source.id, revision: source.revision, values: {} };
  const width = typeof source === "string" ? blockWidth(source) : source.defaultWidth;
  const node = { block, x: coordinate(point.x), y: coordinate(point.y), width };
  if (dispatch({ type: "insert", node, definition })) selectedId.value = id;
}

function moveNode(id: string, geometry: NodeGeometry) {
  dispatch({ type: "geometry", id, geometry });
}

function changeGeometry(event: Event, node: CanvasNode, field: keyof NodeGeometry) {
  const input = event.target as HTMLInputElement;
  if (!input.validity.valid) {
    input.value = String(node[field] ?? "");
    return;
  }
  moveNode(node.block.id, { [field]: input.value === "" ? undefined : input.valueAsNumber });
}

function changeScreen(event: Event, field: "width" | "height") {
  const input = event.target as HTMLInputElement;
  if (!input.validity.valid) {
    input.value = String(draft.value?.document.screen[field]);
    return;
  }
  dispatch({ type: "screen", size: { [field]: input.valueAsNumber } });
}

function removeSelected(id: string) {
  if (!dispatch({ type: "remove", id })) return;
  selectedId.value = null;
  canvas.value?.focus();
}

function order(action: typeof layerActions[number]["action"]) {
  const id = selectedId.value;
  if (id) dispatch({ type: "layer", id, order: action });
}

function history(redo = false) {
  canvas.value?.cancel();
  travel(redo);
  if (!selectedNode.value) selectedId.value = null;
}

function layerName(node: CanvasNode) {
  const block = node.block;
  if (block.kind === "imported") return findDefinition(node)?.name ?? block.definitionId;
  const copy = "title" in block ? block.title : "text" in block ? block.text : block.label;
  return copy.trim() || palette.find(({ kind }) => kind === block.kind)?.label;
}

// Library review and installation never replace or edit the current draft.
const library = ref<ComponentDefinition[]>([]);
const review = ref<ComponentReview>();
const libraryBusy = ref(false);
const libraryError = ref("");
const reviewValues = computed(() => Object.fromEntries(Object.entries(review.value?.definition?.fields ?? {})
  .map(([key, field]) => [key, field.default])));

async function loadComponents() {
  library.value = await prototypeBuilder.components();
}

async function libraryAction(action: () => Promise<void>) {
  if (libraryBusy.value) return;
  libraryBusy.value = true;
  libraryError.value = "";
  try {
    await action();
  } catch {
    libraryError.value = "Could not complete the library action. Try again.";
  } finally {
    libraryBusy.value = false;
  }
}

function chooseComponent() {
  return libraryAction(async () => {
    const result = await prototypeBuilder.chooseComponent();
    if (result) review.value = result;
  });
}

function installComponent() {
  const definition = review.value?.definition;
  if (!definition) return;
  return libraryAction(async () => {
    const result = await prototypeBuilder.installComponent(definition);
    review.value = result.diagnostics.length ? result : undefined;
    await loadComponents();
  });
}

function removeComponent(definition: ComponentDefinition) {
  return libraryAction(async () => {
    await prototypeBuilder.removeComponent(definition.id, definition.revision);
    await loadComponents();
  });
}

function changeImportedField(key: string, event: Event) {
  const block = selectedNode.value?.block;
  if (block?.kind !== "imported") return;
  const input = event.target as HTMLInputElement;
  const value = input.value;
  const defaultValue = selectedDefinition.value?.fields[key]?.default;
  if (new TextEncoder().encode(value).length > 2000) {
    input.value = importedValue(block.values, key, defaultValue ?? "");
    return;
  }
  const values = { ...block.values };
  if (value === defaultValue) delete values[key];
  else values[key] = value;
  updateBlock({ ...block, values });
}

function importedValue(values: Record<string, string>, key: string, fallback: string) {
  return Object.prototype.hasOwnProperty.call(values, key) ? values[key] ?? fallback : fallback;
}

// Keep dirty drafts intact until the user confirms a replacement or route change.
const confirmOpen = ref(false);
let resolveReplacement: ((confirmed: boolean) => void) | undefined;
let replacementTrigger: HTMLElement | undefined;

function answerReplacement(confirmed: boolean) {
  const resolve = resolveReplacement;
  resolveReplacement = undefined;
  confirmOpen.value = false;
  resolve?.(confirmed);
}

function mayReplace(): boolean | Promise<boolean> {
  if (busy.value || confirmOpen.value) return false;
  canvas.value?.cancel();
  finishField();
  if (!dirty.value) return true;

  const focused = document.activeElement;
  replacementTrigger = focused instanceof HTMLElement ? focused : undefined;
  confirmOpen.value = true;
  return new Promise<boolean>((resolve) => { resolveReplacement = resolve; });
}

// Restore the invoking control unless a successful open already focused the new canvas.
function restoreReplacementFocus(event: Event) {
  event.preventDefault();
  if (document.activeElement === document.body && replacementTrigger?.isConnected) {
    replacementTrigger.focus();
  }
  replacementTrigger = undefined;
}

onBeforeRouteLeave(mayReplace);
onScopeDispose(() => answerReplacement(false));
onMounted(load);
onMounted(() => libraryAction(loadComponents));

// Clear editor-only state once an open or import has actually succeeded.
function enterDraft() {
  preview.value = false;
  selectedId.value = null;
  draftKey.value++;
  void nextTick(() => canvas.value?.focus());
}

async function requestNew() {
  if (!(await mayReplace())) return;
  newPrototype();
  enterDraft();
}

async function requestImport() {
  if (!(await mayReplace())) return;
  if (await importStarter()) enterDraft();
}

async function requestSample() {
  if (!(await mayReplace())) return;
  if (await openSample()) enterDraft();
}

async function requestOpen(variant: SavedVariant) {
  if (!(await mayReplace())) return;
  if (await open(variant)) enterDraft();
}

// Switch surfaces without changing the document or retaining a drag gesture.
function togglePreview() {
  canvas.value?.cancel();
  finishField();
  preview.value = !preview.value;
  if (preview.value) void nextTick(() => previewBack.value?.focus());
  else void nextTick(() => canvas.value?.focus());
}

// The shell owns the native close listener and delegates the active draft decision.
async function requestClose(askClose: () => Promise<CloseChoice>) {
  if (busy.value || confirmOpen.value) return false;
  canvas.value?.cancel();
  finishField();
  if (!dirty.value) return true;

  const decision = await askClose();
  if (decision === "discard") return true;
  if (decision !== "save") return false;
  const saved = await save();
  if (!saved) preview.value = false;
  return saved;
}

defineExpose({ requestClose });
</script>

<template>
  <div class="builder tool-workspace" @focusout="finishField">
    <!-- The hidden editor retains its camera while Preview uses fresh form controls. -->
    <section v-if="preview && draft" class="panel preview-mode" aria-label="Prototype preview">
      <button ref="previewBack" class="builder-button" type="button" @click="togglePreview">
        Back to editor
      </button>
      <PrototypeCanvas :document="draft.document" preview />
    </section>

    <div v-show="!preview" class="editor-content">      <!-- Keep project entry points together and errors beside the import action. -->
      <section class="panel">
        <div class="controls">
          <button class="builder-button primary-button" type="button" :disabled="busy" @click="requestNew">
            New prototype
          </button>
          <button class="builder-button" type="button" :disabled="busy" @click="requestSample">
            Open bundled sample
          </button>
        </div>
        <form class="field" @submit.prevent="requestImport">
          <label for="prototype-source">GitHub repository URL</label>
          <div class="controls">
            <input
              id="prototype-source"
              v-model="source"
              placeholder="https://github.com/owner/repo"
              required
              :disabled="busy"
            />
            <button class="builder-button" type="submit" :disabled="busy">Import GitHub starter</button>
          </div>
          <p class="muted">GitHub import requires a WITS starter file.</p>
          <p v-if="importError" class="error" role="alert">{{ importError }}</p>
        </form>
      </section>

      <!-- Both the palette and existing canvas blocks feed the same insertion slots. -->
      <section class="panel" aria-label="Component library">
        <div class="controls">
          <button type="button" class="builder-button" :disabled="libraryBusy || busy" @click="chooseComponent">Import component file</button>
          <button v-if="libraryError" type="button" class="builder-button" :disabled="libraryBusy" @click="libraryAction(loadComponents)">Reload library</button>
        </div>
        <p v-if="libraryError" class="error" role="alert">{{ libraryError }}</p>
        <!-- Keep review separate from palette installation and locate errors in the selected file. -->
        <div v-if="review" class="field">
          <h2>{{ review.name }}</h2>
          <ul v-if="review.diagnostics.length" class="field" aria-label="Import diagnostics" role="alert">
            <li v-for="(diagnostic, index) in review.diagnostics" :key="index">
              <strong>{{ diagnostic.code }}</strong>
              <span v-if="diagnostic.line !== undefined"> · Line {{ diagnostic.line }}, column {{ diagnostic.column }}</span>
              <code v-if="diagnostic.path"> {{ diagnostic.path }}</code>
              <p>{{ diagnostic.message }}</p>
            </li>
          </ul>
          <template v-else-if="review.definition">
            <div class="component-review" :style="{ ...themeCss(theme), width: review.definition.defaultWidth + 'px' }">
              <PrototypeComponentView :element="review.definition.root" :values="reviewValues" :theme="theme" />
            </div>
            <div class="controls">
              <button class="builder-button primary-button" type="button" :disabled="libraryBusy" @click="installComponent">Add to palette</button>
              <button class="builder-button" type="button" :disabled="libraryBusy" @click="review = undefined">Cancel review</button>
            </div>
          </template>
          <button v-if="review.diagnostics.length" class="builder-button" type="button" @click="review = undefined">Dismiss diagnostics</button>
        </div>
      </section>

      <template v-if="draft">
        <!-- The palette and layer list leave the workspace free for direct manipulation. -->
        <section class="panel palette">
          <h2>Components</h2>
          <ul class="palette-items">
            <li v-for="item in palette" :key="item.kind" class="palette-item">
              <span class="drag-handle" @pointerdown="canvas?.startPalette($event, item.kind)">
                <span aria-hidden="true">⠿</span> {{ item.label }}
              </span>
              <button
                class="builder-button"
                type="button"
                :aria-label="'Add ' + item.label"
                :disabled="busy"
                @click="canvas?.add(item.kind)"
              >Add</button>
            </li>
            <li v-for="definition in library" :key="definitionKey(definition)" class="palette-item">
              <span class="drag-handle" @pointerdown="canvas?.startPalette($event, definition)">
                <span aria-hidden="true">⠿</span> {{ definition.name }} · v{{ definition.revision }}
              </span>
              <button type="button" class="builder-button" :disabled="busy" :aria-label="'Add ' + definition.name + ' revision ' + definition.revision" @click="canvas?.add(definition)">Add</button>
              <button type="button" class="builder-button" :disabled="libraryBusy" :aria-label="'Remove ' + definition.name + ' revision ' + definition.revision + ' from palette'" @click="removeComponent(definition)">Remove</button>
            </li>
          </ul>
        </section>

        <div class="editor-grid">
          <section class="panel canvas-panel">
            <div class="controls">
              <button class="builder-button" type="button" :disabled="busy || !canUndo" @click="history()">Undo</button>
              <button class="builder-button" type="button" :disabled="busy || !canRedo" @click="history(true)">Redo</button>
              <button class="builder-button primary-button" type="button" :disabled="busy" @click="togglePreview">Preview</button>
            </div>
            <PrototypeCanvas
              :key="draftKey"
              ref="canvas"
              :document="draft.document"
              :selected-id="selectedId"
              :disabled="busy"
              @select="selectedId = $event"
              @insert="insertNode"
              @move="moveNode"
              @history="history"
            />
          </section>

          <aside class="inspector">
            <section class="panel">
              <h2>Layers</h2>
              <p v-if="!layers.length" class="muted">Add a component to start.</p>
              <ul v-else class="layer-list">
                <li v-for="node in layers" :key="node.block.id">
                  <button
                    type="button"
                    :aria-pressed="selectedId === node.block.id"
                    :disabled="busy"
                    @click="selectedId = node.block.id"
                  >{{ layerName(node) }}</button>
                </li>
              </ul>
              <div v-if="selectedNode" class="controls">
                <button
                  v-for="{ action, label } in layerActions"
                  :key="action"
                  type="button"
                  class="builder-button"
                  :disabled="busy"
                  @click="order(action)"
                >{{ label }}</button>
              </div>
            </section>
            <section class="panel">
              <h2>Properties</h2>
              <fieldset class="geometry-fields" :disabled="busy">
                <legend>Screen</legend>
                <label v-for="field in (['width', 'height'] as const)" :key="field" class="field">
                  {{ field === "width" ? "Screen width" : "Screen height" }}
                  <input type="number" required min="1" :max="MAX_DIMENSION" :value="draft.document.screen[field]" @change="changeScreen($event, field)" />
                </label>
              </fieldset>
              <details>
                <summary>Project theme</summary>
                <PrototypeAppearance :theme="theme" :disabled="busy" @theme="dispatch({ type: 'theme', theme: $event })" />
              </details>
              <template v-if="selectedNode">
                <fieldset class="geometry-fields" :key="selectedNode.block.id" :disabled="busy">
                  <legend>Component</legend>
                  <label v-for="field in (['x', 'y', 'width'] as const)" :key="field" class="field">
                    {{ field === "width" ? "Component width" : field.toUpperCase() }}
                    <input
                      type="number"
                      required
                      :min="field === 'width' ? 1 : MIN_COORDINATE"
                      :max="field === 'width' ? MAX_DIMENSION : MAX_COORDINATE"
                      :value="selectedNode[field]"
                      @change="changeGeometry($event, selectedNode, field)"
                    />
                  </label>
                  <!-- Clearing minimum height returns the component to automatic sizing. -->
                  <label class="field">
                    Minimum height
                    <input
                      type="number"
                      min="1"
                      :max="MAX_DIMENSION"
                      :value="selectedNode.height ?? ''"
                      placeholder="Auto"
                      @change="changeGeometry($event, selectedNode, 'height')"
                    />
                  </label>
                </fieldset>
                <PrototypeBlockProperties
                  v-if="selectedNode.block.kind !== 'imported'"
                  :key="selectedNode.block.id"
                  :block="selectedNode.block"
                  :disabled="busy"
                  @update="updateBlock"
                  @remove="removeSelected"
                />
                <template v-else>
                  <label v-for="(field, key) in selectedDefinition?.fields" :key="key" class="field">
                    {{ field.label }}
                    <input :value="importedValue(selectedNode.block.values, key, field.default)" :disabled="busy" @input="changeImportedField(key, $event)" />
                  </label>
                  <button type="button" class="builder-button" :disabled="busy" @click="removeSelected(selectedNode.block.id)">Remove block</button>
                </template>
                <details :key="'appearance-' + selectedNode.block.id">
                  <summary>Component appearance</summary>
                  <PrototypeAppearance
                    :theme="theme"
                    :node="selectedNode"
                    :definition="selectedDefinition"
                    :disabled="busy"
                    @style="dispatch({ type: 'style', id: selectedNode.block.id, style: $event })"
                  />
                </details>
              </template>
            </section>
          </aside>
        </div>
        <!-- Explicit saves keep unsaved draft content local to the editor. -->
        <section class="panel">
          <form class="field" @submit.prevent="save()">
            <label for="prototype-name">Variant name</label>
            <div class="controls">
              <input
                id="prototype-name"
                :value="draft.name"
                required
                :disabled="busy"
                @input="renameDraft(($event.target as HTMLInputElement).value)"
              />
              <button type="submit" class="builder-button primary-button" :disabled="busy">Save variant</button>
              <button v-if="draft.id" type="button" class="builder-button" :disabled="busy" @click="save(true)">
                Save as new
              </button>
            </div>
            <p v-if="saveError" class="error" role="alert">{{ saveError }}</p>
          </form>
        </section>
      </template>

      <!-- Saved variants remain a local entry point even when GitHub is unavailable. -->
      <section class="panel">
        <h2>Saved variants</h2>
        <p v-if="loading" class="muted" role="status">Loading variants…</p>
        <div v-else-if="listError" class="controls">
          <p class="error" role="alert">{{ listError }}</p>
          <button class="builder-button" type="button" @click="load">Retry</button>
        </div>
        <p v-else-if="!variants.length" class="muted">No saved variants yet.</p>
        <ul v-else class="saved-variants">
          <li v-for="variant in variants" :key="variant.id" class="saved-variant">
            <span>{{ variant.name }}</span>
            <button
              class="builder-button"
              type="button"
              :disabled="busy"
              :aria-label="'Open ' + variant.name"
              @click="requestOpen(variant)"
            >Open</button>
          </li>
        </ul>
      </section>
    </div>

    <!-- Measure legacy content at real document widths before replacing the draft. -->
    <div v-if="measuring" ref="measureHost" class="measurement tool-workspace" aria-hidden="true" inert>
      <div
        v-for="block in measuring.blocks"
        :key="block.id"
        class="prototype-node-content"
        :style="{ width: blockWidth(block.kind) + 'px' }"
      >
        <PrototypeBlockView :block="block" />
      </div>
    </div>
    <!-- Escape and outside dismissal cancel the pending replacement. -->
    <DialogRoot :open="confirmOpen" @update:open="!$event && answerReplacement(false)">
      <DialogPortal>
        <DialogOverlay class="prototype-discard-overlay" />
        <DialogContent class="prototype-discard-dialog dialog-content" @close-auto-focus="restoreReplacementFocus">
          <DialogTitle>Discard unsaved changes?</DialogTitle>
          <DialogDescription>Your edits will be lost if you continue.</DialogDescription>
          <div class="dialog-actions">
            <button class="builder-button" type="button" @click="answerReplacement(false)">Cancel</button>
            <button class="builder-button" type="button" @click="answerReplacement(true)">Discard changes</button>
          </div>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>
  </div>
</template>

<style scoped>
/* Give the canvas room to work and keep supporting panels readable at narrow widths. */
.builder,
.editor-content,
.panel,
.field,
.inspector,
.layer-list,
.saved-variants {
  display: grid;
  min-width: 0;
  gap: 0.65rem;
}

.panel {
  padding: 0.8rem;
  border-radius: 0.85rem;
  background: var(--surface-editor);
}

.editor-grid {
  display: grid;
  min-width: 0;
  align-items: start;
  gap: 0.75rem;
}

@container tool-workspace (min-width: 47rem) {
  .editor-grid {
    grid-template-columns: minmax(0, 1fr) minmax(12rem, 16rem);
  }
}

.controls,
.palette-items,
.palette-item,
.saved-variant {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.saved-variant {
  justify-content: space-between;
}

.palette-item {
  padding-inline: 0.4rem;
  border-radius: 0.75rem;
  background: var(--surface-toolbar);
}

/* Bound review width to available space while using the authored theme. */
.component-review {
  max-width: 100%;
  padding: 0.75rem;
  border-radius: 0.75rem;
}

.drag-handle {
  display: inline-flex;
  align-items: center;
  min-height: 44px;
  gap: 0.4rem;
  cursor: grab;
  touch-action: none;
  user-select: none;
}

/* Use the same field and focus treatment as the rest of the tool. */
.panel h2,
.geometry-fields legend {
  font-weight: 650;
}

.geometry-fields {
  display: grid;
  min-width: 0;
  gap: 0.5rem;
}

.layer-list button {
  width: 100%;
  min-height: 44px;
  overflow-wrap: anywhere;
  text-align: start;
}

.field input,
.builder-button {
  min-height: 44px;
  padding-inline: 0.75rem;
  border-radius: var(--tool-field-radius);
}

.field input {
  width: 100%;
  min-width: 0;
  border: 1px solid var(--text-muted);
  background: var(--surface-editor);
}

.field .controls input {
  flex: 1 1 14rem;
}

.muted {
  color: var(--text-muted);
  font-size: 0.875rem;
}

/* Reserve the available height for the fitted screen and size old blocks offscreen. */
.preview-mode {
  grid-template-rows: auto minmax(0, 1fr);
  height: max(13rem, calc(100dvh - 7rem));
}

.preview-mode > button {
  justify-self: start;
}

.measurement {
  position: fixed;
  left: 0;
  top: 0;
  visibility: hidden;
  pointer-events: none;
}
.error {
  padding: 0.5rem;
  border: 1px solid var(--status-error-indicator);
  border-radius: 0.5rem;
  background: var(--status-error-surface);
  color: var(--status-error-text);
}

/* Match the existing close dialog's surface and keep it above the canvas. */
:global(.prototype-discard-overlay) {
  position: fixed;
  z-index: 20;
  inset: 0;
  background: #0008;
}

:global(.prototype-discard-dialog) {
  position: fixed;
  z-index: 21;
  top: 50%;
  left: 50%;
  width: min(26rem, calc(100% - 2rem));
  border: 1px solid var(--text-muted);
  border-radius: 0.65rem;
  background: var(--surface-editor);
  color: var(--text-primary);
  transform: translate(-50%, -50%);
}

</style>

<!-- Composes a prototype on a vertical canvas and keeps preview and project actions together. -->
<script setup lang="ts">
import { computed, nextTick, onMounted, onScopeDispose, ref } from "vue";
import { onBeforeRouteLeave } from "vue-router";
import { DialogContent, DialogDescription, DialogOverlay, DialogPortal, DialogRoot, DialogTitle } from "reka-ui";

import type { BlockKind, PrototypeBlock, SavedVariant } from "../../api/prototype-builder";
import PrototypeBlockActions from "../../components/prototype-builder/PrototypeBlockActions.vue";
import PrototypeBlockProperties from "../../components/prototype-builder/PrototypeBlockProperties.vue";
import PrototypeBlockView from "../../components/prototype-builder/PrototypeBlockView.vue";
import { usePrototypeBuilder } from "../../composables/prototype-builder/use-prototype-builder";
import { usePrototypeDrag } from "../../composables/prototype-builder/use-prototype-drag";
import { createBlock } from "../../utils/prototype-builder/document";

type CloseChoice = "save" | "discard" | "cancel";

// Keep editor selection and gesture state separate from the persisted document.
const {
  source, draft, variants, busy, loading, importError, saveError, listError, dirty,
  importStarter, openSample, load, open, save, newPrototype, insertBlock, moveBlock,
  updateBlock, removeBlock, renameDraft,
} = usePrototypeBuilder();
const palette = [
  { kind: "heading", label: "Heading" },
  { kind: "text", label: "Text" },
  { kind: "card", label: "Card" },
  { kind: "button", label: "Button" },
  { kind: "signIn", label: "Sign-in card" },
] as const;
const canvas = ref<HTMLElement | null>(null);
const previewBack = ref<HTMLButtonElement | null>(null);
const selectedId = ref<string | null>(null);
const preview = ref(false);
const announcement = ref("");
const blocks = computed(() => draft.value?.document.blocks ?? []);
const selectedBlock = computed(() => blocks.value.find((block) => block.id === selectedId.value));

// Use the rendered canvas order for both new insertions and existing-block moves.
const { beginDrag, cancelDrag, activeDrag, insertionTop } = usePrototypeDrag({
  canvas,
  onDrop(source, slot) {
    if (busy.value || preview.value) return;
    if (source.kind === "palette") {
      addBlock(source.blockKind, slot, true);
      return;
    }

    const sourceIndex = blocks.value.findIndex((block) => block.id === source.id);
    if (sourceIndex >= 0) moveTo(source.id, slot > sourceIndex ? slot - 1 : slot, true);
  },
});

const dragBlock = computed(() => {
  const source = activeDrag.value?.source;
  if (!source) return undefined;
  return source.kind === "palette"
    ? createBlock(source.blockKind, "drag-preview")
    : blocks.value.find((block) => block.id === source.id);
});

// Keep pointer drops in view while letting keyboard actions scroll to the focused block.
function focusBlock(id: string, preventScroll = false) {
  void nextTick(() => {
    const rows = canvas.value?.querySelectorAll<HTMLElement>("[data-prototype-block]");
    const row = Array.from(rows ?? []).find((entry) => entry.dataset.prototypeBlock === id);
    row?.querySelector<HTMLButtonElement>("[data-prototype-select]")?.focus({ preventScroll });
  });
}

function blockLabel(block: PrototypeBlock) {
  switch (block.kind) {
    case "heading": return "Heading";
    case "text": return "Text";
    case "card": return "Card";
    case "button": return "Button";
    case "signIn": return "Sign-in card";
  }
}

function blockName(block: PrototypeBlock) {
  switch (block.kind) {
    case "heading":
    case "text": return block.text.trim() || blockLabel(block);
    case "card":
    case "signIn": return block.title.trim() || blockLabel(block);
    case "button": return block.label.trim() || "Button";
  }
}

function addBlock(kind: BlockKind, index?: number, preventScroll = false) {
  const id = insertBlock(kind, index);
  if (!id) return;
  selectedId.value = id;
  const label = palette.find((entry) => entry.kind === kind)?.label;
  announcement.value = `Added ${label} at position ${(index ?? blocks.value.length - 1) + 1}.`;
  focusBlock(id, preventScroll);
}

function moveTo(id: string, index: number, preventScroll = false) {
  if (!moveBlock(id, index)) return;
  selectedId.value = id;
  announcement.value = `Moved block to position ${index + 1} of ${blocks.value.length}.`;
  focusBlock(id, preventScroll);
}

function removeSelected(id: string) {
  const index = blocks.value.findIndex((block) => block.id === id);
  if (!removeBlock(id)) return;
  const next = blocks.value[Math.min(index, blocks.value.length - 1)];
  selectedId.value = next?.id ?? null;
  announcement.value = "Block removed.";
  if (next) focusBlock(next.id);
  else void nextTick(() => canvas.value?.focus());
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
  cancelDrag();
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

// Clear editor-only state once an open or import has actually succeeded.
function enterDraft() {
  preview.value = false;
  selectedId.value = null;
  announcement.value = "";
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
  openSample();
  enterDraft();
}

async function requestOpen(variant: SavedVariant) {
  if (!(await mayReplace())) return;
  open(variant);
  enterDraft();
}

// Switch surfaces without changing the document or retaining a drag gesture.
function togglePreview() {
  cancelDrag();
  preview.value = !preview.value;
  if (preview.value) void nextTick(() => previewBack.value?.focus());
  else if (selectedId.value) focusBlock(selectedId.value);
  else void nextTick(() => canvas.value?.focus());
}

// The shell owns the native close listener and delegates the active draft decision.
async function requestClose(askClose: () => Promise<CloseChoice>) {
  if (busy.value || confirmOpen.value) return false;
  cancelDrag();
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
  <div class="builder tool-workspace">
    <!-- A separate preview surface contains only authored content and an exit control. -->
    <section v-if="preview && draft" class="panel preview-mode" aria-label="Prototype preview">
      <button ref="previewBack" class="builder-button" type="button" @click="togglePreview">
        Back to editor
      </button>
      <div class="preview-screen">
        <p v-if="!blocks.length" class="muted">This screen is empty.</p>
        <PrototypeBlockView v-for="block in blocks" :key="block.id" :block="block" interactive />
      </div>
    </section>

    <template v-else>
      <!-- Keep project entry points together and errors beside the import action. -->
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
      <template v-if="draft">
        <div class="editor-grid">
          <section class="panel palette">
            <h2>Components</h2>
            <ul class="palette-items">
              <li v-for="item in palette" :key="item.kind" class="palette-item">
                <span
                  class="drag-handle palette-handle"
                  @pointerdown="!busy && beginDrag($event, { kind: 'palette', blockKind: item.kind, label: item.label })"
                >
                  <span class="grip" aria-hidden="true">⠿</span>
                  {{ item.label }}
                </span>
                <button
                  class="builder-button"
                  type="button"
                  :aria-label="'Add ' + item.label"
                  :disabled="busy"
                  @click="addBlock(item.kind)"
                >Add</button>
              </li>
            </ul>
          </section>

          <section class="panel canvas-panel">
            <div class="section-title">
              <h2 id="prototype-canvas-title">Canvas</h2>
              <button class="builder-button" type="button" :disabled="busy" @click="togglePreview">
                Preview
              </button>
            </div>
            <p class="sr-only" role="status">{{ announcement }}</p>
            <div ref="canvas" class="canvas" role="region" aria-labelledby="prototype-canvas-title" tabindex="-1">
              <p v-if="!blocks.length" class="empty-canvas muted">Drag a component here or click Add.</p>
              <ol v-else class="canvas-blocks">
                <li
                  v-for="(block, index) in blocks"
                  :key="block.id"
                  :data-prototype-block="block.id"
                  class="canvas-block"
                  :class="{ 'source-block': activeDrag?.source.kind === 'block' && activeDrag.source.id === block.id }"
                >
                  <div class="block-controls">
                    <span
                      class="drag-handle block-grip"
                      aria-hidden="true"
                      @pointerdown="!busy && beginDrag($event, { kind: 'block', id: block.id, label: blockLabel(block) })"
                    >⠿</span>
                    <button
                      class="builder-button block-select"
                      data-prototype-select
                      type="button"
                      :aria-label="'Select ' + blockLabel(block) + ' at position ' + (index + 1) + ': ' + blockName(block)"
                      :aria-pressed="selectedId === block.id"
                      :disabled="busy"
                      @click="selectedId = block.id"
                    >{{ blockLabel(block) }}</button>
                    <PrototypeBlockActions
                      :position="index + 1"
                      :count="blocks.length"
                      :label="blockName(block)"
                      :disabled="busy"
                      @move="moveTo(block.id, $event)"
                    />
                  </div>
                  <PrototypeBlockView :block="block" />
                </li>
              </ol>
              <div
                v-if="insertionTop !== null"
                class="insertion-line"
                aria-hidden="true"
                :style="{ top: insertionTop + 'px' }"
              />
            </div>
          </section>

          <section class="panel inspector">
            <h2>Properties</h2>
            <PrototypeBlockProperties
              v-if="selectedBlock"
              :key="selectedBlock.id"
              :block="selectedBlock"
              :disabled="busy"
              @update="updateBlock"
              @remove="removeSelected"
            />
            <p v-else class="muted">Select a block to edit its properties.</p>
          </section>
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
    </template>

    <!-- Keep the drag image out of hit testing and workspace clipping. -->
    <Teleport to="body">
      <div
        v-if="activeDrag && dragBlock"
        class="drag-image"
        aria-hidden="true"
        :style="{
          left: 'clamp(8px, ' + (activeDrag.x + 12) + 'px, calc(100vw - min(16rem, 50vw) - 8px))',
          top: 'min(' + (activeDrag.y + 12) + 'px, calc(100vh - 5rem))',
        }"
      >
        <PrototypeBlockView :block="dragBlock" />
      </div>
    </Teleport>

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
/* Use the shared tool surfaces and size the editor against the workspace width. */
.builder {
  display: grid;
  gap: 1rem;
  min-width: 0;
  padding: 1rem;
  overflow-wrap: anywhere;
}

.panel,
.field,
.palette-items,
.canvas-blocks,
.saved-variants,
.preview-screen {
  display: grid;
  min-width: 0;
  gap: 0.75rem;
}

.panel {
  padding: 1rem;
  border-radius: var(--tool-card-radius);
  background: var(--tool-card-surface);
  box-shadow: var(--tool-card-shadow);
}

.editor-grid {
  display: grid;
  align-items: start;
  gap: 1rem;
  min-width: 0;
}

.palette-items {
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 8.5rem), 1fr));
}

@container tool-workspace (min-width: 32rem) {
  .editor-grid {
    grid-template-columns: minmax(10rem, 0.75fr) minmax(0, 2fr);
    align-items: start;
  }

  .palette-items {
    grid-template-columns: 1fr;
  }

  .canvas-panel {
    grid-row: span 2;
  }

  .inspector {
    grid-column: 1;
  }
}

@container tool-workspace (min-width: 65rem) {
  .editor-grid {
    grid-template-columns: minmax(10rem, 0.8fr) minmax(0, 2.4fr) minmax(13rem, 1fr);
  }

  .canvas-panel {
    grid-row: auto;
  }

  .inspector {
    grid-column: auto;
  }

  /* Keep insertion and property controls reachable while a long canvas scrolls. */
  .palette,
  .inspector {
    position: sticky;
    top: 1rem;
  }
}

/* Keep navigation and editing controls easy to target in either appearance. */
.controls,
.section-title,
.palette-item,
.block-controls,
.saved-variant {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
}

.section-title,
.palette-item,
.saved-variant {
  justify-content: space-between;
}

.panel h2,
.field label {
  font-weight: 650;
}

.field input,
.builder-button {
  min-height: 44px;
  padding-inline: 0.85rem;
  border: 1px solid var(--text-muted);
  border-radius: var(--tool-field-radius);
}

.field input {
  flex: 1 1 15rem;
  min-width: 0;
  width: 100%;
  background: var(--surface-editor);
}

.builder-button {
  width: fit-content;
  max-width: 100%;
}

.muted {
  color: var(--text-muted);
  font-size: 0.875rem;
}

/* Use an in-flow canvas and show the drop marker without moving the block rows. */
.canvas {
  position: relative;
  min-height: 10rem;
  padding: 0.75rem;
  border: 1px solid var(--text-muted);
  border-radius: 0.85rem;
  background: var(--surface-editor);
}

.canvas:focus-visible {
  outline: 2px solid var(--accent-strong);
  outline-offset: 2px;
}

.empty-canvas {
  display: grid;
  place-items: center;
  min-height: 8.5rem;
  text-align: center;
}

.canvas-block {
  display: grid;
  gap: 0.55rem;
  min-width: 0;
  padding: 0.65rem;
  border: 1px solid var(--text-muted);
  border-radius: 0.8rem;
}

.canvas-block:has([aria-pressed="true"]) {
  outline: 2px solid var(--accent-strong);
  outline-offset: -2px;
}

.drag-handle {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  min-height: 44px;
  color: var(--text-primary);
  cursor: grab;
  touch-action: none;
  user-select: none;
}

.grip,
.block-grip {
  font-size: 1.2rem;
}

.block-grip {
  justify-content: center;
  width: 2rem;
}

.source-block {
  opacity: 0.6;
}

.insertion-line {
  position: absolute;
  z-index: 1;
  inset-inline: 0.5rem;
  height: 3px;
  border-radius: 2px;
  background: var(--accent-strong);
  transform: translateY(-50%);
  pointer-events: none;
}

.drag-image {
  position: fixed;
  z-index: 10;
  width: min(16rem, 50vw);
  max-height: 9rem;
  overflow: hidden;
  padding: 0.55rem 0.75rem;
  border: 1px solid var(--accent-strong);
  border-radius: 0.65rem;
  background: var(--surface-editor);
  color: var(--text-primary);
  box-shadow: var(--tool-card-raised);
  pointer-events: none;
}

/* Match the authored content canvas to a clean presentation surface. */
.preview-screen {
  max-width: 48rem;
  width: 100%;
  min-height: 10rem;
  justify-self: center;
  align-content: start;
  padding: 1rem;
  border-radius: 0.85rem;
  background: var(--surface-editor);
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

@media (forced-colors: active) {
  .insertion-line {
    background: Highlight;
    forced-color-adjust: none;
  }
}
</style>

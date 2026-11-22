<!-- Presents the starter import, editable card list, preview, and saved variants. -->
<script setup lang="ts">
import { nextTick, onMounted, ref } from "vue";
import { onBeforeRouteLeave } from "vue-router";

import type { Variant } from "../../api/prototype-builder";
import { usePrototypeBuilder } from "../../composables/prototype-builder/use-prototype-builder";

// Scope the drag gesture and draft to this workspace.
const { source, draft, variants, busy, loading, importError, saveError, listError, dirty,
  importStarter, openSample, load, open, save, move, renameCard } = usePrototypeBuilder();
const dragged = ref("");
const destination = ref<number | null>(null);
const announcement = ref("");

onMounted(load);

// Protect pending work when switching projects or leaving the tool.
function mayReplace() {
  return !busy.value && (!dirty.value || window.confirm("Discard unsaved prototype changes?"));
}

onBeforeRouteLeave(mayReplace);

function requestImport() {
  if (mayReplace()) void importStarter();
}

function requestSample() {
  if (mayReplace()) openSample();
}

function requestOpen(variant: Variant) {
  if (mayReplace()) open(variant);
}

// Both input methods use the same index-based operation and announce the result.
function moveCard(id: string, index: number) {
  const cards = draft.value?.starter.cards;
  const card = cards?.find((entry) => entry.id === id);
  if (busy.value || !card || !cards || index < 0 || index >= cards.length || cards[index]?.id === id) return;
  move(id, index);
  announcement.value = `Moved ${card.title} to position ${index + 1} of ${cards.length}.`;
  const focused = document.activeElement;
  if (focused instanceof HTMLButtonElement) void nextTick(() => focused.focus());
}

// Pointer capture keeps card movement inside the editor on Windows WebView2.
function startDrag(event: PointerEvent, id: string) {
  if (busy.value || !event.isPrimary || event.button !== 0) return;
  const handle = event.currentTarget as HTMLElement;
  handle.setPointerCapture(event.pointerId);
  dragged.value = id;
}

function dropCard(event: PointerEvent) {
  trackDrag(event);
  if (dragged.value && destination.value !== null) moveCard(dragged.value, destination.value);
  resetDrag();
}

// Highlight the row under the pointer while the capture stays on the handle.
function trackDrag(event: PointerEvent) {
  if (!dragged.value) return;
  const target = document.elementFromPoint(event.clientX, event.clientY)?.closest("[data-card-index]");
  destination.value = target ? Number(target.getAttribute("data-card-index")) : null;
}

function resetDrag() {
  dragged.value = "";
  destination.value = null;
}
</script>

<template>
  <div class="builder">
    <!-- Keep import feedback and retry beside the source field. -->
    <section class="panel">
      <form @submit.prevent="requestImport">
        <label for="prototype-source">GitHub repository URL</label>
        <div class="controls">
          <input
            id="prototype-source"
            v-model="source"
            placeholder="https://github.com/owner/repo"
            required
            :disabled="busy"
          />
          <button type="submit" :disabled="busy">Import GitHub starter</button>
          <button
            type="button"
            :disabled="busy"
            @click="requestSample"
          >
            Open bundled sample
          </button>
        </div>
        <p>GitHub import requires a WITS starter file. The bundled sample works offline.</p>
        <p v-if="importError" class="error" role="alert">{{ importError }}</p>
      </form>
    </section>

    <!-- Editor controls and preview share the same live draft. -->
    <div v-if="draft" class="columns">
      <section class="panel">
        <h2>Edit cards</h2>
        <p class="sr-only" role="status">{{ announcement }}</p>
        <fieldset :disabled="busy">
          <ol class="cards">
            <li
              v-for="(card, index) in draft.starter.cards"
              :key="card.id"
              :data-card-index="index"
              :class="{ dragging: dragged === card.id, destination: destination === index && dragged !== card.id }"
            >
              <label :for="'prototype-card-' + index">Card {{ index + 1 }} title</label>
              <input
                :id="'prototype-card-' + index"
                :value="card.title"
                required
                @input="renameCard(card.id, ($event.target as HTMLInputElement).value)"
              />
              <div class="controls">
                <span
                  class="handle"
                  aria-hidden="true"
                  @pointerdown="startDrag($event, card.id)"
                  @pointermove="trackDrag"
                  @pointerup="dropCard"
                  @lostpointercapture="resetDrag"
                >Drag to reorder</span>
                <button
                  type="button"
                  :aria-disabled="index === 0"
                  :aria-label="'Move up: ' + card.title"
                  @click="moveCard(card.id, index - 1)"
                >Move up</button>
                <button
                  type="button"
                  :aria-disabled="index === draft.starter.cards.length - 1"
                  :aria-label="'Move down: ' + card.title"
                  @click="moveCard(card.id, index + 1)"
                >Move down</button>
              </div>
            </li>
          </ol>
          <form @submit.prevent="save()">
            <label for="prototype-name">Variant name</label>
            <div class="controls">
              <input id="prototype-name" v-model="draft.name" required @input="dirty = true" />
              <button type="submit">Save variant</button>
              <button v-if="draft.id" type="button" @click="save(true)">Save as new</button>
            </div>
            <p v-if="saveError" class="error" role="alert">{{ saveError }}</p>
          </form>
        </fieldset>
      </section>

      <section class="panel" aria-label="Live preview">
        <h2>{{ draft.starter.title }}</h2>
        <ol class="cards">
          <li v-for="card in draft.starter.cards" :key="card.id">
            <h3>{{ card.title }}</h3>
            <p>{{ card.body }}</p>
          </li>
        </ol>
      </section>
    </div>

    <!-- Saved entries are available even when GitHub cannot be reached. -->
    <section class="panel">
      <h2>Saved variants</h2>
      <p v-if="loading" role="status">Loading variants…</p>
      <div v-else-if="listError" class="controls">
        <p class="error" role="alert">{{ listError }}</p>
        <button type="button" @click="load">Retry</button>
      </div>
      <p v-else-if="!variants.length">No saved variants yet.</p>
      <ul v-else class="cards">
        <li v-for="variant in variants" :key="variant.id ?? variant.name" class="controls">
          <span>{{ variant.name }}</span>
          <button
            type="button"
            :disabled="busy"
            :aria-label="'Open ' + variant.name"
            @click="requestOpen(variant)"
          >Open</button>
        </li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
/* Use the workspace palette and adapt the two panes to narrow windows. */
.builder, .columns, .cards, .panel, fieldset {
  display: grid;
  gap: 0.75rem;
  min-width: 0;
}

.builder {
  padding: 1rem;
  overflow-wrap: anywhere;
}

.columns {
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 19rem), 1fr));
  align-items: start;
}

.panel, .cards > li {
  padding: 1rem;
  border-radius: 12px;
  background: var(--surface-toolbar);
}

.cards > li {
  background: var(--surface-editor);
}

.controls {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

label, h2, h3 {
  font-weight: 600;
}

input, button, .handle {
  min-height: 44px;
  max-width: 100%;
  padding: 0.4rem 0.85rem;
  border: 1px solid var(--accent-strong);
  border-radius: 24px;
}

input {
  flex: 1 1 12rem;
  width: 100%;
  background: var(--surface-editor);
}

button {
  background: var(--accent-strong);
  color: var(--text-on-accent);
}

:is(input, button):focus-visible {
  outline: 2px solid var(--accent-strong);
  outline-offset: 2px;
}

button:disabled, button[aria-disabled="true"], .dragging {
  opacity: 0.65;
}

.destination {
  outline: 2px solid var(--accent-strong);
  outline-offset: -2px;
}

.handle {
  display: inline-flex;
  align-items: center;
  cursor: grab;
  touch-action: none;
  user-select: none;
}

.error {
  padding: 0.5rem;
  border: 1px solid var(--status-error-indicator);
  border-radius: 8px;
  background: var(--status-error-surface);
  color: var(--status-error-text);
}
</style>

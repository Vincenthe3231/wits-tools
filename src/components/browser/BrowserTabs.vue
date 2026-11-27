<!-- Renders the browser tab strip and manages keyboard focus within it. -->
<script setup lang="ts">
import { nextTick, ref } from "vue";

import type { BrowserTab } from "../../api/browser";

const props = defineProps<{
  tabs: BrowserTab[];
  active: number | null;
  pending: boolean;
}>();

const emit = defineEmits<{
  select: [id: number];
  close: [id: number];
  create: [];
}>();

const strip = ref<HTMLElement>();

// Arrow keys move between tabs, while Delete uses the same close action as the button.
function move(event: KeyboardEvent, id: number) {
  const index = props.tabs.findIndex((tab) => tab.id === id);
  const last = props.tabs.length - 1;
  const offsets = { ArrowLeft: index === 0 ? last : index - 1, ArrowRight: index === last ? 0 : index + 1, Home: 0, End: last };
  const target = offsets[event.key as keyof typeof offsets];
  if (event.key === "Delete" && !props.pending) {
    event.preventDefault();
    emit("close", id);
  } else if (target !== undefined) {
    event.preventDefault();
    const tab = props.tabs[target];
    if (!tab) return;
    emit("select", tab.id);
    void nextTick(() => strip.value?.querySelector<HTMLElement>(`#browser-tab-${tab.id}`)?.focus());
  }
}

// Name blank tabs independently of the WebView's about:blank URL.
function label(tab: BrowserTab) {
  return tab.address === "about:blank" ? "New tab" : tab.title || tab.address || "New tab";
}
</script>

<template>
  <div class="browser-tab-strip">
    <div ref="strip" class="browser-tab-list" role="tablist" aria-label="Browser tabs">
      <div
        v-for="tab in tabs"
        :key="tab.id"
        class="browser-tab-item"
        :class="{ 'is-active': tab.id === active }"
        role="presentation"
      >
        <button
          :id="`browser-tab-${tab.id}`"
          class="browser-tab"
          type="button"
          role="tab"
          :aria-selected="tab.id === active"
          :tabindex="tab.id === active ? 0 : -1"
          aria-controls="browser-page"
          :title="label(tab)"
          @click="emit('select', tab.id)"
          @keydown="move($event, tab.id)"
        >
          <span>{{ label(tab) }}</span>
        </button>
        <button
          class="browser-tab-close"
          type="button"
          :aria-label="`Close tab, ${label(tab)}`"
          :title="`Close tab, ${label(tab)}`"
          :disabled="pending"
          @click="emit('close', tab.id)"
        >×</button>
      </div>
    </div>
    <button class="browser-button browser-new-tab" type="button" aria-label="New tab" title="New tab" :disabled="pending" @click="emit('create')">+</button>
  </div>
</template>

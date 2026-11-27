<!-- Combines shared workspace controls with the native page area below the toolbar. -->
<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, nextTick, onMounted, onScopeDispose, ref, watch } from "vue";

import { browser, type BrowserAction, type BrowserState } from "../../api/browser";
import BrowserTabs from "../../components/browser/BrowserTabs.vue";
import WorkspaceAppearance from "../../components/dashboard/WorkspaceAppearance.vue";
import WorkspaceIcon from "../../components/ui/WorkspaceIcon.vue";
import "../../styles/browser.css";
import "../../styles/dashboard/workspace.css";

const state = ref<BrowserState>({ tabs: [], active: null, pending: true, downloads: 0, notice: null });
const activeTab = computed(() => state.value.tabs.find((tab) => tab.id === state.value.active));
const displayedAddress = computed(() => activeTab.value?.address === "about:blank" ? "" : activeTab.value?.address ?? "");
const address = ref("");
const addressInput = ref<HTMLInputElement>();
const chrome = ref<HTMLElement>();
const error = ref("");
let editing = false;
let refreshRequested = false;
let refreshing = false;
let disposed = false;
let observer: ResizeObserver | undefined;
let frame = 0;
let lastTop = -1;
const listeners: UnlistenFn[] = [];

// Preserve typed text during background updates and follow the selected tab on switches.
watch(() => state.value.active, (active, previous) => {
  editing = false;
  address.value = displayedAddress.value;
  if (active !== null && previous === null) {
    void nextTick(() => addressInput.value?.focus());
  }
});
watch(displayedAddress, (value) => { if (!editing) address.value = value; });

// Coalesce navigation events so late responses cannot replace a newer snapshot.
async function refresh() {
  refreshRequested = true;
  if (refreshing) return;
  refreshing = true;
  try {
    while (refreshRequested && !disposed) {
      refreshRequested = false;
      state.value = await browser.toolbar({ kind: "read" });
    }
  } catch (cause) {
    error.value = String(cause);
  } finally {
    refreshing = false;
  }
}

// The Rust host uses physical pixels to position the active site below the Vue controls.
function syncBounds() {
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(() => {
    if (!chrome.value) return;
    const top = Math.ceil(chrome.value.getBoundingClientRect().bottom * devicePixelRatio);
    if (top === lastTop) return;
    lastTop = top;
    void browser.toolbar({ kind: "bounds", top }).catch((cause) => {
      lastTop = -1;
      error.value = String(cause);
    });
  });
}

async function run(action: BrowserAction) {
  error.value = "";
  try {
    await browser.toolbar(action);
    await refresh();
  } catch (cause) {
    error.value = String(cause);
  }
}

function navigate() {
  editing = false;
  void run({ kind: "navigate", address: address.value });
}

// Keep browser shortcuts local to its own window.
function shortcut(event: KeyboardEvent) {
  if (!event.ctrlKey || event.altKey || event.metaKey) return;
  const key = event.key.toLowerCase();
  if (key === "l") {
    event.preventDefault();
    addressInput.value?.select();
  } else if (key === "t" && !state.value.pending) {
    event.preventDefault();
    void run({ kind: "openTab" });
  } else if (key === "w" && activeTab.value && !state.value.pending) {
    event.preventDefault();
    void run({ kind: "closeTab", id: activeTab.value.id });
  }
}

onMounted(async () => {
  observer = new ResizeObserver(syncBounds);
  if (chrome.value) observer.observe(chrome.value);
  window.addEventListener("resize", syncBounds);
  window.addEventListener("keydown", shortcut);
  syncBounds();

  // Register change and focus listeners before reading initial tab state.
  for (const [event, handle] of [
    ["private-browser-changed", () => { void refresh(); }],
    ["private-browser-focus", () => addressInput.value?.focus()],
  ] as const) {
    try {
      const unlisten = await listen(event, handle);
      if (disposed) { unlisten(); return; }
      listeners.push(unlisten);
    } catch (cause) {
      error.value = String(cause);
      break;
    }
  }
  await refresh();
  addressInput.value?.focus();
});

onScopeDispose(() => {
  disposed = true;
  cancelAnimationFrame(frame);
  observer?.disconnect();
  window.removeEventListener("resize", syncBounds);
  window.removeEventListener("keydown", shortcut);
  listeners.forEach((unlisten) => unlisten());
});
</script>

<template>
  <main class="private-browser">
    <header ref="chrome" class="browser-chrome">
      <BrowserTabs
        :tabs="state.tabs"
        :active="state.active"
        :pending="state.pending"
        @select="run({ kind: 'select', id: $event })"
        @close="run({ kind: 'closeTab', id: $event })"
        @create="run({ kind: 'openTab' })"
      />

      <div class="browser-toolbar" aria-label="Browser controls">
        <div class="browser-navigation">
          <button class="browser-button browser-back" type="button" aria-label="Back" title="Back" :disabled="!activeTab?.canBack" @click="run({ kind: 'back' })">
            <WorkspaceIcon name="arrow" />
          </button>
          <button class="browser-button" type="button" aria-label="Forward" title="Forward" :disabled="!activeTab?.canForward" @click="run({ kind: 'forward' })">
            <WorkspaceIcon name="arrow" />
          </button>
          <button class="browser-button" type="button" aria-label="Reload" title="Reload" :disabled="!activeTab" @click="run({ kind: 'reload' })">
            <WorkspaceIcon name="reload" />
          </button>
        </div>
        <form class="browser-address-form" @submit.prevent="navigate">
          <label class="sr-only" for="browser-address">Web address</label>
          <input
            id="browser-address"
            ref="addressInput"
            v-model="address"
            type="text"
            inputmode="url"
            placeholder="Enter a web address"
            autocomplete="off"
            spellcheck="false"
            :disabled="!activeTab"
            @input="editing = true"
          />
        </form>
        <WorkspaceAppearance />
        <button class="browser-button browser-page-focus" type="button" aria-label="Focus web page" title="Focus web page" :disabled="!displayedAddress" @click="run({ kind: 'focusPage', backwards: false })">
          <WorkspaceIcon name="globe" />
        </button>
      </div>

      <p v-if="error" class="browser-feedback browser-error" role="alert">{{ error }}</p>
      <p v-else-if="state.notice || state.downloads" class="browser-feedback" role="status">
        {{ state.downloads ? `${state.downloads} download${state.downloads === 1 ? "" : "s"} in progress` : state.notice }}
      </p>
    </header>
    <section
      id="browser-page"
      class="browser-page"
      role="tabpanel"
      :aria-labelledby="activeTab ? `browser-tab-${activeTab.id}` : undefined"
      :aria-busy="state.pending"
    >
      <p v-if="state.pending && !activeTab" role="status">Opening private browser…</p>
    </section>
  </main>
</template>

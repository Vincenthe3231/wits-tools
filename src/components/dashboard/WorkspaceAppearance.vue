<!-- Applies and remembers the workspace appearance preference. -->
<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";

import WorkspaceIcon from "../ui/WorkspaceIcon.vue";

type Appearance = "system" | "light" | "dark";
const APPEARANCE_KEY = "wits-tools-appearance";

// Default to the operating system until a recognized preference is saved.
function readAppearance(): Appearance {
  try {
    const saved = localStorage.getItem(APPEARANCE_KEY);
    return saved === "light" || saved === "dark" ? saved : "system";
  } catch {
    return "system";
  }
}

const appearance = ref<Appearance>(readAppearance());

// Follow OS changes until the user explicitly chooses a theme.
const systemTheme = matchMedia("(prefers-color-scheme: dark)");
const systemIsDark = ref(systemTheme.matches);
const isDark = computed(() =>
  appearance.value === "system" ? systemIsDark.value : appearance.value === "dark",
);
const actionLabel = computed(() => isDark.value ? "Switch to light theme" : "Switch to dark theme");

function syncSystemTheme(event: MediaQueryListEvent) {
  systemIsDark.value = event.matches;
}

systemTheme.addEventListener("change", syncSystemTheme);
onScopeDispose(() => systemTheme.removeEventListener("change", syncSystemTheme));

// Toggle from the theme actually displayed, including the initial system theme.
function toggleAppearance() {
  appearance.value = isDark.value ? "light" : "dark";
}

// Apply the selection before writing storage so it also works for this session alone.
watch(
  appearance,
  (value) => {
    document.documentElement.dataset.appearance = value;
    try {
      localStorage.setItem(APPEARANCE_KEY, value);
    } catch {
      // The current window still keeps its selected appearance when storage is unavailable.
    }
  },
  { immediate: true },
);
</script>

<template>
  <!-- The icon and accessible name both describe the theme the button will switch to. -->
  <button
    class="workspace-theme-toggle"
    type="button"
    :aria-label="actionLabel"
    :title="actionLabel"
    @click="toggleAppearance"
  >
    <WorkspaceIcon :name="isDark ? 'sun' : 'theme'" />
  </button>
</template>

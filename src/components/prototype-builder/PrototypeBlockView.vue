<!-- Renders the same prototype block in the canvas, preview, and drag image. -->
<script setup lang="ts">
import type { CSSProperties } from "vue";
import type { ComponentDefinition, PrototypeBlock } from "../../api/prototype-builder";
import type { Style, Theme } from "../../utils/prototype-builder/appearance";
import PrototypeComponentView from "./PrototypeComponentView.vue";
import PrototypeSignInCard from "./PrototypeSignInCard.vue";

// Text interpolation keeps imported content as text throughout all views.
defineProps<{ block: PrototypeBlock; interactive?: boolean; appearance?: CSSProperties;
  styleOverride?: Style; definition?: ComponentDefinition; theme?: Theme }>();
</script>

<template>
  <!-- Render only authored content. Canvas controls belong to the page. -->
  <h2 v-if="block.kind === 'heading'" class="prototype-heading" :style="appearance">{{ block.text }}</h2>
  <p v-else-if="block.kind === 'text'" class="prototype-text" :style="appearance">{{ block.text }}</p>
  <article v-else-if="block.kind === 'card'" class="prototype-card" :style="appearance">
    <h3>{{ block.title }}</h3>
    <p>{{ block.body }}</p>
  </article>
  <PrototypeSignInCard v-else-if="block.kind === 'signIn'" :block="block" :interactive="interactive" :style="appearance" />
  <button v-else-if="block.kind === 'button'" class="prototype-button" :style="appearance" type="button" disabled>
    {{ block.label }}
  </button>
  <PrototypeComponentView
    v-else-if="definition && theme"
    :element="definition.root"
    :values="{ ...Object.fromEntries(Object.entries(definition.fields).map(([key, field]) => [key, field.default])), ...block.values }"
    :theme="theme"
    :override="styleOverride"
  />
</template>

<style scoped>
/* Keep block styling independent of editing and preview controls. */
.prototype-heading {
  font-size: 1.5rem;
  font-weight: 650;
  line-height: 1.2;
}

/* Let all content wrap when a freeform node becomes narrow. */
.prototype-heading,
.prototype-text,
.prototype-card,
.prototype-button {
  overflow-wrap: anywhere;
}

.prototype-text,
.prototype-card p {
  white-space: pre-wrap;
}

.prototype-card {
  display: grid;
  align-content: start;
  gap: 0.35rem;
  padding: 0.85rem;
  border-radius: 0.85rem;
  background: var(--surface-toolbar);
}

.prototype-card h3 {
  font-weight: 650;
}

.prototype-button:disabled {
  min-height: 44px;
  max-width: 100%;
  width: 100%;
  padding-inline: 1rem;
  border-radius: 1.5rem;
  background: var(--accent-strong);
  color: var(--text-on-accent);
  opacity: 1;
}
</style>

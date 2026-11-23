<!-- Renders the same prototype block in the canvas, preview, and drag image. -->
<script setup lang="ts">
import type { PrototypeBlock } from "../../api/prototype-builder";
import PrototypeSignInCard from "./PrototypeSignInCard.vue";

// Text interpolation keeps imported content as text throughout all views.
defineProps<{ block: PrototypeBlock; interactive?: boolean }>();
</script>

<template>
  <!-- Render only authored content. Canvas controls belong to the page. -->
  <h2 v-if="block.kind === 'heading'" class="prototype-heading">{{ block.text }}</h2>
  <p v-else-if="block.kind === 'text'" class="prototype-text">{{ block.text }}</p>
  <article v-else-if="block.kind === 'card'" class="prototype-card">
    <h3>{{ block.title }}</h3>
    <p>{{ block.body }}</p>
  </article>
  <PrototypeSignInCard v-else-if="block.kind === 'signIn'" :block="block" :interactive="interactive" />
  <button v-else class="prototype-button" type="button" disabled>
    {{ block.label }}
  </button>
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

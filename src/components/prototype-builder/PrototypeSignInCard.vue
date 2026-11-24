<!-- Renders the sign-in block and enables sample input only in Preview. -->
<script setup lang="ts">
import type { SignInBlock } from "../../api/prototype-builder";

// Canvas and drag images use the same markup without interactive controls.
defineProps<{ block: SignInBlock; interactive?: boolean }>();
</script>

<template>
  <!-- Keep sample values in the preview DOM and prevent form navigation. -->
  <form class="sign-in-card" :inert="!interactive" autocomplete="off" @submit.prevent>
    <h3>{{ block.title }}</h3>
    <label>
      <span>{{ block.emailLabel }}</span>
      <input type="email" required />
    </label>
    <label>
      <span>{{ block.passwordLabel }}</span>
      <input type="password" required />
    </label>
    <button type="submit" class="primary-button">{{ block.buttonLabel }}</button>
  </form>
</template>

<style scoped>
/* Keep the card centered and let authored labels wrap on narrow canvases. */
.sign-in-card {
  display: grid;
  align-content: start;
  gap: 1rem;
  width: 100%;
  justify-self: center;
  padding: clamp(1rem, 5%, 1.75rem);
  border-radius: 1rem;
  background: var(--surface-toolbar);
  box-shadow: var(--tool-card-raised);
  overflow-wrap: anywhere;
}

.sign-in-card h3 {
  font-size: 1.5rem;
  font-weight: 650;
  line-height: 1.2;
}

/* Match WITS tool fields and use the shared accent for the primary action. */
.sign-in-card label {
  display: grid;
  min-width: 0;
  gap: 0.375rem;
  font-weight: 550;
}

.sign-in-card :is(input, button) {
  width: 100%;
  min-width: 0;
  min-height: 44px;
  padding-inline: 1rem;
  border-radius: var(--tool-field-radius);
}

.sign-in-card input {
  border: 1px solid var(--prototype-border, var(--text-muted));
  background: var(--surface-editor);
  font-weight: 400;
}
</style>

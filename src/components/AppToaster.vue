<!-- Hosts notifications for the app window so they remain available across tool navigation. -->
<script setup lang="ts">
import { Toaster } from "vue-sonner";
import "vue-sonner/style.css";

// Place the stack below the toolbar and limit how much of the workspace it covers.
const options = { closeButtonAriaLabel: "Dismiss notification" };
</script>

<template>
  <Toaster
    class="app-toaster"
    container-aria-label="WITS Tools notifications"
    position="top-right"
    :offset="{ top: '4rem', right: '1rem' }"
    :visible-toasts="3"
    :toast-options="options"
    close-button
    close-button-position="top-right"
  />
</template>

<style>
/* Inherit semantic text for descriptions and use indicators for icons in either theme. */
.app-toaster [data-sonner-toast] [data-description] {
  color: inherit;
}

.app-toaster [data-sonner-toast] [data-icon] {
  color: var(--status-toast-indicator);
}

/* Reserve room for the dismiss control so long titles and messages can wrap beside it. */
.app-toaster [data-sonner-toast][data-styled="true"]:has([data-close-button]) {
  padding-inline-end: 2.5rem;
}

/* Keep the glyph inside the toast and preserve a 24 px hit area. */
.app-toaster [data-sonner-toast][data-styled="true"] [data-close-button] {
  top: 0.375rem;
  right: 0.375rem;
  width: 1.5rem;
  height: 1.5rem;
  min-height: 1.5rem;
  padding: 0;
  transform: none;
  border: 0;
  border-radius: 0.25rem;
  background: transparent;
  color: inherit;
}

/* Give hover and keyboard focus feedback without adding a resting button shape. */
.app-toaster [data-sonner-toast][data-styled="true"] [data-close-button]:hover {
  background: transparent;
  color: var(--status-toast-indicator);
}

.app-toaster [data-sonner-toast][data-styled="true"] [data-close-button]:focus-visible {
  outline: 2px solid var(--status-toast-indicator, var(--accent-strong));
  outline-offset: 0;
  box-shadow: none;
}
/* Keep reduced-motion notifications in place while preserving their content and controls. */
@media (prefers-reduced-motion: reduce) {
  .app-toaster [data-sonner-toast] {
    animation-duration: 0s !important;
    transition-duration: 0s !important;
  }
}
</style>

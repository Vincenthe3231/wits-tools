<!-- Offers a keyboard and click equivalent for dropping a block at any position. -->
<script setup lang="ts">
import {
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuPortal,
  DropdownMenuRoot,
  DropdownMenuTrigger,
} from "reka-ui";

// Keep positions relative to the current canvas order.
defineProps<{ position: number; count: number; label: string; disabled: boolean }>();
const emit = defineEmits<{ move: [position: number] }>();
let moved = false;

// Let the page focus the moved block after the menu closes.
function choose(position: number) {
  moved = true;
  emit("move", position);
}

function restoreFocus(event: Event) {
  if (moved) {
    event.preventDefault();
    moved = false;
  }
}
</script>

<template>
  <!-- Present all positions in a single menu with the current slot unavailable. -->
  <DropdownMenuRoot :modal="false">
    <DropdownMenuTrigger
      class="position-trigger"
      :disabled="disabled || count < 2"
      :aria-label="'Move to position: ' + label"
    >
      Move to position…
    </DropdownMenuTrigger>

    <DropdownMenuPortal>
      <DropdownMenuContent
        align="end"
        :side-offset="6"
        :collision-padding="8"
        class="app-dropdown prototype-position-menu"
        @close-auto-focus="restoreFocus"
      >
        <DropdownMenuLabel class="position-label">Move to position</DropdownMenuLabel>
        <DropdownMenuItem
          v-for="slot in count"
          :key="slot"
          :disabled="slot === position"
          class="position-option"
          @select="choose(slot - 1)"
        >
          {{ slot === position ? `Position ${slot} (current)` : `Position ${slot}` }}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenuPortal>
  </DropdownMenuRoot>
</template>

<style scoped>
/* Keep the menu readable and scrollable at either edge of the window. */
.position-trigger {
  min-height: 44px;
  border-color: var(--text-muted);
  border-radius: 1.5rem;
}

/* Reka portals the content outside this component's scoped DOM. */
:global(.prototype-position-menu) {
  z-index: 12;
  max-height: min(18rem, var(--reka-dropdown-menu-content-available-height, 18rem));
  overflow-y: auto;
  padding: 0.3rem;
  border: 1px solid var(--text-muted);
  border-radius: 0.8rem;
  background: var(--surface-toolbar);
  color: var(--text-primary);
  box-shadow: var(--tool-card-shadow);
}

.position-label {
  padding: 0.4rem 0.65rem;
  color: var(--text-muted);
  font-size: 0.85rem;
}

.position-option {
  display: flex;
  align-items: center;
  min-height: 44px;
  padding-inline: 0.65rem;
  border-radius: 0.5rem;
  outline: none;
  cursor: pointer;
}

.position-option[data-highlighted] {
  background: var(--surface-selected);
  color: var(--text-primary);
}

.position-option[data-disabled] {
  color: var(--text-muted);
  cursor: default;
}
</style>

<!-- Shared themed select with Reka keyboard handling and collision-aware placement. -->
<script setup lang="ts" generic="T extends string">
import {
  SelectContent, SelectIcon, SelectItem, SelectItemIndicator, SelectItemText,
  SelectPortal, SelectRoot, SelectTrigger, SelectValue, SelectViewport,
} from "reka-ui";

// Forward label associations and validation attributes to the interactive trigger.
defineOptions({ inheritAttrs: false });
defineProps<{ options: readonly { value: T; label: string }[]; placeholder?: string; compact?: boolean }>();
const model = defineModel<T>();
</script>

<template>
  <SelectRoot v-model="model">
    <SelectTrigger
      class="app-select-trigger"
      :class="{ 'app-select-compact': compact }"
      v-bind="$attrs"
    >
      <SelectValue v-if="!compact" :placeholder="placeholder" />
      <!-- Give Reka a concrete icon child so its default triangle is never inserted. -->
      <SelectIcon as-child>
        <span class="app-dropdown-chevron" />
      </SelectIcon>
    </SelectTrigger>

    <!-- Portal the popup so scroll containers cannot clip it. -->
    <SelectPortal>
      <SelectContent
        class="app-dropdown"
        :class="{ 'app-dropdown-presets': compact }"
        :align="compact ? 'end' : 'start'"
        position="popper"
        :side-offset="5"
      >
        <SelectViewport>
          <SelectItem
            v-for="option in options"
            :key="option.value"
            class="app-dropdown-item"
            :value="option.value"
          >
            <SelectItemText>{{ option.label }}</SelectItemText>
            <SelectItemIndicator aria-hidden="true">✓</SelectItemIndicator>
          </SelectItem>
        </SelectViewport>
      </SelectContent>
    </SelectPortal>
  </SelectRoot>
</template>

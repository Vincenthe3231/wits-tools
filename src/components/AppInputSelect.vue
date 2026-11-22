<!-- Combines an editable field with an inline Reka preset dropdown. -->
<script setup lang="ts">
import { computed } from "vue";

import AppSelect from "./AppSelect.vue";

// Keep free-form values in the input and show selection only for a matching preset.
const props = defineProps<{
  id: string;
  label: string;
  options: readonly { value: string; label: string }[];
  placeholder?: string;
  help?: string;
  type?: "text" | "password";
  autocomplete?: string;
}>();
const value = defineModel<string>({ required: true });
const selected = computed(() => props.options.find((option) => option.value === value.value)?.value);
</script>

<template>
  <div>
    <label :for="id">{{ label }}</label>

    <!-- Pair the chevron with the field without putting a button inside its label. -->
    <div class="input-select-row">
      <input
        :id="id"
        v-model="value"
        :type="type ?? 'text'"
        :placeholder="placeholder"
        :autocomplete="autocomplete"
        :aria-describedby="help ? id + '-help' : undefined"
      />
      <AppSelect
        compact
        :aria-label="label + ' presets'"
        :options="options"
        :model-value="selected"
        @update:model-value="value = $event ?? value"
      />
    </div>

    <small v-if="help" :id="id + '-help'">{{ help }}</small>
  </div>
</template>

<style scoped>
/* Keep the chevron next to the input as the surrounding form changes width. */
.input-select-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  gap: 0.4rem;
}
</style>

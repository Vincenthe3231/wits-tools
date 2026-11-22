<!-- Shares one accessible slider and exact numeric entry across settings forms. -->
<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { SliderRange, SliderRoot, SliderThumb, SliderTrack } from "reka-ui";

// Keep the form value scalar and allow a wider typed range where configured.
const props = defineProps<{
  id: string;
  label: string;
  min: number;
  max: number;
  unit?: string;
  help?: string;
  allowAboveMax?: boolean;
}>();
const value = defineModel<number>({ required: true });
const valid = defineModel<boolean>("valid", { default: true });
const draft = ref(String(value.value));
watch(value, (next) => { if (valid.value && Number(draft.value) !== next) draft.value = String(next); });
const position = computed(() => [Math.min(value.value, props.max)]);
const hint = computed(() => {
  if (!valid.value) {
    return props.allowAboveMax
      ? `Enter a whole number of at least ${props.min}.`
      : `Enter a whole number from ${props.min} to ${props.max}.`;
  }
  if (value.value > props.max) return `The slider covers up to ${props.max.toLocaleString()} ${props.unit ?? ""}.`;
  return props.help;
});

// Leave invalid drafts visible while keeping the last valid value for the slider.
function enter(event: Event) {
  const input = event.target as HTMLInputElement;
  draft.value = input.value;
  const next = input.valueAsNumber;
  const nextValid = input.validity.valid && Number.isSafeInteger(next);
  valid.value = nextValid;
  if (nextValid) value.value = next;
}

function slide(next: number[] | undefined) {
  if (next?.[0] === undefined) return;
  draft.value = String(next[0]);
  value.value = next[0];
  valid.value = true;
}
</script>

<template>
  <!-- Associate the visible label with exact entry and name the slider separately. -->
  <div class="app-slider">
    <div class="app-slider-heading">
      <label :for="id">{{ label }}</label>
      <input
        :id="id"
        :value="draft"
        type="number"
        :min="min"
        :max="allowAboveMax ? Number.MAX_SAFE_INTEGER : max"
        step="1"
        required
        :aria-invalid="!valid"
        :aria-describedby="hint ? id + '-hint' : undefined"
        @input="enter"
      />
    </div>
    <SliderRoot
      class="app-slider-root"
      :model-value="position"
      :min="min"
      :max="max"
      :step="1"
      @update:model-value="slide"
    >
      <SliderTrack class="app-slider-track">
        <SliderRange class="app-slider-range" />
      </SliderTrack>
      <SliderThumb
        class="app-slider-thumb"
        :aria-label="label + ' slider'"
        :aria-valuetext="unit ? position[0] + (unit === 'ms' ? ' milliseconds' : ' ' + unit) : undefined"
        :aria-describedby="hint ? id + '-hint' : undefined"
      />
    </SliderRoot>
    <small v-if="hint" :id="id + '-hint'" :class="{ 'app-slider-error': !valid }">{{ hint }}</small>
  </div>
</template>

<style scoped>
/* Keep the control within its grid cell and give the track a full-height hit area. */
.app-slider-heading {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(4rem, 6.5rem);
  align-items: center;
  gap: 0.5rem;
}

.app-slider-heading input {
  min-width: 0;
  text-align: end;
  font-variant-numeric: tabular-nums;
}

.app-slider-root {
  position: relative;
  display: flex;
  align-items: center;
  min-height: 44px;
  margin-inline: 0.5rem;
  touch-action: none;
  user-select: none;
  cursor: pointer;
}

.app-slider-track {
  position: relative;
  flex-grow: 1;
  height: 6px;
  border: 1px solid var(--accent-strong);
  border-radius: 999px;
  background: var(--surface-editor);
}

.app-slider-range {
  position: absolute;
  height: 100%;
  border-radius: inherit;
  background: var(--accent-strong);
}

.app-slider-thumb {
  display: block;
  width: 16px;
  height: 16px;
  border: 2px solid var(--accent-strong);
  border-radius: 50%;
  background: var(--surface-editor);
}

.app-slider-thumb:focus-visible {
  outline: 2px solid var(--accent-strong);
  outline-offset: 3px;
}

.app-slider-error {
  color: var(--status-error-text);
}
</style>

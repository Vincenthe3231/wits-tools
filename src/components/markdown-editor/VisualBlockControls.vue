<!-- Presents accessible block commands and pointer drag controls for the Visual editor. -->
<script setup lang="ts">
import { computed, nextTick, onScopeDispose, ref, shallowRef, watch } from "vue";
import type { Editor } from "@tiptap/vue-3";

import {
  blockAtPoint,
  blockDestinations,
  editBlock,
  moveAdjacent,
} from "../../utils/markdown-editor/editor-block-actions";
import {
  activeBlock,
  blockActions,
  moveBlock,
  type Block,
} from "../../utils/markdown-editor/visual-blocks";

const props = defineProps<{ editor: Editor; surface: HTMLElement; locked: boolean }>();
const emit = defineEmits<{ error: [message: string] }>();
const hover = shallowRef<Block>();
const picked = shallowRef<Block>();
const menu = ref<HTMLDialogElement>();
const handle = ref<HTMLButtonElement>();
const actionTrigger = ref<HTMLElement>();
const destination = ref<number>();
const picking = ref(false);
const destinationList = ref<HTMLElement>();
const indicator = ref<{ top: number; left: number }>();
let gesture:
  | { id: number; x: number; y: number; block: Block; document: object; dragging: boolean }
  | undefined;
let scrollFrame = 0;

// Anchor the handle to the hovered block or, for keyboard users, the current selection.
const current = computed(() => hover.value ?? activeBlock(props.editor.state.selection.$head));
const handlePosition = ref<{ left: number; top: number }>();
const destinations = computed(() =>
  picked.value ? blockDestinations(props.editor, picked.value) : [],
);

// Discard coordinates captured against a previous document after an edit or undo.
watch(
  () => props.editor.state.doc,
  () => {
    hover.value = undefined;
    picked.value = undefined;
    menu.value?.close();
    cancelDrag();
    positionHandle();
  },
);

// Recalculate the visible gutter position as the pointer or editor scrolls.
function positionHandle() {
  const dom = current.value && props.editor.view.nodeDOM(current.value.pos);
  if (!(dom instanceof HTMLElement)) {
    handlePosition.value = undefined;
    return;
  }

  const bounds = props.surface.getBoundingClientRect();
  const block = dom.getBoundingClientRect();
  handlePosition.value =
    block.bottom > bounds.top && block.top < bounds.bottom
      ? {
          top: Math.max(0, block.top - bounds.top),
          // Use the reserved gutter for every nesting depth so the handle clears list markers.
          left: 8,
        }
      : undefined;
}

function hoverBlock(event: PointerEvent) {
  if (
    !gesture &&
    !menu.value?.open &&
    !props.locked &&
    !handle.value?.contains(event.target as Node)
  ) {
    const block = blockAtPoint(props.editor, event.clientX, event.clientY);
    if (block) {
      hover.value = block;
      positionHandle();
    }
  }
}

// Freeze the target while the action dialog is open so buttons cannot target another block.
function openMenu(block?: Block, trigger?: HTMLElement) {
  if (!block || props.locked) {
    return;
  }

  picked.value = block;
  actionTrigger.value = trigger;
  picking.value = false;
  menu.value?.showModal();
}

function openForSelection(trigger?: HTMLElement) {
  openMenu(activeBlock(props.editor.state.selection.$head), trigger);
}

// Move focus into the destination list when its trigger is replaced.
async function chooseDestination() {
  picking.value = true;
  await nextTick();
  (destinationList.value?.querySelector("button") ?? destinationList.value)?.focus();
}

// Close the dialog before the transaction restores editor focus.
function perform(action: Parameters<typeof editBlock>[2] | -1 | 1, target?: number) {
  const block = picked.value;
  if (!block || props.locked) {
    return;
  }

  menu.value?.close();
  const error =
    target !== undefined
      ? moveBlock(props.editor, block, target)
      : typeof action === "number"
        ? moveAdjacent(props.editor, block, action)
        : editBlock(props.editor, block, action);
  emit("error", error ?? "");
  if (error) {
    actionTrigger.value?.focus();
  }
}

// A movement threshold separates clicking the handle from starting a drag.
function begin(event: PointerEvent) {
  if (event.button !== 0 || !current.value || props.locked) {
    return;
  }
  event.preventDefault();
  handle.value?.setPointerCapture(event.pointerId);
  gesture = {
    id: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    block: current.value,
    document: props.editor.state.doc,
    dragging: false,
  };
}

// Match the pointer to a compatible boundary, preferring the depth nearest its horizontal position.
function updateDrop(x: number, y: number) {
  destination.value = undefined;
  indicator.value = undefined;
  if (!gesture || gesture.document !== props.editor.state.doc) {
    return;
  }
  const bounds = props.surface.getBoundingClientRect();
  if (x < bounds.left || x > bounds.right || y < bounds.top || y > bounds.bottom) {
    return;
  }

  let best = Infinity;
  for (const item of blockDestinations(props.editor, gesture.block)) {
    const resolved = props.editor.state.doc.resolve(item.pos);
    const next = resolved.nodeAfter;
    const dom = props.editor.view.nodeDOM(
      next ? item.pos : item.pos - (resolved.nodeBefore?.nodeSize ?? 0),
    );
    if (!(dom instanceof HTMLElement)) {
      continue;
    }
    const box = dom.getBoundingClientRect();
    const top = next ? box.top : box.bottom;
    const score = Math.abs(top - y) + Math.abs(box.left - x) * 0.25;
    if (top >= bounds.top && top <= bounds.bottom && score < best) {
      best = score;
      destination.value = item.pos;
      indicator.value = { top: top - bounds.top, left: box.left - bounds.left };
    }
  }
}

// Continue edge scrolling while the pointer is stationary near the top or bottom.
function drag(event: PointerEvent) {
  if (!gesture || gesture.id !== event.pointerId) {
    return;
  }
  if (!gesture.dragging && Math.hypot(event.clientX - gesture.x, event.clientY - gesture.y) < 5) {
    return;
  }
  gesture.dragging = true;
  cancelAnimationFrame(scrollFrame);
  const { clientX: x, clientY: y } = event;
  function tick() {
    if (!gesture) {
      return;
    }
    const bounds = props.surface.getBoundingClientRect();
    const scroll = props.surface.querySelector(".visual-scroll");
    const within = x >= bounds.left && x <= bounds.right && y >= bounds.top && y <= bounds.bottom;
    const speed = y < bounds.top + 32 ? -12 : y > bounds.bottom - 32 ? 12 : 0;
    if (within && scroll && speed) {
      scroll.scrollTop += speed;
      scrollFrame = requestAnimationFrame(tick);
    }
    updateDrop(x, y);
  }
  tick();
}

// Commit only a current drop target. Lost capture, Escape and suspension cancel without edits.
function finish(event: PointerEvent) {
  if (!gesture || gesture.id !== event.pointerId) {
    return;
  }
  const { block, dragging, document } = gesture;
  if (dragging) {
    updateDrop(event.clientX, event.clientY);
  }
  const target = destination.value;
  cancelDrag();
  if (!dragging) {
    openMenu(block, handle.value);
  } else if (target !== undefined && document === props.editor.state.doc && !props.locked) {
    emit("error", moveBlock(props.editor, block, target) ?? "");
  }
}

function cancelDrag() {
  const pointer = gesture?.id;
  gesture = undefined;
  if (pointer !== undefined && handle.value?.hasPointerCapture(pointer)) {
    handle.value.releasePointerCapture(pointer);
  }
  cancelAnimationFrame(scrollFrame);
  destination.value = undefined;
  indicator.value = undefined;
}

// When the editor is suspended, release its transient targets and dialogs.
function dismiss() {
  cancelDrag();
  hover.value = undefined;
  handlePosition.value = undefined;
  menu.value?.close();
}

// Listen on the editor surface so no editable elements are added to the ProseMirror document.
props.surface.addEventListener("pointermove", hoverBlock);
props.surface.addEventListener("scroll", positionHandle, true);
onScopeDispose(() => {
  cancelDrag();
  props.surface.removeEventListener("pointermove", hoverBlock);
  props.surface.removeEventListener("scroll", positionHandle, true);
});
defineExpose({ openForSelection, dismiss });
</script>

<template>
  <!-- Keep the handle and drop line outside the editable document. -->
  <button
    v-if="handlePosition && !locked"
    ref="handle"
    class="block-handle"
    type="button"
    aria-label="Block actions and drag handle"
    :style="{ top: handlePosition.top + 'px', left: handlePosition.left + 'px' }"
    @pointerdown="begin"
    @pointermove="drag"
    @pointerup="finish"
    @pointercancel="cancelDrag"
    @lostpointercapture="cancelDrag"
    @keydown.esc.stop.prevent="cancelDrag"
    @click="$event.detail === 0 && openMenu(current, handle)"
  >
    <svg width="14" height="18" viewBox="0 0 14 18" fill="currentColor" aria-hidden="true">
      <circle
        v-for="point in [
          [4, 4],
          [10, 4],
          [4, 9],
          [10, 9],
          [4, 14],
          [10, 14],
        ]"
        :key="point.join(',')"
        :cx="point[0]"
        :cy="point[1]"
        r="1.4"
      />
    </svg>
  </button>
  <span
    v-if="indicator"
    class="drop-line"
    :style="{ top: indicator.top + 'px', left: indicator.left + 'px' }"
    aria-hidden="true"
  />

  <!-- Native dialog focus and Escape support also make Move To usable without dragging. -->
  <dialog ref="menu" aria-labelledby="block-actions-title">
    <div v-if="picked" class="dialog-content">
      <h2 id="block-actions-title">Block actions</h2>
      <div v-if="picking" ref="destinationList" class="block-action-list" tabindex="-1">
        <button
          v-for="item in destinations"
          :key="item.pos"
          type="button"
          @click="perform(1, item.pos)"
        >
          {{ item.label }}
        </button>
        <p v-if="!destinations.length">No compatible destinations.</p>
      </div>
      <div v-else class="block-action-list">
        <button type="button" @click="perform('above')">Add Above</button>
        <button type="button" @click="perform('below')">Add Below</button>
        <details v-if="picked.node.isTextblock">
          <summary>Turn Into</summary>
          <button
            v-for="item in blockActions"
            :key="item.label"
            type="button"
            @click="perform(item)"
          >
            {{ item.label }}
          </button>
        </details>
        <button type="button" @click="perform('duplicate')">Duplicate</button>
        <button type="button" :disabled="!picked.index" @click="perform(-1)">Move Up</button>
        <button
          type="button"
          :disabled="picked.index + 1 >= picked.parent.childCount"
          @click="perform(1)"
        >
          Move Down
        </button>
        <button type="button" @click="chooseDestination">Move To</button>
        <button type="button" @click="perform('delete')">Delete</button>
      </div>
      <button
        type="button"
        @click="
          menu?.close();
          actionTrigger?.focus();
        "
      >
        Close
      </button>
    </div>
  </dialog>
</template>

<style scoped>
/* Keep the gutter handle compact and the insertion line visually separate from text. */
.block-handle,
.drop-line {
  position: absolute;
  z-index: 3;
}

.block-handle {
  display: grid;
  width: 24px;
  min-height: 28px;
  padding: 0;
  place-items: center;
  color: var(--accent-strong);
  touch-action: none;
  cursor: grab;
}

.drop-line {
  right: 1rem;
  height: 2px;
  background: var(--accent-strong);
  pointer-events: none;
}

/* Scroll long destination lists without moving the surrounding dialog. */
.block-action-list {
  display: grid;
  max-height: 45vh;
  overflow-y: auto;
  scrollbar-width: thin;
}

.block-action-list button {
  text-align: start;
}
</style>

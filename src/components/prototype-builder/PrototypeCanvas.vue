<!-- Projects the saved screen and components into a pannable editor or fitted preview. -->
<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import type { BlockKind, CanvasNode, ComponentDefinition, NodeGeometry, PrototypeBlock, PrototypeDocument } from "../../api/prototype-builder";
import { blockWidth, coordinate, fitCamera, toDocument, type Camera, type Point } from "../../utils/prototype-builder/geometry";
import { advanceGesture, type Gesture, type GestureAction } from "../../utils/prototype-builder/engine";
import { currentTheme, styleCss, themeCss } from "../../utils/prototype-builder/appearance";
import PrototypeBlockView from "./PrototypeBlockView.vue";

const props = defineProps<{ document: PrototypeDocument; selectedId?: string | null; preview?: boolean; disabled?: boolean }>();
const emit = defineEmits<{
  select: [id: string | null];
  insert: [kind: PaletteSource, point: Point];
  move: [id: string, geometry: NodeGeometry];
  history: [redo: boolean];
}>();

// Measure only the viewport and the selected node. The camera never changes the document.
const viewport = ref<HTMLElement>();
const frame = ref<HTMLElement>();
const size = ref({ width: 0, height: 0 });
const camera = ref<Camera>({ x: 0, y: 0, zoom: 1 });
const fitted = computed(() => fitCamera({ x: 0, y: 0, ...props.document.screen }, size.value, props.preview ? 8 : 32, 1));
const view = computed(() => props.preview ? fitted.value : camera.value);
const tool = ref<"select" | "hand">("select");
const space = ref(false);
const selectedHeight = ref(0);
const selected = computed(() => props.document.nodes.find((node) => node.block.id === props.selectedId));
const transient = ref<{ id: string; geometry: NodeGeometry }>();
type PaletteSource = BlockKind | ComponentDefinition;
const ghost = ref<{ kind: PaletteSource; point: Point }>();
const theme = computed(() => props.document.theme ?? currentTheme());
const widthOf = (source: PaletteSource) => typeof source === "string" ? blockWidth(source) : source.defaultWidth;
const definitionFor = (block: PrototypeBlock) => block.kind === "imported"
  ? props.document.definitions?.find(({ id, revision }) => id === block.definitionId && revision === block.revision)
  : undefined;
const visibleSelection = computed(() => selected.value ? projectNode(selected.value) : undefined);
let viewportObserver: ResizeObserver | undefined;
let selectionObserver: ResizeObserver | undefined;
let previewObserver: IntersectionObserver | undefined;
let needsFit = true;

function projectNode(node: CanvasNode) {
  return transient.value?.id === node.block.id ? { ...node, ...transient.value.geometry } : node;
}

function nodeStyle(node: Omit<CanvasNode, "block">) {
  return {
    left: node.x + "px",
    top: node.y + "px",
    width: node.width + "px",
    minHeight: node.height === undefined ? undefined : node.height + "px",
  };
}

function fitScreen() {
  if (active) return;
  needsFit = size.value.width === 0 || size.value.height === 0;
  if (!needsFit) camera.value = fitted.value;
}

function findNode(id: string) {
  return Array.from(viewport.value?.querySelectorAll<HTMLElement>("[data-node]") ?? [])
    .find((element) => element.dataset.node === id);
}

function fitSelection() {
  if (active) return;
  const node = visibleSelection.value;
  if (!node) return;
  camera.value = fitCamera({ ...node, height: Math.max(1, selectedHeight.value) }, size.value, 32, 1);
}

function zoomBy(factor: number) {
  if (active) return;
  const current = camera.value;
  const center = { x: size.value.width / 2, y: size.value.height / 2 };
  const anchor = toDocument(center, current);
  const zoom = Math.max(Math.min(0.05, current.zoom), Math.min(8, current.zoom * factor));
  camera.value = { x: center.x - anchor.x * zoom, y: center.y - anchor.y * zoom, zoom };
}

function add(kind: PaletteSource) {
  const center = toDocument({ x: size.value.width / 2, y: size.value.height / 2 }, camera.value);
  emit("insert", kind, { x: center.x - widthOf(kind) / 2, y: center.y });
  viewport.value?.focus({ preventScroll: true });
}

// Keep clipped controls out of the tab order without changing form geometry.
function observePreview() {
  previewObserver?.disconnect();
  if (!props.preview || !frame.value) return;
  const root = frame.value;
  previewObserver = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      (entry.target as HTMLElement).tabIndex = entry.intersectionRatio > 0 ? 0 : -1;
    }
  }, { root });
  for (const control of root.querySelectorAll<HTMLElement>("input, button")) {
    control.tabIndex = -1;
    previewObserver.observe(control);
  }
}

watch(() => props.selectedId, async (id) => {
  await nextTick();
  selectionObserver?.disconnect();
  const element = id ? findNode(id) : undefined;
  selectedHeight.value = element?.offsetHeight ?? 0;
  if (element) selectionObserver?.observe(element);
}, { flush: "post" });
watch(() => props.document.nodes, observePreview, { flush: "post" });

// Capture one pointer, preview its changes locally, and commit only on release.
type Edge = "left" | "right";
type Action = Exclude<GestureAction, { kind: "pan" | "palette" }>
  | { kind: "pan" } | { kind: "palette"; blockKind: PaletteSource };
let active: { pointer: number; target: Element; gesture: Gesture; action: Action } | undefined;
const dragging = ref(false);
const clientPoint = (event: PointerEvent) => ({ x: event.clientX, y: event.clientY });

function local(point: Point) {
  const rect = viewport.value!.getBoundingClientRect();
  return { x: point.x - rect.left, y: point.y - rect.top };
}

function start(event: PointerEvent, action: Action) {
  if (active || props.disabled || props.preview || !event.isPrimary) return;
  const target = event.currentTarget as Element;
  event.preventDefault();
  viewport.value?.focus({ preventScroll: true });
  target.setPointerCapture(event.pointerId);
  active = { pointer: event.pointerId, target, gesture: { phase: "pending", start: local(clientPoint(event)), camera: { ...camera.value }, action }, action };
}

function startPalette(event: PointerEvent, blockKind: PaletteSource) {
  if (event.button === 0) start(event, { kind: "palette", blockKind });
}

function startCanvas(event: PointerEvent) {
  if (active || props.disabled || props.preview || !event.isPrimary) return;
  if (event.button === 1 || event.button === 0 && (space.value || tool.value === "hand")) {
    start(event, { kind: "pan" });
    return;
  }
  if (event.button !== 0) return;
  const target = event.target as HTMLElement;
  const handle = target.closest<HTMLElement>("[data-edge]");
  const edge = handle?.dataset.edge as Edge | undefined;
  const id = edge ? props.selectedId : target.closest<HTMLElement>("[data-node]")?.dataset.node;
  emit("select", id ?? null);
  if (!id) {
    viewport.value?.focus({ preventScroll: true });
    return;
  }
  const node = props.document.nodes.find((entry) => entry.block.id === id);
  if (!node) return;
  if (edge) {
    const cornerHeight = handle?.hasAttribute("data-corner") ? findNode(id)?.offsetHeight : undefined;
    start(event, { kind: "resize", node, edge, cornerHeight });
  } else {
    start(event, { kind: "move", node });
  }
}

function inside(point: Point) {
  return point.x >= 0 && point.y >= 0 && point.x < size.value.width && point.y < size.value.height;
}

function move(event: PointerEvent) {
  if (!active || active.pointer !== event.pointerId) return;
  if (props.disabled || props.preview) {
    clearGesture();
    return;
  }
  const point = local(clientPoint(event));
  const result = advanceGesture(active.gesture, point);
  if (!result) return;
  active.gesture.phase = "active";
  dragging.value = true;
  if (result.kind === "pan") camera.value = result.camera;
  if (result.kind === "geometry") transient.value = result;
  if (result.kind === "palette" && active.action.kind === "palette") {
    ghost.value = inside(point) ? { kind: active.action.blockKind, point: result.point } : undefined;
  }
}

function clearGesture(restore = true) {
  const gesture = active;
  active = undefined;
  if (restore && gesture?.action.kind === "pan") camera.value = gesture.gesture.camera;
  if (gesture?.target.hasPointerCapture(gesture.pointer)) gesture.target.releasePointerCapture(gesture.pointer);
  transient.value = undefined;
  ghost.value = undefined;
  dragging.value = false;
}

function end(event: PointerEvent) {
  if (!active || active.pointer !== event.pointerId) return;
  move(event);
  const change = transient.value;
  const placement = ghost.value;
  clearGesture(false);
  if (change) emit("move", change.id, change.geometry);
  if (placement) emit("insert", placement.kind, placement.point);
}

function cancel(event: PointerEvent) {
  if (active?.pointer === event.pointerId) clearGesture();
}

function onBlur() {
  space.value = false;
  clearGesture();
}

// A save or replacement can disable editing while a pointer is still captured.
watch(() => props.disabled, (disabled) => { if (disabled) onBlur(); });

// Focus-scoped keyboard controls leave project fields and form inputs to the browser.
function keydown(event: KeyboardEvent) {
  if (event.key === "Escape" && active) {
    event.preventDefault();
    clearGesture();
  }
  if (event.target !== viewport.value || props.disabled) return;
  if (event.code === "Space") {
    event.preventDefault();
    space.value = true;
  }
  if ((event.ctrlKey || event.metaKey) && ["z", "y"].includes(event.key.toLowerCase())) {
    event.preventDefault();
    emit("history", event.shiftKey || event.key.toLowerCase() === "y");
  }
  const directions: Record<string, Point> = {
    ArrowLeft: { x: -1, y: 0 }, ArrowRight: { x: 1, y: 0 },
    ArrowUp: { x: 0, y: -1 }, ArrowDown: { x: 0, y: 1 },
  };
  const direction = directions[event.key];
  if (!direction || active) return;
  if (tool.value === "hand") {
    event.preventDefault();
    camera.value = { ...camera.value, x: camera.value.x - direction.x * 40, y: camera.value.y - direction.y * 40 };
  } else if (selected.value) {
    event.preventDefault();
    const step = event.shiftKey ? 10 : 1;
    emit("move", selected.value.block.id, {
      x: coordinate(selected.value.x + direction.x * step), y: coordinate(selected.value.y + direction.y * step),
    });
  }
}

function keyup(event: KeyboardEvent) {
  if (event.code === "Space") space.value = false;
}

onMounted(() => {
  viewportObserver = new ResizeObserver(([entry]) => {
    if (!entry) return;
    size.value = { width: entry.contentRect.width, height: entry.contentRect.height };
    if (needsFit && !props.preview) fitScreen();
  });
  viewportObserver.observe(viewport.value!);
  selectionObserver = new ResizeObserver(() => {
    selectedHeight.value = props.selectedId ? findNode(props.selectedId)?.offsetHeight ?? 0 : 0;
  });
  const element = props.selectedId ? findNode(props.selectedId) : undefined;
  if (element) selectionObserver.observe(element);
  if (props.preview) {
    observePreview();
    return;
  }
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", end);
  window.addEventListener("pointercancel", cancel);
  window.addEventListener("lostpointercapture", cancel);
  window.addEventListener("keydown", keydown);
  window.addEventListener("keyup", keyup);
  window.addEventListener("blur", onBlur);
});

onBeforeUnmount(() => {
  onBlur();
  viewportObserver?.disconnect();
  selectionObserver?.disconnect();
  previewObserver?.disconnect();
  window.removeEventListener("pointermove", move);
  window.removeEventListener("pointerup", end);
  window.removeEventListener("pointercancel", cancel);
  window.removeEventListener("lostpointercapture", cancel);
  window.removeEventListener("keydown", keydown);
  window.removeEventListener("keyup", keyup);
  window.removeEventListener("blur", onBlur);
});

defineExpose({ startPalette, add, fitScreen, focus: () => viewport.value?.focus(), cancel: onBlur });
</script>

<template>
  <!-- Editor chrome stays in viewport pixels as the scene pans and zooms underneath it. -->
  <div class="canvas-shell" :class="{ 'preview-shell': preview }">
    <div v-if="!preview" class="canvas-toolbar" role="group" aria-label="Canvas tools">
      <button type="button" :aria-pressed="tool === 'select'" @click="tool = 'select'">Select</button>
      <button type="button" :aria-pressed="tool === 'hand'" @click="tool = 'hand'">Hand</button>
      <button type="button" aria-label="Zoom out" @click="zoomBy(1 / 1.25)">−</button>
      <output aria-label="Zoom">{{ Number((view.zoom * 100).toPrecision(3)) }}%</output>
      <button type="button" aria-label="Zoom in" @click="zoomBy(1.25)">+</button>
      <button type="button" @click="fitScreen">Fit screen</button>
      <button type="button" :disabled="!selected" @click="fitSelection">Fit selection</button>
    </div>
    <div
      ref="viewport"
      class="canvas-viewport"
      :class="{ 'hand-mode': !preview && (tool === 'hand' || space), dragging }"
      role="region"
      :aria-label="preview ? 'Prototype screen' : 'Freeform canvas'"
      :tabindex="preview ? undefined : 0"
      @pointerdown="startCanvas"
    >
      <!-- Both modes render the same screen and ordered blocks. Only Preview clips to the frame. -->
      <div
        ref="frame"
        class="canvas-frame"
        :class="{ clipped: preview }"
        :style="{
          ...themeCss(theme),
          transform: 'translate(' + view.x + 'px, ' + view.y + 'px) scale(' + view.zoom + ')',
          width: document.screen.width + 'px',
          height: document.screen.height + 'px',
        }"
      >
        <div
          v-for="node in document.nodes"
          :key="node.block.id"
          :data-node="node.block.id"
          class="canvas-node prototype-node-content"
          :style="nodeStyle(projectNode(node))"
        >
          <div class="node-body" :inert="!preview">
            <PrototypeBlockView
              :block="node.block"
              :interactive="preview"
              :definition="definitionFor(node.block)"
              :theme="theme"
              :appearance="styleCss(node.style, theme)"
              :style-override="node.style"
            />
          </div>
        </div>
        <div
          v-if="ghost"
          class="canvas-node canvas-ghost"
          :style="nodeStyle({ ...ghost.point, width: widthOf(ghost.kind) })"
        />
      </div>
      <div
        v-if="!preview && visibleSelection"
        class="canvas-selection"
        :style="{
          left: (view.x + visibleSelection.x * view.zoom) + 'px',
          top: (view.y + visibleSelection.y * view.zoom) + 'px',
          width: (visibleSelection.width * view.zoom) + 'px',
          height: (Math.max(1, selectedHeight) * view.zoom) + 'px',
        }"
      >
        <span v-for="edge in ['left', 'right']" :key="edge" :data-edge="edge" class="canvas-handle" aria-hidden="true" />
        <span v-for="edge in ['left', 'right']" :key="'corner-' + edge" :data-edge="edge" data-corner class="canvas-handle" aria-hidden="true" />
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Hide native scrolling and let the bounded viewport clip the surrounding workspace. */
.canvas-shell {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  min-width: 0;
  min-height: 22rem;
  height: min(72vh, 48rem);
  gap: 0.5rem;
}

.preview-shell {
  grid-template-rows: minmax(0, 1fr);
  height: 100%;
  min-height: 0;
}

.canvas-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.35rem;
}

.canvas-toolbar button {
  min-height: 2.5rem;
  padding-inline: 0.65rem;
}

.canvas-toolbar output {
  min-width: 3.5rem;
  text-align: center;
}

.canvas-viewport {
  position: relative;
  overflow: clip;
  touch-action: none;
  isolation: isolate;
  border-radius: 0.75rem;
  background: var(--surface-toolbar);
}

.preview-shell .canvas-viewport {
  touch-action: auto;
}

.hand-mode {
  cursor: grab;
}

.hand-mode.dragging {
  cursor: grabbing;
}

.canvas-frame {
  position: absolute;
  transform-origin: top left;
  background: var(--surface-editor);
  outline: 1px solid var(--text-muted);
  isolation: isolate;
}

.canvas-frame.clipped {
  overflow: clip;
}

.canvas-node {
  position: absolute;
  display: grid;
}

/* Let component surfaces fill the box while their content retains its natural height. */
.node-body {
  display: grid;
}

.canvas-ghost {
  pointer-events: none;
  height: 80px;
  border: 2px dashed var(--accent-strong);
  background: var(--surface-selected);
  opacity: 0.75;
}

/* Project the selection above every layer, leaving only its handles hit-testable. */
.canvas-selection {
  position: absolute;
  outline: 2px solid var(--accent-strong);
  pointer-events: none;
}

.canvas-handle {
  position: absolute;
  top: 50%;
  width: 20px;
  height: 24px;
  transform: translate(-50%, -50%);
  pointer-events: auto;
  cursor: ew-resize;
}

.canvas-handle[data-edge="right"] {
  left: 100%;
}

.canvas-handle[data-corner] {
  top: 100%;
  cursor: nesw-resize;
}

.canvas-handle[data-corner][data-edge="right"] {
  cursor: nwse-resize;
}

.canvas-handle::after {
  content: "";
  position: absolute;
  inset: 5px 6px;
  border: 2px solid var(--accent-strong);
  border-radius: 2px;
  background: var(--surface-editor);
}
</style>

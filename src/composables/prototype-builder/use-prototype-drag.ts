/** Tracks captured drag gestures, canvas insertion slots, and workspace edge scrolling. */
import { computed, onScopeDispose, shallowRef, type Ref } from "vue";

import type { BlockKind } from "../../api/prototype-builder";
import { findDropPosition } from "../../utils/prototype-builder/drop-position";

// Pointer coordinates stay in viewport space until the indicator is positioned on the canvas.
type Point = { x: number; y: number };
export type PrototypeDragSource =
  | { kind: "palette"; blockKind: BlockKind; label: string }
  | { kind: "block"; id: string; label: string };
type ActiveDrag = Point & { source: PrototypeDragSource };
type DropPosition = ReturnType<typeof findDropPosition>;

type Gesture = {
  source: PrototypeDragSource;
  pointerId: number;
  handle: HTMLElement;
  origin: Point;
  point: Point;
  dragging: boolean;
};

type Options = {
  canvas: Ref<HTMLElement | null>;
  scrollContainer?: Ref<HTMLElement | null>;
  onDrop: (source: PrototypeDragSource, insertionIndex: number) => void;
};

const DRAG_THRESHOLD = 5;
const SCROLL_EDGE = 32;
const MAX_SCROLL_PER_FRAME = 12;

// Use the scrollable content box so scrollbars and clipped canvas areas cannot accept a drop.
function workspaceBounds(scroller: HTMLElement | null) {
  const bounds = scroller?.getBoundingClientRect();
  const top = (bounds?.top ?? 0) + (scroller?.clientTop ?? 0);
  const left = (bounds?.left ?? 0) + (scroller?.clientLeft ?? 0);

  return {
    top: Math.max(0, top),
    right: Math.min(window.innerWidth, left + (scroller?.clientWidth ?? window.innerWidth)),
    bottom: Math.min(window.innerHeight, top + (scroller?.clientHeight ?? window.innerHeight)),
    left: Math.max(0, left),
  };
}

// Scroll toward canvas content beyond the workspace edge, with speed rising near the edge.
function scrollStep(
  point: Point,
  target: DOMRect,
  workspace: ReturnType<typeof workspaceBounds>,
) {
  const { x, y } = point;
  const { left, right, top, bottom } = workspace;
  if (x < Math.max(left, target.left) || x > Math.min(right, target.right) || y < top || y > bottom) {
    return 0;
  }

  const edge = Math.min(SCROLL_EDGE, (bottom - top) / 2);
  if (edge <= 0) {
    return 0;
  }
  if (target.top < top && y < top + edge) {
    return -Math.ceil(MAX_SCROLL_PER_FRAME * (1 - (y - top) / edge));
  }
  if (target.bottom > bottom && y > bottom - edge) {
    return Math.ceil(MAX_SCROLL_PER_FRAME * (1 - (bottom - y) / edge));
  }
  return 0;
}

export function usePrototypeDrag({ canvas, scrollContainer, onDrop }: Options) {
  // Keep transient drag state out of the document until a release confirms the destination.
  const activeDrag = shallowRef<ActiveDrag | null>(null);
  const dropPosition = shallowRef<DropPosition | null>(null);
  const insertionIndex = computed(() => dropPosition.value?.index ?? null);
  const insertionTop = computed(() => dropPosition.value?.top ?? null);
  let gesture: Gesture | undefined;
  let frame: number | undefined;

  // Derive the visible workspace using the app's scroll surface unless an explicit one was supplied.
  function getScroller(element: HTMLElement) {
    return scrollContainer?.value ?? element.closest<HTMLElement>(".workspace-content");
  }

  // Hit-test the rendered block midpoints and return a canvas-local indicator offset.
  function updateDrop(point: Point, element: HTMLElement, workspace: ReturnType<typeof workspaceBounds>) {
    const bounds = element.getBoundingClientRect();
    const { x, y } = point;
    if (
      x < Math.max(bounds.left, workspace.left) ||
      x > Math.min(bounds.right, workspace.right) ||
      y < Math.max(bounds.top, workspace.top) ||
      y > Math.min(bounds.bottom, workspace.bottom)
    ) {
      dropPosition.value = null;
      return;
    }

    const blocks = Array.from(
      element.querySelectorAll<HTMLElement>("[data-prototype-block]"),
      (block) => block.getBoundingClientRect(),
    );
    const position = findDropPosition(blocks, y, (bounds.top + bounds.bottom) / 2);
    dropPosition.value = {
      index: position.index,
      top: position.top - bounds.top - element.clientTop + element.scrollTop,
    };
  }

  // Schedule one layout read per animation frame and keep edge scrolling while the pointer is still.
  function scheduleFrame() {
    if (gesture?.dragging && frame === undefined) {
      frame = requestAnimationFrame(renderFrame);
    }
  }

  function renderFrame() {
    frame = undefined;
    const element = canvas.value;
    if (!gesture?.dragging || !element) {
      cancelDrag();
      return;
    }

    const { source, point } = gesture;
    const scroller = getScroller(element);
    const workspace = workspaceBounds(scroller);
    activeDrag.value = { source, ...point };
    if (scroller) {
      const speed = scrollStep(point, element.getBoundingClientRect(), workspace);
      if (speed) {
        const previous = scroller.scrollTop;
        scroller.scrollTop += speed;
        if (scroller.scrollTop !== previous) {
          scheduleFrame();
        }
      }
    }
    updateDrop(point, element, workspace);
  }

  // Capture the initiating pointer so palette and block handles keep tracking outside their bounds.
  function beginDrag(event: PointerEvent, source: PrototypeDragSource) {
    if (
      gesture ||
      !canvas.value ||
      !event.isPrimary ||
      event.button !== 0 ||
      !(event.currentTarget instanceof HTMLElement)
    ) {
      return;
    }

    const handle = event.currentTarget;
    const origin = { x: event.clientX, y: event.clientY };
    handle.setPointerCapture(event.pointerId);
    gesture = { source, pointerId: event.pointerId, handle, origin, point: origin, dragging: false };

    handle.addEventListener("lostpointercapture", cancelPointer);
    window.addEventListener("pointermove", trackPointer);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", cancelPointer);
    window.addEventListener("keydown", cancelOnEscape, true);
    window.addEventListener("blur", cancelDrag);
    window.addEventListener("scroll", scheduleFrame, true);
    window.addEventListener("resize", scheduleFrame);
  }

  // A click or tiny pointer movement leaves the document and drag indicators untouched.
  function trackPointer(event: PointerEvent) {
    if (!gesture || gesture.pointerId !== event.pointerId) {
      return;
    }

    const point = { x: event.clientX, y: event.clientY };
    gesture.point = point;
    gesture.dragging ||= Math.hypot(point.x - gesture.origin.x, point.y - gesture.origin.y) >= DRAG_THRESHOLD;
    if (gesture.dragging) {
      event.preventDefault();
      scheduleFrame();
    }
  }

  // Read the final position synchronously because pointerup can arrive before the next frame.
  function finishDrag(event: PointerEvent) {
    if (!gesture || gesture.pointerId !== event.pointerId) {
      return;
    }

    const { source, dragging } = gesture;
    const element = canvas.value;
    if (!dragging || !element) {
      cancelDrag();
      return;
    }
    updateDrop({ x: event.clientX, y: event.clientY }, element, workspaceBounds(getScroller(element)));
    const destination = dropPosition.value?.index;
    cancelDrag();
    if (destination !== undefined) {
      onDrop(source, destination);
    }
  }

  // Only the captured pointer can cancel its gesture.
  function cancelPointer(event: PointerEvent) {
    if (gesture?.pointerId === event.pointerId) {
      cancelDrag();
    }
  }

  // Let Escape reach other workspace controls when there is no drag gesture.
  function cancelOnEscape(event: KeyboardEvent) {
    if (gesture && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancelDrag();
    }
  }

  // Clear animation and listeners before releasing capture to avoid a second cancellation.
  function cancelDrag() {
    const current = gesture;
    if (!current) {
      return;
    }
    gesture = undefined;
    if (frame !== undefined) {
      cancelAnimationFrame(frame);
      frame = undefined;
    }
    activeDrag.value = null;
    dropPosition.value = null;

    current.handle.removeEventListener("lostpointercapture", cancelPointer);
    window.removeEventListener("pointermove", trackPointer);
    window.removeEventListener("pointerup", finishDrag);
    window.removeEventListener("pointercancel", cancelPointer);
    window.removeEventListener("keydown", cancelOnEscape, true);
    window.removeEventListener("blur", cancelDrag);
    window.removeEventListener("scroll", scheduleFrame, true);
    window.removeEventListener("resize", scheduleFrame);
    if (current.handle.hasPointerCapture(current.pointerId)) {
      current.handle.releasePointerCapture(current.pointerId);
    }
  }

  onScopeDispose(cancelDrag);
  return { beginDrag, cancelDrag, activeDrag, insertionIndex, insertionTop };
}

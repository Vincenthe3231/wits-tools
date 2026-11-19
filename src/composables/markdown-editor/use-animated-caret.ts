import { onMounted, onScopeDispose, shallowRef, type Ref } from "vue";

import {
  CARET_TRAIL_MAX_LENGTH,
  CARET_TRAIL_MIN_DISTANCE,
} from "../../const/markdown-editor/caret-motion";

interface CaretPosition {
  left: number;
  top: number;
  bottom: number;
}

export function useAnimatedCaret(
  container: Ref<HTMLElement | undefined>,
  locate: () => CaretPosition | null,
) {
  const caret = shallowRef<{ transform: string; height: string; transition?: string }>();
  const trail = shallowRef<{ id: number; transform: string; width: string }>();
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce), (forced-colors: active)");

  let frame = 0;
  let trailId = 0;
  let moved = false;
  let layoutChanged = false;
  let previous: { x: number; y: number; height: number } | undefined;
  let mountedSurface: HTMLElement | undefined;
  let observer: ResizeObserver | undefined;

  function measure() {
    frame = 0;
    const allowTrail = moved && !layoutChanged;
    const snap = layoutChanged;
    moved = false;
    layoutChanged = false;

    const bounds = container.value?.getBoundingClientRect();
    const position = reducedMotion.matches || document.hidden ? null : locate();
    if (
      !bounds ||
      !position ||
      position.bottom <= position.top ||
      position.left < bounds.left ||
      position.left > bounds.right ||
      position.top < bounds.top ||
      position.bottom > bounds.bottom
    ) {
      hide();
      return;
    }

    const x = position.left - bounds.left;
    const y = position.top - bounds.top;
    const height = position.bottom - position.top;

    if (snap) {
      trail.value = undefined;
    } else if (allowTrail && previous) {
      const dx = previous.x - x;
      const dy = previous.y + previous.height / 2 - y - height / 2;
      const distance = Math.hypot(dx, dy);

      if (distance >= CARET_TRAIL_MIN_DISTANCE) {
        trail.value = {
          id: ++trailId,
          transform: `translate(${x}px, ${y + height / 2}px) rotate(${Math.atan2(dy, dx)}rad)`,
          width: `${Math.min(distance, CARET_TRAIL_MAX_LENGTH)}px`,
        };
      }
    }

    previous = { x, y, height };
    caret.value = {
      transform: `translate(${x}px, ${y}px)`,
      height: `${height}px`,
      transition: snap ? "none" : undefined,
    };
  }

  function refresh() {
    if (!frame) {
      frame = requestAnimationFrame(measure);
    }
  }

  function refreshMovement() {
    moved = true;
    refresh();
  }

  function refreshLayout() {
    layoutChanged = true;
    refresh();
  }

  function hide() {
    caret.value = undefined;
    trail.value = undefined;
    previous = undefined;
  }

  function clearTrail(id: number) {
    if (trail.value?.id === id) {
      trail.value = undefined;
    }
  }

  onMounted(() => {
    mountedSurface = container.value;
    if (mountedSurface) {
      observer = new ResizeObserver(refreshLayout);
      observer.observe(mountedSurface);
      mountedSurface.addEventListener("scroll", refreshLayout, true);
    }
    document.addEventListener("visibilitychange", refresh);
    reducedMotion.addEventListener("change", refresh);
  });

  onScopeDispose(() => {
    cancelAnimationFrame(frame);
    observer?.disconnect();
    mountedSurface?.removeEventListener("scroll", refreshLayout, true);
    document.removeEventListener("visibilitychange", refresh);
    reducedMotion.removeEventListener("change", refresh);
  });

  return { caret, trail, refresh, refreshMovement, hide, clearTrail };
}

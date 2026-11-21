/** Tracks the editor insertion point and animates a shared caret and trailing shadow. */
import { onMounted, onScopeDispose, shallowRef, type Ref } from "vue";

import {
  CARET_HEAD_FREQUENCY,
  CARET_TAIL_FREQUENCY,
  CARET_TRAIL_MAX_LENGTH,
} from "../../const/markdown-editor/caret-motion";

interface CaretPosition {
  left: number;
  top: number;
  bottom: number;
}

interface Point {
  x: number;
  y: number;
}

interface Spring {
  position: Point;
  velocity: Point;
}

// Integrate a critically damped spring using elapsed time so frame rate does not change its feel.
function springAxis(position: number, velocity: number, target: number, frequency: number, elapsed: number) {
  const offset = position - target;
  const impulse = velocity + frequency * offset;
  const decay = Math.exp(-frequency * elapsed);

  return {
    position: target + (offset + impulse * elapsed) * decay,
    velocity: (velocity - frequency * impulse * elapsed) * decay,
  };
}

function advanceSpring(spring: Spring, target: Point, frequency: number, elapsed: number) {
  const x = springAxis(spring.position.x, spring.velocity.x, target.x, frequency, elapsed);
  const y = springAxis(spring.position.y, spring.velocity.y, target.y, frequency, elapsed);

  spring.position = { x: x.position, y: y.position };
  spring.velocity = { x: x.velocity, y: y.velocity };
}

// Start or reset both springs at the insertion point after focus or layout changes.
function at(position: Point): Spring {
  return { position, velocity: { x: 0, y: 0 } };
}

export function useAnimatedCaret(
  container: Ref<HTMLElement | undefined>,
  locate: () => CaretPosition | null,
) {
  const caret = shallowRef<{ transform: string; height: string }>();
  const trail = shallowRef<{ transform: string; opacity: number }>();
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce), (forced-colors: active)");

  let frame = 0;
  let measurePending = false;
  let snapPending = false;
  let lastTime: number | undefined;
  let target: Point | undefined;
  let height = 0;
  let head: Spring | undefined;
  let tail: Spring | undefined;
  let mountedSurface: HTMLElement | undefined;
  let observer: ResizeObserver | undefined;

  // Read layout on editor or scroll updates only. Running springs reuse the stored target.
  function measure() {
    const bounds = container.value?.getBoundingClientRect();
    const position = reducedMotion.matches || document.hidden ? null : locate();

    if (
      !bounds || !position || position.bottom <= position.top ||
      position.left < bounds.left || position.left > bounds.right ||
      position.top < bounds.top || position.bottom > bounds.bottom
    ) {
      hide();
      return;
    }

    height = position.bottom - position.top;
    target = { x: position.left - bounds.left, y: position.top - bounds.top + height / 2 };
    if (snapPending || !head) {
      head = at(target);
      tail = at(target);
      lastTime = undefined;
    }
  }

  // The head glides first. The slower tail stretches the shadow in the direction of travel.
  function animate(now: number) {
    frame = 0;
    if (measurePending) {
      measurePending = false;
      measure();
      snapPending = false;
    }
    if (!head || !tail || !target) {
      return;
    }

    const elapsed = lastTime === undefined ? 1 / 60 : Math.min((now - lastTime) / 1000, 0.05);
    lastTime = now;
    advanceSpring(head, target, CARET_HEAD_FREQUENCY, elapsed);
    advanceSpring(tail, target, CARET_TAIL_FREQUENCY, elapsed);

    const { x, y } = head.position;
    const dx = tail.position.x - x;
    const dy = tail.position.y - y;
    const distance = Math.hypot(dx, dy);
    const settled = distance < 0.3 && Math.hypot(target.x - x, target.y - y) < 0.3;

    if (settled) {
      head = at(target);
      tail = at(target);
      lastTime = undefined;
    }

    const position = head.position;
    caret.value = {
      transform: `translate(${position.x}px, ${position.y - height / 2}px)`,
      height: `${height}px`,
    };
    trail.value = settled || distance < 1 ? undefined : {
      transform: `translate(${x + 2}px, ${y - 3}px) rotate(${Math.atan2(dy, dx)}rad) scaleX(${Math.min(distance, CARET_TRAIL_MAX_LENGTH)})`,
      opacity: Math.min(0.76, distance / 12),
    };

    if (!settled) {
      frame = requestAnimationFrame(animate);
    }
  }

  // Coalesce editor updates into one measurement before the next paint.
  function refresh() {
    measurePending = true;
    if (!frame) {
      frame = requestAnimationFrame(animate);
    }
  }

  // Scrolling changes the measurement origin. Follow it directly to keep text and caret aligned.
  function refreshLayout() {
    snapPending = true;
    refresh();
  }

  // Stop pending work and give the editor its native caret back during composition or blur.
  function hide() {
    cancelAnimationFrame(frame);
    frame = 0;
    measurePending = false;
    caret.value = undefined;
    trail.value = undefined;
    head = undefined;
    tail = undefined;
    target = undefined;
    lastTime = undefined;
  }

  // Observe the editor surface and changes to the OS accessibility preferences.
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

  // Clean up observers and frames when an editor mode is removed.
  onScopeDispose(() => {
    hide();
    observer?.disconnect();
    mountedSurface?.removeEventListener("scroll", refreshLayout, true);
    document.removeEventListener("visibilitychange", refresh);
    reducedMotion.removeEventListener("change", refresh);
  });

  return { caret, trail, refresh, hide };
}

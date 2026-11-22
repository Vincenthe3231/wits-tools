/** Coordinates decorative card activation within a cached tool view. */
import { onActivated, onDeactivated, onUnmounted, type Ref } from "vue";

export function useActiveToolCard(workspace: Ref<HTMLElement | null | undefined>) {
  let active: HTMLElement | undefined;

  // Clear the DOM marker so returning to a card can replay its shimmer.
  function clear() {
    active?.removeAttribute("data-card-active");
    active = undefined;
  }

  function findCard(target: EventTarget | null) {
    const card = target instanceof Element ? target.closest<HTMLElement>('[data-slot="card"][data-card-variant="outer"]') : null;
    return card && workspace.value?.contains(card) ? card : undefined;
  }

  // Ignore keyboard-generated clicks and leave the control's normal action intact.
  function activate(event: MouseEvent) {
    if (!event.detail) return;
    const card = findCard(event.target);
    if (!card || card === active) return;
    clear();
    active = card;
    card.setAttribute("data-card-active", "");
  }

  // Resolve portaled selects through the open trigger before Reka closes the popup.
  function clearOutside(event: PointerEvent) {
    const target = event.target;
    if (!active || event.button !== 0 || findCard(target) || !(target instanceof Element)) return;
    const popup = target.closest('[role="listbox"][id]');
    const controls = active.querySelectorAll('[role="combobox"][aria-expanded="true"]');
    if (popup && Array.from(controls).some((control) => control.getAttribute("aria-controls") === popup.id)) return;
    clear();
  }

  // Attach listeners only while the cached tool is visible.
  function detach() {
    document.removeEventListener("click", activate, true);
    document.removeEventListener("pointerdown", clearOutside, true);
    clear();
  }

  onActivated(() => {
    document.addEventListener("click", activate, true);
    document.addEventListener("pointerdown", clearOutside, true);
  });
  onDeactivated(detach);
  onUnmounted(detach);

  return clear;
}

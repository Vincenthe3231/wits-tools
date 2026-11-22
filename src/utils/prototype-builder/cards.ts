/** Applies card edits without mutating the current draft. */
import type { Starter } from "../../api/prototype-builder";

// Keep card IDs stable and interpret the destination as the final index.
export function reorder(cards: Starter["cards"], id: string, destination: number) {
  const index = cards.findIndex((card) => card.id === id);
  const card = cards[index];
  if (!card || destination < 0 || destination >= cards.length || index === destination) return cards;
  const ordered = [...cards];
  ordered.splice(index, 1);
  ordered.splice(destination, 0, card);
  return ordered;
}

// Update only the selected card so unrelated content keeps its identity.
export function rename(cards: Starter["cards"], id: string, title: string) {
  return cards.map((card) => card.id === id ? { ...card, title } : card);
}

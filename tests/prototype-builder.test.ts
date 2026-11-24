/** Verifies starter conversion and the block operations used by the editor. */
import { expect, test } from "bun:test";

import type { ComponentDefinition, Starter } from "../src/api/prototype-builder";
import { advanceGesture, applyEdit, type Gesture } from "../src/utils/prototype-builder/engine";
import { currentTheme, styleCss } from "../src/utils/prototype-builder/appearance";
import pricing from "../examples/prototype-builder/pricing-card.wits-component.json";
import { createBlock, fromStarter, openVariant } from "../src/utils/prototype-builder/document";
import { emptyCanvas, fitCamera, layerNodes, resizeCorner, resizeWidth, stackCanvas, toDocument } from "../src/utils/prototype-builder/geometry";
import { usePrototypeBuilder } from "../src/composables/prototype-builder/use-prototype-builder";

// Include a legacy card ID that would collide with the generated heading ID.
const starter: Starter = {
  version: 1,
  title: "Release readiness",
  cards: [
    { id: "starter-heading", title: "Status", body: "Ready" },
    { id: "next", title: "Next steps", body: "Confirm release notes" },
  ],
};

test("opens a legacy variant with its heading, card identities and source intact", () => {
  const sourceUrl = "https://github.com/example/starter";
  const opened = openVariant({ id: "original", name: "Release", sourceUrl, starter });
  expect(opened).toMatchObject({ id: "original", name: "Release", sourceUrl });
  expect(opened.document).toEqual({
    version: 2,
    blocks: [
      { id: "starter-heading-title", kind: "heading", text: starter.title },
      ...starter.cards.map((card) => ({ ...card, kind: "card" })),
    ],
  });
  expect(fromStarter(starter)).toEqual(opened.document);
});

// A reopened document can be edited without changing the saved variant.
test("clones local blocks when opening a version 2 variant", () => {
  const document = fromStarter(starter);
  const opened = openVariant({ id: "original", name: "Release", sourceUrl: null, document });
  expect(opened.document).toEqual(document);
  if (opened.document.version !== 2) throw new Error("Expected the saved linear document");
  expect(opened.document.blocks).not.toBe(document.blocks);
  expect(opened.document.blocks[0]).not.toBe(document.blocks[0]);
});

// Preserve the framed layout at every zoom, including positions outside the screen.
test("inverts the panned camera and anchors width resizing", () => {
  expect(toDocument({ x: 0, y: 80 }, { x: 20, y: 40, zoom: 0.5 })).toEqual({ x: -40, y: 80 });
  const node = { block: createBlock("card", "card"), x: -40, y: 80, width: 200 };
  expect(resizeWidth(node, 35, "left")).toEqual({ x: -5, width: 165 });
  expect(resizeWidth(node, -400, "right")).toEqual({ x: -40, width: 1 });
  expect(resizeCorner(node, 100, { x: 35, y: 40 }, "left")).toEqual({ x: -5, width: 165, height: 140 });
  expect(resizeCorner(node, 100, { x: 35, y: -140 }, "right")).toEqual({ x: -40, width: 235, height: 1 });
  expect(fitCamera({ x: 0, y: 0, width: 1440, height: 900 }, { width: 720, height: 900 }, 0, 1))
    .toEqual({ x: 0, y: 225, zoom: 0.5 });
});

// Measured height, stable IDs, and the ordered array preserve intent on conversion.
test("migrates long rows and changes layer order without changing geometry", () => {
  const document = stackCanvas(fromStarter(starter), [40, 950, 80]);
  expect(document.nodes.map(({ y }) => y)).toEqual([32, 88, 1054]);
  expect(document.screen.height).toBe(1166);
  const moved = layerNodes(document.nodes, "starter-heading", "front");
  expect(moved.map(({ block }) => block.id)).toEqual(["starter-heading-title", "next", "starter-heading"]);
  expect(moved[2]).toBe(document.nodes[1]);
});

// Reopening a snapshot supplies the same clean checkpoint used by a successful save.
test("groups live field edits and undoes and redoes across a clean checkpoint", async () => {
  const project = usePrototypeBuilder(async (linear) => stackCanvas(linear, [40, 40, 40]));
  const node = { block: createBlock("heading", "title"), x: -8, y: 30, width: 470 };
  const document = { ...emptyCanvas(), nodes: [node] };
  expect(await project.open({ id: "saved", name: "Release", sourceUrl: null, document })).toBe(true);
  project.updateBlock({ id: "title", kind: "heading", text: "One" });
  project.updateBlock({ id: "title", kind: "heading", text: "One field edit" });
  project.finishField();
  expect(project.dirty.value).toBe(true);
  project.travel();
  expect(project.draft.value?.document.version).toBe(4);
  expect(project.draft.value?.document.nodes).toEqual(document.nodes);
  expect(project.dirty.value).toBe(false);
  project.travel(true);
  expect(project.draft.value?.document.nodes[0]?.block).toMatchObject({ text: "One field edit" });
  expect(project.dirty.value).toBe(true);
});

// A pending pointer emits no geometry. Active projections leave the document untouched.
test("uses captured zoom and applies one validated geometry command", () => {
  const node = { block: createBlock("card", "one"), x: -14, y: 10, width: 360 };
  const document = { ...emptyCanvas(), nodes: [node] };
  const gesture: Gesture = { phase: "pending", start: { x: 10, y: 20 }, camera: { x: 0, y: 0, zoom: 0.5 },
    action: { kind: "resize", node, edge: "left", cornerHeight: 150 } };
  expect(advanceGesture(gesture, { x: 12, y: 20 })).toBeUndefined();
  const projected = advanceGesture(gesture, { x: 20, y: 35 });
  expect(projected).toMatchObject({ kind: "geometry", geometry: { x: 6, width: 340, height: 180 } });
  expect(document.nodes[0]).toBe(node);
  if (projected?.kind !== "geometry") throw new Error("Expected geometry");
  const command = { ...projected, type: "geometry" as const };
  const changed = applyEdit(document, command);
  expect(changed.nodes[0]?.height).toBe(180);
  expect(applyEdit(changed, command)).toBe(changed);
  expect(applyEdit(changed, { type: "geometry", id: "one", geometry: { width: 0 } })).toBe(changed);
});

// Multiple placements share one revision and can override content and style independently.
test("pins imported definitions and applies authored theme colors", () => {
  const definition = pricing as ComponentDefinition;
  const initial = { ...emptyCanvas(), version: 4 as const, theme: currentTheme(), definitions: [] };
  const makeNode = (id: string) => ({ block: { id, kind: "imported" as const, definitionId: definition.id,
    revision: definition.revision, values: {} }, x: 0, y: 0, width: definition.defaultWidth });
  const first = applyEdit(initial, { type: "insert", node: makeNode("one"), definition });
  const second = applyEdit(first, { type: "insert", node: makeNode("two"), definition });
  const styled = applyEdit(second, { type: "style", id: "two", style: { fill: "accent", radius: 20 } });
  expect(styled.definitions).toEqual([definition]);
  expect(styled.nodes[0]?.style).toBeUndefined();
  expect(styleCss(styled.nodes[1]?.style, initial.theme)).toMatchObject({ backgroundColor: initial.theme.accent, borderRadius: "20px" });
  const removed = applyEdit(applyEdit(styled, { type: "remove", id: "one" }), { type: "remove", id: "two" });
  expect(removed.definitions).toEqual([]);
});

// A failed write leaves the edited snapshot dirty and the previous checkpoint usable.
test("tracks the successful save through a failed save and undo", async () => {
  let fail = false;
  const project = usePrototypeBuilder(async (linear) => stackCanvas(linear, []), {
    save: async () => { if (fail) throw "storageUnavailable"; return "saved"; },
    import: async () => { throw new Error("Unexpected import"); },
    list: async () => [],
  });
  project.newPrototype();
  project.dispatch({ type: "screen", size: { width: 390 } });
  expect(await project.save()).toBe(true);
  project.dispatch({ type: "screen", size: { width: 440 } });
  fail = true;
  expect(await project.save()).toBe(false);
  expect(project.draft.value?.document.screen.width).toBe(440);
  expect(project.dirty.value).toBe(true);
  project.travel();
  expect(project.draft.value?.document.screen.width).toBe(390);
  expect(project.dirty.value).toBe(false);
});

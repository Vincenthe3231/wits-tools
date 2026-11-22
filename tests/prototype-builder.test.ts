/** Verifies starter conversion and the block operations used by the editor. */
import { expect, test } from "bun:test";

import type { Starter } from "../src/api/prototype-builder";
import { createBlock, fromStarter, insert, openVariant, remove, reorder, update } from "../src/utils/prototype-builder/document";

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
  expect(opened.document.blocks).not.toBe(document.blocks);
  expect(opened.document.blocks[0]).not.toBe(document.blocks[0]);
});

// Exercise the same order operations used by pointer drops and accessible actions.
test("inserts, reorders, updates and removes blocks without mutating the input", () => {
  const heading = createBlock("heading", "title");
  const button = createBlock("button", "next");
  const initial = insert([], heading, 0);
  const blocks = insert(initial, button, 0);
  expect(initial).toEqual([heading]);
  expect(blocks).toEqual([button, heading]);
  expect(insert(blocks, button, 1)).toBe(blocks);
  expect(insert(blocks, createBlock("card", "card"), 3)).toBe(blocks);
  expect(reorder(blocks, "next", 1)).toEqual([heading, button]);
  expect(reorder(blocks, "next", -1)).toBe(blocks);
  const updated = update(blocks, { id: "next", kind: "button", label: "Details" });
  expect(updated[0]).toEqual({ id: "next", kind: "button", label: "Details" });
  expect(blocks[0]).toBe(button);
  expect(update(blocks, { id: "next", kind: "text", text: "Wrong type" })).toBe(blocks);
  expect(remove(remove(updated, "title"), "next")).toEqual([]);
});

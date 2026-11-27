/** Verifies the native Visual schema's portability and rejection paths. */
import { expect, test } from "bun:test";
import { getSchema } from "@tiptap/core";
import { TableMap } from "@tiptap/pm/tables";

import { starterTable, validateVisualDocument, visualExtensions } from "../src/utils/visual-editor/visual-document";

const schema = getSchema(visualExtensions());

// The installed table commands expect structural roles as well as valid cell content.
test("validates a native document with editable table cells, an image, and a stable Board", () => {
  const document = {
    version: 1 as const,
    content: {
      type: "doc",
      content: [
        starterTable(),
        { type: "documentImage", attrs: { src: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScL2WQAAAABJRU5ErkJggg==", alt: "Screenshot" } },
        { type: "board", attrs: { id: "release", objects: [] } },
      ],
    },
  };
  expect(() => validateVisualDocument(document, schema)).not.toThrow();
  const table = schema.nodeFromJSON(document.content).firstChild!;
  expect(TableMap.get(table).width).toBe(2);
  expect(TableMap.get(table).height).toBe(2);
});

// Imported files cannot fetch remote or script-backed image URLs.
test("rejects an image reference that cannot travel with the Visual file", () => {
  expect(() => validateVisualDocument({
    version: 1,
    content: {
      type: "doc",
      content: [{ type: "documentImage", attrs: { src: "https://example.com/image.png", alt: "Remote" } }],
    },
  }, schema)).toThrow();
});

import { describe, expect, test } from "bun:test";
import { Editor } from "@tiptap/core";
import type { Transaction } from "@tiptap/pm/state";

import {
  checkedVisualContent,
  formatVisualMarkdown,
  markdownExtensions,
} from "../src/utils/markdown-editor/markdown-format";

import { activeBlock, moveBlock } from "../src/utils/markdown-editor/visual-blocks";

function check(markdown: string) {
  const editor = new Editor({
    element: null,
    content: { type: "doc", content: [{ type: "paragraph" }] },
    extensions: markdownExtensions(),
  });
  try {
    return checkedVisualContent(editor, markdown);
  } finally {
    editor.destroy();
  }
}

describe("Visual Markdown compatibility", () => {
  test.each([
    "",
    // Exercise standalone, inline, and reference images through the round-trip guard.
    "![Screenshot](./screenshots/example.png)",
    "Before ![Screenshot](<shots/example one.png> \"Title\") after",
    "![Preview][shot]\n\n[shot]: ./preview.png",
    "# Example\n\nA **bold** and *italic* paragraph with `code`.\n",
    "[example](https://example.com) and [local](./other.md)",
    "- [ ] Draft\n- [x] Done\n",
    "> A quote\n\n2. Second\n3. Third\n",
    "```js\nconst sample = '<strong>';\n```\n",
    "\uFEFF# Windows\r\n\r\nFirst paragraph.\r\n",
  ])("opens supported source: %j", (markdown) => {
    expect(check(markdown)).toHaveProperty("content");
  });

  test.each([
    ["---\ntitle: Note\n---\n# Note", "front matter"],
    ["<!-- comment -->\n\n# Note", "raw HTML"],
    ["| A | B |\n| --- | --- |\n| a | b |", "tables"],
  ])("keeps %j in Source", (markdown, reason) => {
    expect(check(markdown)).toHaveProperty("reason", expect.stringContaining(reason));
  });

  test("preserves BOM and CRLF after a Visual edit", () => {
    expect(formatVisualMarkdown("# Changed\n\nBody", "\uFEFF# Original\r\n")).toBe(
      "\uFEFF# Changed\r\n\r\nBody",
    );
  });
});

// A block drag uses the same move transaction as keyboard block actions.
test("moves an image paragraph without losing its Markdown representation", () => {
  const editor = new Editor({
    element: null,
    extensions: markdownExtensions(),
    content: "First paragraph\n\n![Screenshot](./shot.png)",
    contentType: "markdown",
  });

  // Supply the two view methods used at commit time without mounting a browser editor.
  let applied: Transaction | undefined;
  Object.defineProperty(editor, "view", {
    value: {
      dispatch: (transaction: Transaction) => { applied = transaction; },
      focus: () => {},
    },
  });

  try {
    const imageBlock = activeBlock(editor.state.doc.resolve(editor.state.doc.firstChild!.nodeSize));
    expect(imageBlock).toBeDefined();
    expect(moveBlock(editor, imageBlock!, 0)).toBeUndefined();
    expect(editor.markdown?.serialize(applied!.doc.toJSON())).toBe(
      "![Screenshot](./shot.png)\n\nFirst paragraph",
    );
  } finally {
    editor.destroy();
  }
});

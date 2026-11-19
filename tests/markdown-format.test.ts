import { describe, expect, test } from "bun:test";
import { Editor } from "@tiptap/core";

import {
  checkedVisualContent,
  formatVisualMarkdown,
  markdownExtensions,
} from "../src/utils/markdown-editor/markdown-format";

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
    ["![Example](relative.png)", "images"],
  ])("keeps %j in Source", (markdown, reason) => {
    expect(check(markdown)).toHaveProperty("reason", expect.stringContaining(reason));
  });

  test("preserves BOM and CRLF after a Visual edit", () => {
    expect(formatVisualMarkdown("# Changed\n\nBody", "\uFEFF# Original\r\n")).toBe(
      "\uFEFF# Changed\r\n\r\nBody",
    );
  });
});

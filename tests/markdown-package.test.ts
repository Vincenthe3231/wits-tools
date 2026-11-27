/** Verifies that package links change without reformatting the Markdown around them. */
import { expect, test } from "bun:test";

import { markdownPackage } from "../src/utils/markdown-editor/markdown-package";

// Include a shared definition, a table image, and an example which should stay literal.
test("rewrites local image destinations and preserves unrelated Markdown", () => {
  const source = [
    '![A](<./screens/a b.png> "Title")',
    "![B][asset] [Open the same image][asset]",
    "",
    "[asset]: other/shot.png",
    "",
    "| Example |",
    "| --- |",
    "| ![C](screens/shot.png) |",
    "",
    "![Remote](https://example.com/image.png)",
    "",
    "```md",
    "![Example](unchanged.png)",
    "```",
  ].join("\r\n");

  const result = markdownPackage(source);
  expect(result.assets).toEqual([
    { source: "screens/shot.png", target: "image-1.png" },
    { source: "other/shot.png", target: "image-2.png" },
    { source: "./screens/a b.png", target: "image-3.png" },
  ]);
  expect(result.markdown).toBe(source
    .replace("<./screens/a b.png>", "img/image-3.png")
    .replace("other/shot.png", "img/image-2.png")
    .replace("screens/shot.png", "img/image-1.png"));
});

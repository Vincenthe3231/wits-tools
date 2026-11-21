/** Covers image clipboard payloads shared by Source and Visual mode. */
import { describe, expect, test } from "bun:test";

import { clipboardImages } from "../src/utils/markdown-editor/image-attachments";

// Model the clipboard file list and item list independently, as WebViews can expose either.
describe("image clipboard payloads", () => {
  const screenshot = new File(["image bytes"], "Screenshot.png", { type: "image/png" });
  const plainText = { kind: "string", type: "text/plain" };

  // A bitmap takes precedence over plain text bundled with the clipboard image.
  test("prefers bitmap items without duplicating files", () => {
    const clipboard = {
      items: [plainText, { kind: "file", type: "image/png", getAsFile: () => screenshot }],
      files: [screenshot],
    } as unknown as DataTransfer;

    expect(clipboardImages(clipboard)).toEqual([screenshot]);
  });

  // Text pastes stay with CodeMirror when no bitmap is available.
  test("ignores text and falls back to image files when needed", () => {
    const textClipboard = { items: [plainText], files: [] } as unknown as DataTransfer;
    const fileClipboard = { items: [], files: [screenshot] } as unknown as DataTransfer;

    expect(clipboardImages(textClipboard)).toEqual([]);
    expect(clipboardImages(fileClipboard)).toEqual([screenshot]);
  });
});

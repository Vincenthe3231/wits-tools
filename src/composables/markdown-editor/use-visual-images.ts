/** Handles image files and bitmap clipboard operations for the current Visual selection. */
import { NodeSelection, type Transaction } from "@tiptap/pm/state";
import type { Editor } from "@tiptap/vue-3";
import { ref, type ShallowRef } from "vue";

import {
  clipboardImages,
  imageMarkdown,
  stageImage,
} from "../../utils/markdown-editor/image-attachments";

import { EditorFailure } from "../../api/markdown-editor";
import { editorNotice } from "../../const/markdown-editor/errors";
import { notify } from "../use-notifications";

// Reserve one image insertion at a time and map the insertion point while the files are being staged.
export function useVisualImages(editor: ShallowRef<Editor | undefined>, locked: () => boolean) {
  const busy = ref(false);

  // Keep each imported image in a paragraph so the existing handle moves the whole image.
  async function insert(files: File[], operation: "pasteImage" | "insertImage" = "insertImage") {
    const current = editor.value;
    if (!current || locked() || busy.value || !files.length) return;

    busy.value = true;
    let bookmark = current.state.selection.getBookmark();
    const map = ({ transaction }: { transaction: Transaction }) => {
      bookmark = bookmark.map(transaction.mapping);
    };
    current.on("transaction", map);

    try {
      const blocks = [];
      for (const file of files) {
        const attrs = await stageImage(file, operation);
        blocks.push({ type: "paragraph", content: [{ type: "image", attrs }] });
      }

      if (current === editor.value && !current.isDestroyed && !locked()) {
        const { from, to } = bookmark.resolve(current.state.doc);
        if (!current.chain().focus().insertContentAt({ from, to }, blocks).run()) {
          throw new Error("The image could not be inserted here.");
        }
      }
    } catch (error) {
      notify(editorNotice(error, operation));
    } finally {
      current.off("transaction", map);
      busy.value = false;
    }
  }

  // Route image files through native storage. Let ProseMirror handle text and rich paste normally.
  function paste(event: ClipboardEvent): boolean {
    const files = clipboardImages(event.clipboardData);
    if (!files.length) return false;

    event.preventDefault();
    void insert(files, "pasteImage");
    return true;
  }

  // Copy a selected image as PNG for other applications, plus Markdown for plain-text targets.
  function copy(event: ClipboardEvent): boolean {
    const current = editor.value;
    const selection = current?.state.selection;
    if (!current || !(selection instanceof NodeSelection) || selection.node.type.name !== "image") {
      return false;
    }

    const markdown = imageMarkdown(selection.node.toJSON());
    // Preserve the image node in rich clipboard consumers if bitmap access is unavailable.
    const element = document.createElement("img");
    element.setAttribute("src", selection.node.attrs.src);
    element.alt = selection.node.attrs.alt;
    if (selection.node.attrs.title) element.title = selection.node.attrs.title;
    event.clipboardData?.setData("text/html", element.outerHTML);
    event.clipboardData?.setData("text/plain", markdown);
    event.preventDefault();
    const dom = current.view.nodeDOM(selection.from);
    if (
      dom instanceof HTMLImageElement &&
      dom.complete &&
      dom.naturalWidth &&
      navigator.clipboard?.write &&
      typeof ClipboardItem !== "undefined"
    ) {
      const canvas = document.createElement("canvas");
      canvas.width = dom.naturalWidth;
      canvas.height = dom.naturalHeight;
      const context = canvas.getContext("2d");
      if (!context) return true;
      context.drawImage(dom, 0, 0);
      const bitmap = new Promise<Blob>((resolve, reject) => {
        canvas.toBlob((blob) => blob ? resolve(blob) : reject(new Error("Could not encode the image.")));
      });
      void navigator.clipboard.write([new ClipboardItem({
        "image/png": bitmap,
        "text/plain": new Blob([markdown], { type: "text/plain" }),
      })]).catch(() => {
        notify(editorNotice(new EditorFailure("clipboardWriteFailed", "copyImage"), "copyImage"));
      });
    }
    return true;
  }

  return { busy, insert, paste, copy };
}

/** Defines the Markdown image node and its local attachment preview. */
import { Node } from "@tiptap/core";

import { EditorFailure, imageTransfer } from "../../api/markdown-editor";
import { editorNotice } from "../../const/markdown-editor/errors";
import { imageMarkdown } from "./image-attachments";

// Keep the original URL in the document and derive a separate, temporary preview URL.
export const VisualImage = Node.create({
  name: "image",
  inline: true,
  group: "inline",
  atom: true,

  addAttributes() {
    return {
      src: { default: "" },
      alt: { default: "" },
      title: { default: null },
    };
  },

  // Preserve image references in rich clipboard content.
  parseHTML: () => [{ tag: "img[src]" }],
  renderHTML: ({ HTMLAttributes }) => ["img", HTMLAttributes],

  // Match the native Markdown token so images also work inside existing text blocks.
  parseMarkdown: (token, helpers) =>
    helpers.createNode("image", { src: token.href, alt: token.text, title: token.title ?? null }),
  renderMarkdown: imageMarkdown,

  // Keep filesystem paths out of the WebView URL and revoke previews when nodes are removed.
  addNodeView() {
    return ({ node }) => {
      const dom = document.createElement("img");
      const { src, alt, title } = node.attrs;
      dom.alt = alt;
      dom.draggable = false;
      dom.decoding = "async";
      if (title) dom.title = title;

      let preview: string | undefined;
      let destroyed = false;

      // Render only document-local files. Remote references retain their Markdown without fetching.
      if (!/^(?:[a-z][a-z\d+.-]*:|[\\/])/i.test(src)) {
        void Promise.resolve()
          .then(() => imageTransfer.read(decodeURIComponent(src)))
          .then((buffer) => {
            if (!destroyed) {
              preview = URL.createObjectURL(new Blob([buffer]));
              dom.src = preview;
            }
          })
          .catch(() => {
            // Keep missing-image feedback on the affected node without creating a toast per image.
            if (!destroyed) {
              const failure = new EditorFailure("imagePreviewUnavailable", "previewImage");
              dom.title = editorNotice(failure, "previewImage").description;
            }
          });
      } else {
        dom.title = "External image reference";
      }

      return {
        dom,
        destroy() {
          destroyed = true;
          if (preview) URL.revokeObjectURL(preview);
        },
      };
    };
  },
});

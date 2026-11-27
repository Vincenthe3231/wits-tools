/** Defines the self-contained Visual schema and validates content before adoption. */
import { Extension, Node, type JSONContent } from "@tiptap/core";
import TaskItem from "@tiptap/extension-task-item";
import TaskList from "@tiptap/extension-task-list";
import { type Schema } from "@tiptap/pm/model";
import { goToNextCell, tableEditing, tableNodes } from "@tiptap/pm/tables";
import StarterKit from "@tiptap/starter-kit";

import { EditorFailure } from "../../api/markdown-editor";
import type { VisualDocument } from "../../api/visual-editor";

const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
const MAX_IMAGE_BYTES = 16 * 1024 * 1024;
const IMAGE_SOURCE = /^data:image\/(?:png|jpeg|gif|webp);base64,[A-Za-z\d+/]+={0,2}$/;

// Reject mismatched image signatures before opening data supplied by a document or paste.
function validImageSource(value: unknown): value is string {
  if (typeof value !== "string" || value.length > MAX_IMAGE_BYTES * 1.34 + 64 || !IMAGE_SOURCE.test(value)) {
    return false;
  }
  const comma = value.indexOf(",");
  const encoded = value.slice(comma + 1);
  if (encoded.length < 16 || encoded.length % 4 !== 0) return false;
  const signature = atob(encoded.slice(0, 16));
  if (value.startsWith("data:image/png")) return signature.startsWith("\x89PNG\r\n\x1a\n");
  if (value.startsWith("data:image/jpeg")) return signature.startsWith("\xff\xd8\xff");
  if (value.startsWith("data:image/gif")) return signature.startsWith("GIF87a") || signature.startsWith("GIF89a");
  return signature.startsWith("RIFF") && signature.slice(8) === "WEBP";
}

// Let the installed table primitives own cell selections and structural commands.
const specs = tableNodes({ tableGroup: "block", cellContent: "paragraph+", cellAttributes: {} });
const tableExtensions = Object.entries(specs).map(([name, spec]) => Node.create({
  name,
  content: spec.content,
  group: spec.group,
  isolating: spec.isolating,
  addAttributes: () => spec.attrs ?? {},
  parseHTML: () => spec.parseDOM ?? [],
  renderHTML: ({ node }) => spec.toDOM!(node),
}));
const TableEditing = Extension.create({
  name: "visualTables",
  extendNodeSchema: (extension) => {
    const spec = specs[extension.name as keyof typeof specs];
    return spec ? { tableRole: spec.tableRole } : {};
  },
  addProseMirrorPlugins: () => [tableEditing()],
  addKeyboardShortcuts() {
    const move = (direction: -1 | 1) => goToNextCell(direction)(this.editor.state, this.editor.view.dispatch);
    return { Tab: () => move(1), "Shift-Tab": () => move(-1) };
  },
});

// Store supported raster bytes directly in the document for portable Save As and recovery.
const DocumentImage = Node.create({
  name: "documentImage",
  group: "block",
  atom: true,
  draggable: true,
  addAttributes: () => ({ src: { default: "" }, alt: { default: "" } }),
  parseHTML: () => [{
    tag: "img[src]",
    getAttrs: (element) => validImageSource(element.getAttribute("src"))
      ? { src: element.getAttribute("src"), alt: element.getAttribute("alt") ?? "" }
      : false,
  }],
  renderHTML: ({ node }) => ["img", {
    src: IMAGE_SOURCE.test(node.attrs.src) ? node.attrs.src : "",
    alt: node.attrs.alt,
  }],
});

// Reserve a stable Board node so the next phase can add canvas editing without a file migration.
const BoardBlock = Node.create({
  name: "board",
  group: "block",
  atom: true,
  addAttributes: () => ({ id: { default: null }, objects: { default: [] } }),
  renderHTML: () => ["div", { "data-type": "board" }, "Board"],
});

export const visualExtensions = () => [
  StarterKit.configure({ link: { openOnClick: false }, trailingNode: false }),
  TaskList,
  TaskItem.configure({ nested: true }),
  ...tableExtensions,
  TableEditing,
  DocumentImage,
  BoardBlock,
];

// Check the entire structure before replacing an existing editor session.
export function validateVisualDocument(document: VisualDocument, schema: Schema) {
  if (document.version !== 1 || document.content?.type !== "doc") {
    throw new EditorFailure("invalidEncoding", "open");
  }
  try {
    const root = schema.nodeFromJSON(document.content);
    root.check();
    const boardIds = new Set<string>();
    root.descendants((node) => {
      if (node.type.name === "documentImage" &&
          (!validImageSource(node.attrs.src) || typeof node.attrs.alt !== "string")) {
        throw new Error("Invalid image");
      }
      if (node.type.name === "board") {
        const { id, objects } = node.attrs;
        if (typeof id !== "string" || !id || boardIds.has(id) || !Array.isArray(objects)) {
          throw new Error("Invalid Board");
        }
        boardIds.add(id);
      }
    });
  } catch (cause) {
    throw new EditorFailure("invalidEncoding", "open", cause);
  }
}

export const emptyVisualContent = (): JSONContent => ({ type: "doc", content: [{ type: "paragraph" }] });

// Seed a simple table with a header and one editable data row.
export function starterTable(): JSONContent {
  return {
    type: "table",
    content: ["table_header", "table_cell"].map((type) => ({
      type: "table_row",
      content: Array.from({ length: 2 }, () => ({ type, content: [{ type: "paragraph" }] })),
    })),
  };
}

// Decode the selected raster before inserting it so failed pastes leave the document intact.
export async function embeddedImage(file: File, operation: "pasteImage" | "insertImage"): Promise<JSONContent> {
  if (file.size > MAX_IMAGE_BYTES) throw new EditorFailure("imageTooLarge", operation);
  if (!IMAGE_TYPES.includes(file.type)) throw new EditorFailure("unsupportedImageType", operation);
  try {
    const bitmap = await createImageBitmap(file);
    bitmap.close();
    const src = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
    if (!validImageSource(src)) throw new Error("Image type does not match its contents");
    return { type: "documentImage", attrs: { src, alt: file.name.replace(/\.[^.]+$/, "") || "Pasted image" } };
  } catch (cause) {
    throw new EditorFailure("imageStageFailed", operation, cause);
  }
}

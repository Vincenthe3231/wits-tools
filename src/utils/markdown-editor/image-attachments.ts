/** Shares image clipboard detection, native staging, and Markdown formatting between editor modes. */
import { imageTransfer } from "../../api/markdown-editor";

// Avoid reading oversized clipboard images into IPC memory.
const MAX_IMAGE_BYTES = 16 * 1024 * 1024;

// Prefer image file items when the WebView exposes them and fall back to its file list.
export function clipboardImages(clipboard: DataTransfer | null): File[] {
  const images = Array.from(clipboard?.items ?? [])
    .filter((item) => item.kind === "file" && item.type.startsWith("image/"))
    .map((item) => item.getAsFile())
    .filter((file): file is File => file !== null);
  return images.length
    ? images
    : Array.from(clipboard?.files ?? []).filter((file) => file.type.startsWith("image/"));
}

// Stage bytes before returning a path so either mode can preview and save the same attachment.
export async function stageImage(file: File) {
  if (file.size > MAX_IMAGE_BYTES) throw new Error("Images can be up to 16 MiB each.");
  const src = await imageTransfer.stage(await file.arrayBuffer());
  const alt = file.name.replace(/\.[^.]+$/, "") || "Pasted image";
  return { src, alt };
}

// Native failures arrive as structured IPC errors while browser failures use Error objects.
export function imageImportMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error && "message" in error) return String(error.message);
  return "The image could not be inserted.";
}

// Escape Markdown punctuation while preserving the decoded destination and optional title.
export function imageMarkdown(
  node: { attrs?: { src?: string; alt?: string; title?: string | null } },
): string {
  const { src = "", alt = "", title } = node.attrs ?? {};
  const escape = (value: string) => value.replace(/[\\\[\]]/g, "\\$&");
  const target = /[\s()]/.test(src) ? `<${src}>` : src;
  const label = title === null || title === undefined ? "" : ` "${title.replace(/[\\"]/g, "\\$&")}"`;
  return `![${escape(alt)}](${target}${label})`;
}

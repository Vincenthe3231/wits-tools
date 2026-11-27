/** Maps Markdown editor failures to messages and severity for the owning view. */
import { EditorFailure, type EditorErrorCode, type EditorOperation } from "../../api/markdown-editor";

// Set the title from the action so identical I/O errors make sense in different workflows.
const titles: Record<EditorOperation, string> = {
  open: "Couldn’t open the document",
  save: "Couldn’t save the document",
  saveAs: "Couldn’t save a copy",
  reset: "Couldn’t reset the editor session",
  recoveryRead: "Couldn’t read the recovery draft",
  recoveryWrite: "Couldn’t update draft recovery",
  pasteImage: "Couldn’t paste the image",
  insertImage: "Couldn’t insert the image",
  previewImage: "Couldn’t preview the image",
  copyImage: "Couldn’t copy the image as a bitmap",
  visual: "Formatted mode couldn’t convert this document",
  exportPackage: "Couldn’t export the Markdown package",
};

// Keep diagnostic paths and raw exception text out of the visible messages.
const descriptions: Partial<Record<EditorErrorCode, string>> = {
  invalidPath: "Choose another file.",
  invalidDestination: "Choose a valid destination.",
  fileNotFound: "The file may have moved or been deleted.",
  permissionDenied: "Check file permissions or choose another location.",
  writeFailed: "Check available disk space and write access.",
  unsupportedFileType: "Choose a regular .md or .markdown file.",
  fileTooLarge: "Markdown files can be up to 8 MiB.",
  invalidEncoding: "Choose a UTF-8 Markdown file.",
  externalChange: "Your draft is still open. Use Save As to keep your edits.",
  exportConflict: "A package with this name already exists. Choose another destination.",
  unsupportedImageType: "Choose a PNG, JPEG, GIF, or WebP image.",
  imageTooLarge: "Images can be up to 16 MiB each.",
  imagePreviewUnavailable: "Check that the linked image is available in the document folder.",
  imageChanged: "An attached image changed on disk. Reinsert it before saving.",
  imageConflict: "A different image already exists at the destination. Choose another location.",
  clipboardWriteFailed: "The Markdown reference was copied.",
  visualRoundTripFailed: "The Markdown could not be converted safely.",
};

// Combine actionable details with a brief statement about the retained work.
export function editorNotice(cause: unknown, fallback: EditorOperation) {
  const { code, operation } = cause instanceof EditorFailure
    ? cause
    : new EditorFailure("unexpected", fallback);
  const severity = code === "externalChange" || code === "visualUnsupportedContent"
    || operation === "recoveryWrite" || code === "clipboardWriteFailed"
    ? "warning"
    : "error";
  const title = code === "externalChange" ? "The file changed outside WITS Tools" : titles[operation];
  const detail = operation === "exportPackage" && code === "imagePreviewUnavailable"
    ? "Check the local image paths. Save an untitled document first if it uses relative paths."
    : descriptions[code];
  const retained = operation === "pasteImage" || operation === "insertImage"
    ? "Your Markdown is unchanged."
    : operation === "save" || operation === "saveAs" || operation === "recoveryWrite"
      ? "Your edits are still open."
      : "";
  const description = code === "externalChange"
    ? detail!
    : [detail, retained].filter(Boolean).join(" ") || "Try the action again.";
  return { code, operation, severity, title, description } as const;
}

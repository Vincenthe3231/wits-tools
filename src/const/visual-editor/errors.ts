/** Names Visual file failures without exposing native diagnostics. */
import type { EditorOperation } from "../../api/markdown-editor";
import { editorNotice } from "../markdown-editor/errors";

// Reuse the shared action and severity mapping, with format-specific recovery guidance.
export function visualNotice(cause: unknown, operation: EditorOperation) {
  const notice = editorNotice(cause, operation);
  const descriptions = {
    unsupportedFileType: "Choose a regular .witsdoc file.",
    invalidEncoding: "This Visual file is damaged or uses an unsupported version.",
    fileTooLarge: "Visual files and recovery drafts can be up to 64 MiB.",
  } as const;
  const description = descriptions[notice.code as keyof typeof descriptions] ?? notice.description;
  return { ...notice, description };
}

/** Checks that IPC failures produce actionable, safe editor messages. */
import { describe, expect, spyOn, test } from "bun:test";
import { effectScope } from "vue";
import { toast } from "vue-sonner";

import { EditorFailure, markdownEditor } from "../src/api/markdown-editor";
import { useDocumentSession } from "../src/composables/markdown-editor/use-document-session";
import { editorNotice } from "../src/const/markdown-editor/errors";

// Exercise the notice paths that must preserve the current document or its recovery action.
describe("Markdown editor notices", () => {
  test("keeps Save As available after an external change", () => {
    const notice = editorNotice(new EditorFailure("externalChange", "save"), "reset");
    expect(notice).toMatchObject({ code: "externalChange", severity: "warning" });
    expect(notice.description).toContain("Save As");
  });

  test("explains a paste failure without exposing the native cause", () => {
    const cause = new Error("Internal path or diagnostic");
    const notice = editorNotice(new EditorFailure("imageStageFailed", "pasteImage", cause), "visual");
    expect(notice).toMatchObject({
      title: "Couldn’t paste the image",
      description: "Your Markdown is unchanged.",
    });
    expect(JSON.stringify(notice)).not.toContain(cause.message);
  });

  test("uses the current action for unexpected failures", () => {
    const notice = editorNotice("unvalidated native exception", "open");
    expect(notice).toMatchObject({ code: "unexpected", title: "Couldn’t open the document" });
  });
});

// Drive repeated failures through the session and inspect the actual shared Sonner queue.
test("announces a recovery outage once until a snapshot succeeds", async () => {
  const environment = globalThis as typeof globalThis & { isTauri?: boolean };
  const original = environment.isTauri;
  environment.isTauri = true;
  const scope = effectScope();
  const update = spyOn(markdownEditor, "updateRecovery");
  const session = scope.run(() => useDocumentSession(() => ""))!;
  const count = () =>
    toast.getHistory().filter((item) =>
      "title" in item && item.title === "Couldn’t update draft recovery",
    ).length;

  try {
    const before = count();
    session.editSource("Unsaved");
    update.mockRejectedValue(new EditorFailure("permissionDenied", "recoveryWrite"));
    await session.queueRecovery();
    await session.queueRecovery();
    expect(count()).toBe(before + 1);
    expect(session.buffer.value).toBe("Unsaved");

    update.mockResolvedValue(undefined);
    await session.queueRecovery();
    expect(session.recoveryError.value).toBe("");

    update.mockRejectedValue(new EditorFailure("writeFailed", "recoveryWrite"));
    await session.queueRecovery();
    expect(count()).toBe(before + 2);
  } finally {
    update.mockRestore();
    scope.stop();
    if (original === undefined) delete environment.isTauri;
    else environment.isTauri = original;
  }
});

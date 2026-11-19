import { Editor, type JSONContent } from "@tiptap/core";
import TaskItem from "@tiptap/extension-task-item";
import TaskList from "@tiptap/extension-task-list";
import { Markdown } from "@tiptap/markdown";
import StarterKit from "@tiptap/starter-kit";
import { Marked } from "marked";

const lexer = new Marked({ gfm: true });

const sourceOnly = new Map([
  ["html", "raw HTML"],
  ["table", "tables"],
  ["image", "images"],
  ["del", "strikethrough"],
  ["hr", "thematic breaks"],
]);

export function markdownExtensions() {
  return [
    StarterKit.configure({
      horizontalRule: false,
      strike: false,
      underline: false,
      trailingNode: false,
      link: { openOnClick: false, autolink: false, linkOnPaste: false },
    }),
    TaskList,
    TaskItem.configure({ nested: true }),
    Markdown,
  ];
}

type VisualContent = { content: JSONContent } | { reason: string };

export function checkedVisualContent(editor: Editor, markdown: string): VisualContent {
  const source = markdown.replace(/^\uFEFF/, "");
  if (/^(---|\+\+\+)[ \t]*\r?\n/.test(source)) {
    return { reason: "This document contains front matter. Edit it in Source mode." };
  }

  try {
    let unsupported: string | undefined;
    lexer.walkTokens(lexer.lexer(source), (token) => {
      unsupported ??= sourceOnly.get(token.type);
    });
    if (unsupported) {
      return { reason: `This document contains ${unsupported}. Edit it in Source mode.` };
    }

    const manager = editor.markdown;
    if (!manager) {
      return { reason: "The Visual editor could not load." };
    }

    const content = manager.parse(source);
    const doc = editor.schema.nodeFromJSON(content);
    if (doc.childCount > 0) {
      doc.check();
    }

    const serialized = manager.serialize(content);
    // Compare independently rendered Markdown, including link targets and task states.
    // These HTML strings are only compared, never inserted into the page.
    if (lexer.parse(source) === lexer.parse(serialized)) {
      return { content };
    }
  } catch {
    return { reason: "This Markdown could not be converted safely. Edit it in Source mode." };
  }

  return {
    reason:
      "Visual mode would change this document’s content or structure. Edit it in Source mode.",
  };
}

export function formatVisualMarkdown(markdown: string, original: string): string {
  const text = original.includes("\r\n") ? markdown.replace(/\r?\n/g, "\r\n") : markdown;
  return original.startsWith("\uFEFF") ? "\uFEFF" + text : text;
}

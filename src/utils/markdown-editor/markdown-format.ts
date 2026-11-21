/** Configures the Visual Markdown schema and checks whether files can be edited safely. */
import { Editor, type JSONContent } from "@tiptap/core";
import TaskItem from "@tiptap/extension-task-item";
import TaskList from "@tiptap/extension-task-list";
import { Markdown } from "@tiptap/markdown";
import StarterKit from "@tiptap/starter-kit";
import { Marked } from "marked";

import { VisualImage } from "./visual-image";

const lexer = new Marked({ gfm: true });

const sourceOnly = new Map([
  ["html", "raw HTML"],
  ["table", "tables"],
  ["del", "strikethrough"],
  ["hr", "thematic breaks"],
]);

// Give empty blocks explicit Markdown so inserting one is safe before the user types.
const VisualStarterKit = StarterKit.extend({
  addExtensions() {
    return (this.parent?.() ?? []).map((extension) => {
      if (extension.name === "paragraph") {
        return extension.extend({
          // The default paragraph parser unwraps lone images for block image schemas.
          parseMarkdown(token, helpers) {
            const tokens = token.tokens ?? [];
            if (tokens.length === 1 && tokens[0].type === "image") {
              return helpers.createNode("paragraph", undefined, helpers.parseInline(tokens));
            }
            return this.parent?.(token, helpers) ?? null;
          },
          renderMarkdown(node, helpers, context) {
            // An empty paragraph moved to the start of the document needs an explicit marker.
            return !node.content?.length && context.parentType === "doc"
              ? "&nbsp;"
              : (this.parent?.(node, helpers, context) ?? "");
          },
        });
      }
      if (extension.name === "heading") {
        return extension.extend({
          renderMarkdown(node, helpers, context) {
            return node.content?.length
              ? (this.parent?.(node, helpers, context) ?? "")
              : "#".repeat(Number(node.attrs?.level ?? 1));
          },
        });
      }
      if (extension.name === "blockquote") {
        return extension.extend({
          renderMarkdown(node, helpers, context) {
            const empty =
              node.content?.length === 1 &&
              !node.content[0].content?.length &&
              node.content[0].type === "paragraph";
            return empty ? "> &nbsp;" : (this.parent?.(node, helpers, context) ?? "");
          },
        });
      }
      if (extension.name === "listItem") {
        return extension.extend({
          renderMarkdown(node, helpers, context) {
            const output = this.parent?.(node, helpers, context) ?? "";
            if (node.content?.length !== 1 || node.content[0].content?.length) {
              return output;
            }
            return context.parentType === "orderedList" ? output + "&nbsp;" : output + " ";
          },
        });
      }
      return extension;
    });
  },
});

// The task tokenizer requires whitespace after an otherwise empty checkbox marker.
const VisualTaskItem = TaskItem.extend({
  renderMarkdown(node, helpers, context) {
    const output = this.parent?.(node, helpers, context) ?? "";
    return node.content?.length === 1 && !node.content[0].content?.length ? output + " " : output;
  },
});

// Share the same extensions for editing, compatibility checks, and round trips.
export function markdownExtensions() {
  return [
    VisualStarterKit.configure({
      horizontalRule: false,
      strike: false,
      underline: false,
      trailingNode: false,
      link: { openOnClick: false, autolink: false, linkOnPaste: false },
    }),
    TaskList,
    // The live task NodeView needs the same selector as the serialized HTML.
    VisualTaskItem.configure({ nested: true, HTMLAttributes: { "data-type": "taskItem" } }),
    VisualImage,
    Markdown,
  ];
}

// Keep the compatibility reason available for both the toast and the persistent mode hint.
export type VisualIssue = {
  code: "visualUnsupportedContent" | "visualRoundTripFailed";
  reason: string;
};

type VisualContent = { content: JSONContent } | VisualIssue;

export function checkedVisualContent(editor: Editor, markdown: string): VisualContent {
  const source = markdown.replace(/^\uFEFF/, "");
  if (/^(---|\+\+\+)[ \t]*\r?\n/.test(source)) {
    return { code: "visualUnsupportedContent", reason: "This document contains front matter. Edit it in Source mode." };
  }

  try {
    let unsupported: string | undefined;
    lexer.walkTokens(lexer.lexer(source), (token) => {
      unsupported ??= sourceOnly.get(token.type);
    });
    if (unsupported) {
      return { code: "visualUnsupportedContent", reason: `This document contains ${unsupported}. Edit it in Source mode.` };
    }

    const manager = editor.markdown;
    if (!manager) {
      return { code: "visualRoundTripFailed", reason: "The Visual editor could not load." };
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
    return { code: "visualRoundTripFailed", reason: "This Markdown could not be converted safely. Edit it in Source mode." };
  }

  return {
    code: "visualRoundTripFailed",
    reason:
      "Visual mode would change this document’s content or structure. Edit it in Source mode.",
  };
}

export function formatVisualMarkdown(markdown: string, original: string): string {
  const text = original.includes("\r\n") ? markdown.replace(/\r?\n/g, "\r\n") : markdown;
  return original.startsWith("\uFEFF") ? "\uFEFF" + text : text;
}

// Find real Markdown images, including reference-style links, while ignoring examples in code fences.
export function markdownImageSources(markdown: string): string[] {
  const sources = new Set<string>();

  lexer.walkTokens(lexer.lexer(markdown.replace(/^\uFEFF/, "")), (token) => {
    if (token.type === "image" && typeof token.href === "string") {
      sources.add(token.href);
    }
  });

  return [...sources];
}

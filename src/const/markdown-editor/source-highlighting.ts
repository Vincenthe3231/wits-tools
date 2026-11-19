import { HighlightStyle } from "@codemirror/language";
import { tags } from "@lezer/highlight";

// Adapt WitsFlow's semantic highlighting to the WITS Tools theme.
export const sourceHighlighting = HighlightStyle.define([
  { tag: [tags.meta, tags.punctuation, tags.comment], color: "var(--text-primary)" },
  { tag: tags.heading, color: "var(--accent-strong)", fontWeight: "700" },
  { tag: tags.strong, fontWeight: "700" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: [tags.link, tags.url], color: "var(--accent-strong)", textDecoration: "underline" },
  {
    tag: [tags.keyword, tags.operator, tags.typeName],
    color: "var(--accent-strong)",
    fontWeight: "600",
  },
  { tag: [tags.string, tags.regexp, tags.number, tags.bool], color: "var(--accent-strong)" },
  { tag: tags.invalid, textDecoration: "underline wavy" },
]);

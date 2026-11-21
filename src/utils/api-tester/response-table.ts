/** Parses, formats, and expands JSON response previews for inspection. */
interface ResponseRow {
  id: string;
  depth: number;
  name: string;
  value: string;
  expandable: boolean;
}

// Distinguish invalid or incomplete previews from valid JSON values such as null.
export function parseResponseJson(text: string): { value: unknown } | undefined {
  try {
    return { value: JSON.parse(text) };
  } catch {
    return undefined;
  }
}

// Adjust only whitespace outside strings so number literals and duplicate keys survive.
export function formatResponseJson(text: string): string {
  let depth = 0;
  const newline = () => "\n" + "  ".repeat(Math.min(depth, 40));
  return text.replace(/"(?:[^"\\]|\\.)*"|[[{]\s*[\]}]|[[{\]},:]|\s+/gs, (token) => {
    if (token.length > 1 && (token[0] === "{" || token[0] === "[")) {
      return token[0] + token[token.length - 1];
    }
    switch (token) {
      case "{":
      case "[":
        depth++;
        return token + newline();
      case "}":
      case "]":
        depth--;
        return newline() + token;
      case ",":
        return token + newline();
      case ":":
        return ": ";
      default:
        return token.trim();
    }
  });
}

// Visit only open branches. Position-based IDs distinguish repeated property names across objects.
export function responseRows(value: unknown, expanded: ReadonlySet<string>): ResponseRow[] {
  const rows: ResponseRow[] = [];
  function visit(name: string, value: unknown, id: string, depth: number) {
    const entries = value !== null && typeof value === "object" ? Object.entries(value) : undefined;
    rows.push({
      id,
      depth,
      name,
      value: entries ? (Array.isArray(value) ? "Array" : "Object") + " (" + entries.length + ")" : JSON.stringify(value),
      expandable: Boolean(entries?.length),
    });
    if (expanded.has(id)) {
      entries?.forEach(([key, child], index) => visit(key, child, id + "." + index, depth + 1));
    }
  }
  visit("Root", value, "root", 0);
  return rows;
}

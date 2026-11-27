/** Plans source-preserving edits and asset copies for a portable Markdown package. */
import { markdownLanguage } from "@codemirror/lang-markdown";

// Normalize definition labels using the same whitespace rules as Markdown.
function label(value: string) {
  return value.slice(1, -1).trim().replace(/\s+/g, " ").toLowerCase();
}

// Decode local URL escapes while leaving web and embedded sources in the document.
function imagePath(raw: string) {
  const value = raw.replace(/^<|>$/g, "").replace(/\\([()[\]<># ])/g, "$1");
  if (/^file:/i.test(value)) {
    const url = new URL(value);
    const path = decodeURIComponent(url.pathname);
    return url.hostname ? "//" + url.hostname + path : path.replace(/^\/(?=[a-z]:\/)/i, "");
  }
  if (/^(?:\/\/|[a-z][a-z\d+.-]*:)/i.test(value) && !/^[a-z]:[\\/]/i.test(value)) {
    return undefined;
  }
  const path = value.split(/[?#]/, 1)[0];
  return path ? decodeURIComponent(path.replace(/%(?![a-f\d]{2})/gi, "%25")) : undefined;
}

// Work with parser ranges so unrelated links, titles, and fenced examples keep their syntax.
export function markdownPackage(markdown: string) {
  const definitions = new Map<string, { from: number; to: number }>();
  const references = new Set<string>();
  const destinations: { from: number; to: number }[] = [];
  markdownLanguage.parser.parse(markdown).iterate({
    enter({ name, node }) {
      const url = node.getChild("URL");
      const key = node.getChild("LinkLabel");
      if (name === "LinkReference" && key && url) {
        const normalized = label(markdown.slice(key.from, key.to));
        if (!definitions.has(normalized)) definitions.set(normalized, url);
      } else if (name === "Image" && url) {
        destinations.push(url);
      } else if (name === "Image") {
        const marks = node.getChildren("LinkMark");
        const start = marks[0];
        const end = marks[1];
        if (!start || !end || markdown.slice(start.from, start.to) !== "![") return;
        const explicit = key && markdown.slice(key.from, key.to);
        references.add(label(explicit && explicit !== "[]" ? explicit : "[" + markdown.slice(start.to, end.from) + "]"));
      }
    },
  });
  for (const reference of references) {
    const definition = definitions.get(reference);
    if (definition) destinations.push(definition);
  }

  // Repeated occurrences share one asset, while equal basenames get separate filenames.
  const assets = new Map<string, string>();
  let result = markdown;
  for (const destination of destinations.sort((first, second) => second.from - first.from)) {
    const source = imagePath(markdown.slice(destination.from, destination.to));
    if (!source) continue;
    const extension = source.match(/\.([a-z0-9]{1,10})$/i)?.[1]?.toLowerCase();
    const target = assets.get(source) ?? "image-" + (assets.size + 1) + (extension ? "." + extension : "");
    assets.set(source, target);
    result = result.slice(0, destination.from) + "img/" + target + result.slice(destination.to);
  }
  return { markdown: result, assets: Array.from(assets, ([source, target]) => ({ source, target })) };
}

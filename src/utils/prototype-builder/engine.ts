/** Applies document commands and calculates temporary pointer projections. */
import type { CanvasNode, ComponentDefinition, NodeGeometry, PrototypeBlock, PrototypeDocument, ScreenSize } from "../../api/prototype-builder";
import { coordinate, dimension, layerNodes, resizeCorner, resizeWidth, toDocument, type Camera, type Point } from "./geometry";
import { definitionKey } from "./document";
import { validStyle, validTheme, type Style, type Theme } from "./appearance";

export type EditCommand =
  | { type: "insert"; node: CanvasNode; definition?: ComponentDefinition }
  | { type: "remove"; id: string }
  | { type: "geometry"; id: string; geometry: NodeGeometry }
  | { type: "layer"; id: string; order: Parameters<typeof layerNodes>[2] }
  | { type: "content"; block: PrototypeBlock }
  | { type: "screen"; size: Partial<ScreenSize> }
  | { type: "theme"; theme: Theme }
  | { type: "style"; id: string; style: Style };

// Validate geometry once regardless of whether it came from a gesture or Properties.
function validGeometry(node: CanvasNode) {
  return coordinate(node.x) === node.x && coordinate(node.y) === node.y
    && dimension(node.width) === node.width
    && (node.height === undefined || dimension(node.height) === node.height);
}

export function applyEdit(document: PrototypeDocument, command: EditCommand): PrototypeDocument {
  const nodes = document.nodes;
  if (command.type === "theme") return document.version === 4 && validTheme(command.theme)
    && JSON.stringify(command.theme) !== JSON.stringify(document.theme)
    ? { ...document, theme: command.theme } : document;
  if (command.type === "screen") {
    const screen = { ...document.screen, ...command.size };
    return dimension(screen.width) === screen.width && dimension(screen.height) === screen.height
      && (screen.width !== document.screen.width || screen.height !== document.screen.height)
      ? { ...document, screen } : document;
  }
  if (command.type === "insert") {
    const node = command.node;
    if (!node.block.id.trim() || !validGeometry(node) || nodes.some(({ block }) => block.id === node.block.id)) return document;
    if (node.block.kind !== "imported") return { ...document, nodes: [...nodes, node] };
    if (document.version !== 4) return document;
    const key = definitionKey({ id: node.block.definitionId, revision: node.block.revision });
    const definitions = document.definitions;
    const existing = definitions.find((definition) => definitionKey(definition) === key);
    if (!existing && (!command.definition || definitionKey(command.definition) !== key)) return document;
    if (existing && command.definition && JSON.stringify(existing) !== JSON.stringify(command.definition)) return document;
    return { ...document, nodes: [...nodes, node], definitions: existing ? definitions : [...definitions, command.definition!] };
  }

  const id = command.type === "content" ? command.block.id : command.id;
  const node = nodes.find(({ block }) => block.id === id);
  if (!node) return document;
  if (command.type === "remove") {
    const nodes = document.nodes.filter((item) => item !== node);
    if (document.version !== 4) return { ...document, nodes };
    const definitions = document.definitions.filter((definition) => nodes.some(({ block }) =>
      block.kind === "imported" && block.definitionId === definition.id && block.revision === definition.revision));
    return { ...document, nodes, definitions };
  }
  if (command.type === "layer") {
    const ordered = layerNodes(nodes, id, command.order);
    return ordered === nodes ? document : { ...document, nodes: ordered };
  }

  if (command.type === "content" && node.block.kind !== command.block.kind) return document;
  if (command.type === "style" && (document.version !== 4 || !validStyle(command.style))) return document;
  if (command.type === "content" && command.block.kind === "imported") {
    const { definitionId, revision, values } = command.block;
    if (node.block.kind !== "imported" || node.block.definitionId !== definitionId || node.block.revision !== revision ||
      !document.definitions?.some((definition) => definition.id === definitionId && definition.revision === revision
        && Object.entries(values).every(([key, value]) => Object.prototype.hasOwnProperty.call(definition.fields, key)
          && new TextEncoder().encode(value).length <= 2000))) {
      return document;
    }
  }
  const updated = command.type === "content" ? { ...node, block: command.block }
    : command.type === "style" ? { ...node, style: Object.keys(command.style).length ? command.style : undefined }
      : { ...node, ...command.geometry };
  return validGeometry(updated) && JSON.stringify(updated) !== JSON.stringify(node)
    ? { ...document, nodes: nodes.map((item) => item === node ? updated : item) }
    : document;
}

export type GestureAction =
  | { kind: "pan" | "palette" }
  | { kind: "move"; node: CanvasNode }
  | { kind: "resize"; node: CanvasNode; edge: "left" | "right"; cornerHeight?: number };
export type Gesture = { phase: "pending" | "active"; start: Point; camera: Camera; action: GestureAction };

// Keep the captured camera and geometry fixed for the full pointer session.
export function advanceGesture(gesture: Gesture, point: Point) {
  const dx = point.x - gesture.start.x;
  const dy = point.y - gesture.start.y;
  if (gesture.phase === "pending" && Math.hypot(dx, dy) < 4) return undefined;
  const { action, camera } = gesture;
  const delta = { x: dx / camera.zoom, y: dy / camera.zoom };
  if (action.kind === "pan") return { kind: "pan" as const, camera: { ...camera, x: camera.x + dx, y: camera.y + dy } };
  if (action.kind === "palette") return { kind: "palette" as const, point: toDocument(point, camera) };
  if (action.kind === "move") return { kind: "geometry" as const, id: action.node.block.id,
    geometry: { x: coordinate(action.node.x + delta.x), y: coordinate(action.node.y + delta.y) } };
  if (action.kind === "resize") return { kind: "geometry" as const, id: action.node.block.id,
    geometry: action.cornerHeight === undefined ? resizeWidth(action.node, delta.x, action.edge)
      : resizeCorner(action.node, action.cornerHeight, delta, action.edge) };
}

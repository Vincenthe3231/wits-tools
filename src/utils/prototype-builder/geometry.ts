/** Maps viewport pixels to document geometry and lays out older linear documents. */
import type { BlockKind, CanvasNode, LinearDocument, PrototypeDocument, ScreenSize } from "../../api/prototype-builder";

export type Point = { x: number; y: number };
export type Camera = Point & { zoom: number };
export const MAX_DIMENSION = 1_000_000;
export const MIN_COORDINATE = -2_147_483_648;
export const MAX_COORDINATE = 2_147_483_647;

// Use the same rounding and limits for pointer edits and the inspector.
export const coordinate = (value: number) => Math.max(MIN_COORDINATE, Math.min(MAX_COORDINATE, Math.round(value)));
export const dimension = (value: number) => Math.max(1, Math.min(MAX_DIMENSION, Math.round(value)));
export const toDocument = (point: Point, camera: Camera): Point => ({
  x: (point.x - camera.x) / camera.zoom,
  y: (point.y - camera.y) / camera.zoom,
});

// Keep the opposite edge fixed when dragging either horizontal handle.
export function resizeWidth(node: CanvasNode, delta: number, edge: "left" | "right") {
  const width = dimension(node.width + (edge === "left" ? -delta : delta));
  if (edge === "right") return { x: node.x, width };
  const x = coordinate(node.x + node.width - width);
  return { x, width: node.x + node.width - x };
}

// Bottom corners move both axes while keeping the opposite top corner fixed.
export function resizeCorner(node: CanvasNode, renderedHeight: number, delta: Point, edge: "left" | "right") {
  return { ...resizeWidth(node, delta.x, edge), height: dimension(renderedHeight + delta.y) };
}

// Fit the target using viewport pixels, leaving room for the workspace chrome.
export function fitCamera(target: Point & ScreenSize, viewport: ScreenSize, padding: number, maxZoom: number): Camera {
  const zoom = Math.min(maxZoom, Math.max(1, viewport.width - padding * 2) / target.width,
    Math.max(1, viewport.height - padding * 2) / target.height);
  return {
    x: (viewport.width - target.width * zoom) / 2 - target.x * zoom,
    y: (viewport.height - target.height * zoom) / 2 - target.y * zoom,
    zoom,
  };
}

// Default widths preserve readable component proportions on an empty canvas.
export const blockWidth = (kind: BlockKind) => kind === "signIn" ? 400 : kind === "button" ? 180 : 720;

export function emptyCanvas(): PrototypeDocument {
  return { version: 3, screen: { width: 1440, height: 900 }, nodes: [] };
}

// The caller measures the actual renderer, so wrapping never overlaps the next row.
export function stackCanvas(linear: LinearDocument, heights: number[]): PrototypeDocument {
  const canvas = emptyCanvas();
  let y = 32;
  canvas.nodes = linear.blocks.map((block, index) => {
    const height = heights[index];
    if (!Number.isFinite(height) || height! < 0) throw new Error("Missing measurement");
    const node = { block, x: 360, y, width: blockWidth(block.kind) };
    y += Math.ceil(height!) + 16;
    return node;
  });
  const height = Math.max(canvas.screen.height, y + 16);
  if (height > MAX_DIMENSION) throw new Error("Stack exceeds the supported canvas size");
  canvas.screen.height = height;
  return canvas;
}

// Changing stack order moves exactly one node and leaves geometry untouched.
export function layerNodes(nodes: CanvasNode[], id: string, action: "back" | "lower" | "raise" | "front") {
  const index = nodes.findIndex((node) => node.block.id === id);
  if (index < 0) return nodes;
  const target = { back: 0, lower: Math.max(0, index - 1), raise: Math.min(nodes.length - 1, index + 1),
    front: nodes.length - 1 }[action];
  if (target === index) return nodes;
  const result = [...nodes];
  const [node] = result.splice(index, 1);
  result.splice(target, 0, node!);
  return result;
}

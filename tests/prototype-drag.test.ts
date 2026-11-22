/** Checks the insertion slot and line geometry used by the Prototype Builder drag gesture. */
import { describe, expect, test } from "bun:test";

import { findDropPosition } from "../src/utils/prototype-builder/drop-position";

describe("prototype canvas drop positions", () => {
  // Empty canvases expose the first slot at the center of their drop surface.
  test("finds an empty canvas slot", () => {
    expect(findDropPosition([], 90, 120)).toEqual({ index: 0, top: 120 });
  });

  // Midpoints create ordered insertion slots even when blocks have unequal heights.
  test("places the indicator before, between, and after blocks", () => {
    const blocks = [{ top: 100, bottom: 180 }, { top: 200, bottom: 320 }];

    expect(findDropPosition(blocks, 110, 0)).toEqual({ index: 0, top: 100 });
    expect(findDropPosition(blocks, 150, 0)).toEqual({ index: 1, top: 190 });
    expect(findDropPosition(blocks, 220, 0)).toEqual({ index: 1, top: 190 });
    expect(findDropPosition(blocks, 300, 0)).toEqual({ index: 2, top: 320 });
  });
});

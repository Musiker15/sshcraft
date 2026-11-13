import { expect, test } from "vitest";
import { cellOf, dragSizes, gridShape, normalizeSizes } from "./grid";

test.each([
  [1, 1, 1],
  [2, 2, 1],
  [3, 2, 2],
  [4, 2, 2],
  [5, 3, 2],
  [6, 3, 2],
  [7, 3, 3],
  [9, 3, 3],
  [10, 4, 3],
  [12, 4, 3],
  [13, 4, 4],
  [16, 4, 4],
])("%i panes make %i columns and %i rows", (n, cols, rows) => {
  expect(gridShape(n)).toEqual({ cols, rows });
});

test("the last pane reaches the right edge", () => {
  expect(cellOf(0, 3)).toEqual({ row: 0, col: 0, colSpan: 1 });
  expect(cellOf(1, 3)).toEqual({ row: 0, col: 1, colSpan: 1 });
  expect(cellOf(2, 3)).toEqual({ row: 1, col: 0, colSpan: 2 });
  expect(cellOf(4, 5)).toEqual({ row: 1, col: 1, colSpan: 2 });
  expect(cellOf(3, 4)).toEqual({ row: 1, col: 1, colSpan: 1 });
});

test("sizes that do not fit the grid become equal", () => {
  expect(normalizeSizes([1, 2], 3)).toEqual([1, 1, 1]);
  expect(normalizeSizes([1, 0], 2)).toEqual([1, 1]);
  expect(normalizeSizes([1, Number.NaN], 2)).toEqual([1, 1]);
  expect(normalizeSizes([1, 3], 2)).toEqual([1, 3]);
});

test("dragging a divider keeps the sum and a minimum on both sides", () => {
  const moved = dragSizes([1, 1], 0, 0.25);
  expect(moved[0] + moved[1]).toBeCloseTo(2);
  expect(moved[0]).toBeCloseTo(1.5);
  const far = dragSizes([1, 1], 0, 5);
  expect(far[1]).toBeCloseTo(0.1);
  expect(far[0] + far[1]).toBeCloseTo(2);
  const back = dragSizes([1, 1, 1], 1, -5);
  expect(back[1]).toBeCloseTo(0.1);
  expect(back[0]).toBe(1);
  expect(back[1] + back[2]).toBeCloseTo(2);
});

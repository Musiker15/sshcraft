import { expect, test } from "vitest";
import { lineForOffset, offsetForLine, pieces } from "./marks";

test("pieces cut at every step and at every mark", () => {
  expect(pieces(10, [3], 4)).toEqual([3, 4, 8, 10]);
  expect(pieces(8, [4, 4, 0, 8, 12], 4)).toEqual([4, 8]);
  expect(pieces(0, [], 4)).toEqual([]);
});

test("a mark finds the line written up to it", () => {
  const table = [
    { offset: 4, line: 0 },
    { offset: 8, line: 2 },
    { offset: 12, line: 5 },
  ];
  expect(lineForOffset(table, 0)).toBe(0);
  expect(lineForOffset(table, 8)).toBe(2);
  expect(lineForOffset(table, 9)).toBe(2);
  expect(lineForOffset(table, 99)).toBe(5);
});

test("a line finds the last position written before it", () => {
  const table = [
    { offset: 4, line: 0 },
    { offset: 8, line: 2 },
    { offset: 12, line: 5 },
  ];
  expect(offsetForLine(table, 0)).toBe(4);
  expect(offsetForLine(table, 3)).toBe(8);
  expect(offsetForLine(table, 10)).toBe(12);
  expect(offsetForLine([], 3)).toBe(0);
});

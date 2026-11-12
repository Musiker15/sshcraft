import { expect, test } from "vitest";
import { fitMenu } from "./ContextMenu";

const view = { width: 800, height: 600 };
const menu = { width: 180, height: 120 };

test("a menu that fits stays where it was opened", () => {
  expect(fitMenu({ x: 100, y: 100 }, menu, view)).toEqual({ x: 100, y: 100 });
});

test("a menu at the right edge moves left into the window", () => {
  expect(fitMenu({ x: 760, y: 40 }, menu, view)).toEqual({ x: 800 - 180 - 8, y: 40 });
});

test("a menu at the bottom moves up", () => {
  expect(fitMenu({ x: 10, y: 590 }, menu, view)).toEqual({ x: 10, y: 600 - 120 - 8 });
});

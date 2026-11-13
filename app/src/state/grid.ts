// Where the panes of a split workspace sit: a near-square grid, the last pane reaching the right
// edge. Column widths and row heights are shares (CSS `fr`).

/** A share is never less than this part of the average share. */
const MIN_SHARE = 0.1;

export function gridShape(n: number): { cols: number; rows: number } {
  const count = Math.max(1, n);
  const cols = Math.ceil(Math.sqrt(count));
  return { cols, rows: Math.ceil(count / cols) };
}

export function cellOf(index: number, n: number): { row: number; col: number; colSpan: number } {
  const { cols } = gridShape(n);
  const row = Math.floor(index / cols);
  const col = index % cols;
  return { row, col, colSpan: index === n - 1 ? cols - col : 1 };
}

/**
 * How many rows, from the top, the divider between column `boundary` and `boundary + 1` runs
 * through: it stops above a last pane that spans that boundary.
 */
export function colDividerRows(boundary: number, n: number): number {
  const { rows } = gridShape(n);
  return boundary < cellOf(n - 1, n).col ? rows : rows - 1;
}

/** `sizes` when they fit `count` cells and are all positive numbers, else equal shares. */
export function normalizeSizes(sizes: number[], count: number): number[] {
  const fits = sizes.length === count && sizes.every((s) => Number.isFinite(s) && s > 0);
  return fits ? sizes : Array.from({ length: count }, () => 1);
}

/**
 * Moves divider `divider` (between cell `divider` and `divider + 1`) by `delta`, a fraction of
 * the whole width or height. Only the two neighbours change, their sum stays.
 */
export function dragSizes(sizes: number[], divider: number, delta: number): number[] {
  const total = sizes.reduce((a, b) => a + b, 0);
  const min = (MIN_SHARE * total) / sizes.length;
  const pair = sizes[divider] + sizes[divider + 1];
  const left = Math.min(pair - min, Math.max(min, sizes[divider] + delta * total));
  const next = [...sizes];
  next[divider] = left;
  next[divider + 1] = pair - left;
  return next;
}

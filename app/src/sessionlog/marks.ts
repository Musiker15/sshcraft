// Where bookmarks and comments of a session log are on screen. The viewer writes a log into
// xterm in pieces and notes after each piece on which line the cursor is: that table maps
// byte positions to lines and back.

/** A position in the log and the line the cursor was on after writing up to it. */
export type LineAt = { offset: number; line: number };

/** End positions of the pieces to write: every `step` bytes and at every mark, then the end. */
export function pieces(size: number, offsets: number[], step = 4096): number[] {
  if (size <= 0) return [];
  const cuts = new Set<number>([size]);
  for (let at = step; at < size; at += step) cuts.add(at);
  for (const offset of offsets) if (offset > 0 && offset < size) cuts.add(offset);
  return [...cuts].sort((a, b) => a - b);
}

/** The line a mark at `offset` is on: the line after the last piece that ends at or before it. */
export function lineForOffset(table: LineAt[], offset: number): number {
  let line = 0;
  for (const entry of table) {
    if (entry.offset > offset) break;
    line = entry.line;
  }
  return line;
}

/** The position for a new mark at `line`: the end of the last piece written up to that line. */
export function offsetForLine(table: LineAt[], line: number): number {
  let offset = 0;
  for (const entry of table) {
    if (entry.line > line) break;
    offset = entry.offset;
  }
  return offset;
}

// Window-wide shortcuts, freely bindable. Inside the terminal only "terminal-safe" combinations
// count (Ctrl+Shift without Alt, or Ctrl+Tab), so keys the shell needs (Ctrl+W, Ctrl+K, Ctrl+R,
// Alt+..., AltGr = Ctrl+Alt) always reach it. The notation matches `valid_combo` in Rust.

export const SHORTCUT_ACTIONS = [
  "palette",
  "newTab",
  "closeTab",
  "nextTab",
  "prevTab",
  "splitPane",
  "closePane",
  "nextPane",
  "prevPane",
  "toggleLayout",
  "toggleBroadcast",
] as const;
export type ShortcutAction = (typeof SHORTCUT_ACTIONS)[number];
export type Bindings = Record<ShortcutAction, string[]>;

export const DEFAULT_SHORTCUTS: Bindings = {
  palette: ["Ctrl+Shift+P", "Ctrl+K"],
  newTab: ["Ctrl+Shift+T"],
  closeTab: ["Ctrl+Shift+W"],
  nextTab: ["Ctrl+Tab", "Ctrl+PageDown"],
  prevTab: ["Ctrl+Shift+Tab", "Ctrl+PageUp"],
  splitPane: ["Ctrl+Shift+D"],
  closePane: ["Ctrl+Shift+X"],
  nextPane: ["Ctrl+Shift+PageDown"],
  prevPane: ["Ctrl+Shift+PageUp"],
  toggleLayout: ["Ctrl+Shift+M"],
  toggleBroadcast: ["Ctrl+Shift+B"],
};

export type KeyLike = { key: string; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean };

const MODIFIERS = ["Ctrl", "Alt", "Shift", "Meta"];
const MODIFIER_KEYS = new Set(["Control", "Alt", "AltGraph", "Shift", "Meta", "OS", "Dead", "Unidentified"]);
const MAX_COMBO_LEN = 40;

/** The combination a key press stands for, or `null` for a lone modifier key. */
export function comboOf(e: KeyLike): string | null {
  if (MODIFIER_KEYS.has(e.key) || e.key === "") return null;
  const key = [...e.key].length === 1 ? e.key.toUpperCase() : e.key;
  const parts = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Meta"].filter(
    (p): p is string => typeof p === "string",
  );
  return [...parts, key].join("+");
}

function split(combo: string): { modifiers: string[]; key: string } {
  const parts = combo.split("+");
  return { modifiers: parts.slice(0, -1), key: parts[parts.length - 1] };
}

function isFunctionKey(key: string): boolean {
  const n = /^F(\d{1,2})$/.exec(key);
  return n !== null && Number(n[1]) >= 1 && Number(n[1]) <= 24;
}

export function isValidCombo(combo: string): boolean {
  if (combo === "" || combo.length > MAX_COMBO_LEN) return false;
  const { modifiers, key } = split(combo);
  let last = -1;
  for (const m of modifiers) {
    const position = MODIFIERS.indexOf(m);
    if (position <= last) return false;
    last = position;
  }
  if (key === "" || MODIFIERS.includes(key)) return false;
  if ([...key].length > 1 && !/^[A-Za-z0-9]+$/.test(key)) return false;
  if (key.startsWith("F") && key.length > 1 && !isFunctionKey(key)) return false;
  return modifiers.some((m) => m !== "Shift") || isFunctionKey(key);
}

export function isTerminalSafe(combo: string): boolean {
  const { modifiers, key } = split(combo);
  if (!modifiers.includes("Ctrl") || modifiers.includes("Alt")) return false;
  return modifiers.includes("Shift") || key === "Tab";
}

/** The defaults, replaced action by action by the user's own bindings. */
export function resolveBindings(custom: Record<string, string[]>): Bindings {
  const bindings = { ...DEFAULT_SHORTCUTS };
  for (const action of SHORTCUT_ACTIONS) {
    const own = custom[action];
    if (Array.isArray(own)) bindings[action] = own.filter(isValidCombo);
  }
  return bindings;
}

export function matchShortcut(e: KeyLike, context: "terminal" | "app", bindings: Bindings): ShortcutAction | null {
  const combo = comboOf(e);
  if (combo === null) return null;
  if (context === "terminal" && !isTerminalSafe(combo)) return null;
  return SHORTCUT_ACTIONS.find((action) => bindings[action].includes(combo)) ?? null;
}

/** The other action that already uses `combo`, if any. */
export function findConflict(bindings: Bindings, action: ShortcutAction, combo: string): ShortcutAction | null {
  return SHORTCUT_ACTIONS.find((other) => other !== action && bindings[other].includes(combo)) ?? null;
}

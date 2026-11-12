// Window-wide shortcuts. Inside the terminal only Ctrl+Shift and Ctrl+Tab combinations count,
// so keys the shell needs (Ctrl+W, Ctrl+K, Ctrl+R, Alt+...) always reach it.

export type ShortcutAction = "palette" | "newTab" | "closeTab" | "nextTab" | "prevTab";

export type KeyLike = { key: string; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean };

export function matchShortcut(e: KeyLike, context: "terminal" | "app"): ShortcutAction | null {
  if (!e.ctrlKey || e.altKey || e.metaKey) return null;
  const key = e.key.toLowerCase();
  if (e.shiftKey) {
    switch (key) {
      case "p":
        return "palette";
      case "t":
        return "newTab";
      case "w":
        return "closeTab";
      case "tab":
        return "prevTab";
      default:
        return null;
    }
  }
  switch (key) {
    case "tab":
      return "nextTab";
    // Ctrl+PageUp/PageDown are common bindings in vim, tmux and others: not in the terminal.
    case "pagedown":
      return context === "app" ? "nextTab" : null;
    case "pageup":
      return context === "app" ? "prevTab" : null;
    case "k":
      return context === "app" ? "palette" : null;
    default:
      return null;
  }
}

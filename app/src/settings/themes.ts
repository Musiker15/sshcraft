// Terminal colour schemes and the app's light/dark choice. Solarized, Nord and Dracula use
// the colours of the public, MIT-licensed schemes; the SSHCraft schemes are our own.
import type { ITheme } from "@xterm/xterm";
import type { AppTheme } from "../api";

type Colors = Required<
  Pick<
    ITheme,
    | "background" | "foreground" | "cursor" | "selectionBackground"
    | "black" | "red" | "green" | "yellow" | "blue" | "magenta" | "cyan" | "white"
    | "brightBlack" | "brightRed" | "brightGreen" | "brightYellow"
    | "brightBlue" | "brightMagenta" | "brightCyan" | "brightWhite"
  >
>;

export type TerminalTheme = { id: string; name: string; colors: Colors };

const solarizedAnsi = {
  black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
  blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
  brightBlack: "#002b36", brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83",
  brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1", brightWhite: "#fdf6e3",
};

export const TERMINAL_THEMES: TerminalTheme[] = [
  {
    id: "sshcraft-dark",
    name: "SSHCraft Dark",
    colors: {
      background: "#111318", foreground: "#e7e8ec", cursor: "#f5a524", selectionBackground: "#3a3324",
      black: "#1b1e26", red: "#e5534b", green: "#57ab5a", yellow: "#d4a72c",
      blue: "#539bf5", magenta: "#b083f0", cyan: "#39c5cf", white: "#c9ccd3",
      brightBlack: "#636a78", brightRed: "#ff7b72", brightGreen: "#7ee787", brightYellow: "#f5c451",
      brightBlue: "#79c0ff", brightMagenta: "#d2a8ff", brightCyan: "#56d4dd", brightWhite: "#ffffff",
    },
  },
  {
    id: "sshcraft-light",
    name: "SSHCraft Light",
    colors: {
      background: "#fbfbfc", foreground: "#1b1d23", cursor: "#c47f00", selectionBackground: "#f3e1bd",
      black: "#1b1d23", red: "#c4312b", green: "#2c7a39", yellow: "#9a6a00",
      blue: "#1f5fbf", magenta: "#8a3fc2", cyan: "#127f86", white: "#d6d8de",
      brightBlack: "#5f6472", brightRed: "#e5534b", brightGreen: "#3d9a4e", brightYellow: "#b98400",
      brightBlue: "#3d7ee0", brightMagenta: "#a35cd6", brightCyan: "#1a9ba3", brightWhite: "#ffffff",
    },
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    colors: { background: "#002b36", foreground: "#839496", cursor: "#93a1a1", selectionBackground: "#073642", ...solarizedAnsi },
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    colors: { background: "#fdf6e3", foreground: "#657b83", cursor: "#586e75", selectionBackground: "#eee8d5", ...solarizedAnsi },
  },
  {
    id: "nord",
    name: "Nord",
    colors: {
      background: "#2e3440", foreground: "#d8dee9", cursor: "#d8dee9", selectionBackground: "#434c5e",
      black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b",
      blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0",
      brightBlack: "#4c566a", brightRed: "#bf616a", brightGreen: "#a3be8c", brightYellow: "#ebcb8b",
      brightBlue: "#81a1c1", brightMagenta: "#b48ead", brightCyan: "#8fbcbb", brightWhite: "#eceff4",
    },
  },
  {
    id: "dracula",
    name: "Dracula",
    colors: {
      background: "#282a36", foreground: "#f8f8f2", cursor: "#f8f8f2", selectionBackground: "#44475a",
      black: "#21222c", red: "#ff5555", green: "#50fa7b", yellow: "#f1fa8c",
      blue: "#bd93f9", magenta: "#ff79c6", cyan: "#8be9fd", white: "#f8f8f2",
      brightBlack: "#6272a4", brightRed: "#ff6e6e", brightGreen: "#69ff94", brightYellow: "#ffffa5",
      brightBlue: "#d6acff", brightMagenta: "#ff92df", brightCyan: "#a4ffff", brightWhite: "#ffffff",
    },
  },
];

export function terminalTheme(id: string): TerminalTheme {
  return TERMINAL_THEMES.find((t) => t.id === id) ?? TERMINAL_THEMES[0];
}

export function resolveAppTheme(setting: AppTheme, prefersDark: boolean): "dark" | "light" {
  if (setting === "system") return prefersDark ? "dark" : "light";
  return setting;
}

export const FONT_PRESETS = ["Cascadia Mono", "Consolas", "JetBrains Mono", "Fira Code", "Courier New"];

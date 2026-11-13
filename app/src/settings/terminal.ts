// Terminal types a session can ask for, the same list as `TERMINAL_TYPES` in Rust (settings.rs).
export const TERMINAL_TYPES = [
  "xterm-256color",
  "xterm",
  "screen-256color",
  "tmux-256color",
  "linux",
  "vt220",
  "vt100",
] as const;

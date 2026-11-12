// Keyboard focus follows the visible terminal, so keys never go to a session in a hidden tab.

export function syncTerminalFocus(terminal: { focus(): void; blur(): void }, active: boolean): void {
  if (active) terminal.focus();
  else terminal.blur();
}

import { useEffect } from "react";

export type MenuItem = { label: string; onSelect: () => void; danger?: boolean; disabled?: boolean; title?: string };

/** Right-click menu at the pointer; any click elsewhere or Escape closes it. */
export function ContextMenu({ items, at, onClose }: { items: MenuItem[]; at: { x: number; y: number }; onClose: () => void }) {
  useEffect(() => {
    const close = () => onClose();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
    };
  }, [onClose]);
  return (
    <ul className="context-menu" role="menu" style={{ left: at.x, top: at.y }} onMouseDown={(e) => e.stopPropagation()}>
      {items.map((item) => (
        <li key={item.label}>
          <button
            role="menuitem"
            className={item.danger ? "danger" : undefined}
            disabled={item.disabled}
            title={item.title}
            onClick={() => {
              onClose();
              item.onSelect();
            }}
          >
            {item.label}
          </button>
        </li>
      ))}
    </ul>
  );
}

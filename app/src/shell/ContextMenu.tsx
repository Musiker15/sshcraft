import { useEffect, useLayoutEffect, useRef, useState } from "react";

export type MenuItem = { label: string; onSelect: () => void; danger?: boolean; disabled?: boolean; title?: string };

type Point = { x: number; y: number };
type Size = { width: number; height: number };

/** Space kept between a menu and the window edge. */
const EDGE = 8;

/** Where a menu of `size` opened at `at` goes so it stays inside the window: shifted left (or up) where it would cut off. */
export function fitMenu(at: Point, size: Size, view: Size): Point {
  const x = Math.max(EDGE, Math.min(at.x, view.width - size.width - EDGE));
  const y = Math.max(EDGE, Math.min(at.y, view.height - size.height - EDGE));
  return { x, y };
}

/** Right-click menu at the pointer; any click elsewhere or Escape closes it. */
export function ContextMenu({ items, at, onClose }: { items: MenuItem[]; at: Point; onClose: () => void }) {
  const ref = useRef<HTMLUListElement>(null);
  const [pos, setPos] = useState<Point>(at);
  useLayoutEffect(() => {
    const box = ref.current?.getBoundingClientRect();
    if (box) {
      setPos(fitMenu(at, box, { width: window.innerWidth, height: window.innerHeight }));
    }
  }, [at]);
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
    <ul
      ref={ref}
      className="context-menu"
      role="menu"
      style={{ left: pos.x, top: pos.y }}
      onMouseDown={(e) => e.stopPropagation()}
    >
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

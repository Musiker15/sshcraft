import { ArrowLeftRight, FileClock, KeyRound, type LucideIcon, Server, Settings, SquareCode } from "lucide-react";
import { useT } from "../i18n/i18n";

export type Page = "hosts" | "keys" | "snippets" | "forwarding" | "logs" | "settings";

const MAIN: { page: Page; icon: LucideIcon }[] = [
  { page: "hosts", icon: Server },
  { page: "keys", icon: KeyRound },
  { page: "snippets", icon: SquareCode },
  { page: "forwarding", icon: ArrowLeftRight },
  { page: "logs", icon: FileClock },
];

/** Navigation of the start page; settings sit at the bottom. */
export function NavSidebar({ page, onPage }: { page: Page; onPage: (page: Page) => void }) {
  const t = useT();
  const item = (p: Page, Icon: LucideIcon) => (
    <button
      key={p}
      className={p === page ? "nav-item active" : "nav-item"}
      aria-current={p === page ? "page" : undefined}
      onClick={() => onPage(p)}
    >
      <Icon size={18} aria-hidden="true" />
      <span>{t(`nav.${p}`)}</span>
    </button>
  );
  return (
    <nav className="nav">
      {MAIN.map(({ page: p, icon }) => item(p, icon))}
      <div className="nav-spacer" />
      {item("settings", Settings)}
    </nav>
  );
}

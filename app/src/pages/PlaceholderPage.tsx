import { useT } from "../i18n/i18n";
import type { Page } from "../shell/NavSidebar";

/** Pages that get their content in a later version (snippets, logs). */
export function PlaceholderPage({ page }: { page: Page }) {
  const t = useT();
  return (
    <section className="page">
      <header className="page-head">
        <h1>{t(`nav.${page}`)}</h1>
      </header>
      <p className="empty">{t("placeholder.text")}</p>
    </section>
  );
}

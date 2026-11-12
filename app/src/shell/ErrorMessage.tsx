import { errorDetail, errorText } from "../i18n/errors";
import { useT } from "../i18n/i18n";

/** A translated error, with the technical detail folded away. */
export function ErrorMessage({ error }: { error: unknown }) {
  const t = useT();
  const detail = errorDetail(error);
  return (
    <div className="error">
      <div>{errorText(t, error)}</div>
      {detail && (
        <details>
          <summary>{t("common.details")}</summary>
          <code className="detail">{detail}</code>
        </details>
      )}
    </div>
  );
}

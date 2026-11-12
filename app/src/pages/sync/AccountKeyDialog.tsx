import { useState } from "react";
import type { AccountCreated } from "../../api";
import { useT } from "../../i18n/i18n";
import { ErrorMessage } from "../../shell/ErrorMessage";
import { svgDataUrl } from "./syncView";

/**
 * Shows the new account key once. It lives only in this dialog's props and is gone when the
 * dialog closes; closing needs the confirmation that it was saved.
 */
export function AccountKeyDialog({ created, onClose }: { created: AccountCreated; onClose: () => void }) {
  const t = useT();
  const [saved, setSaved] = useState(false);
  const [copied, setCopied] = useState(false);

  return (
    <div className="dialog account-key">
      <h2>{t("sync.keyTitle")}</h2>
      <p className="hint">{t("sync.keyHint")}</p>
      {created.unfinished && (
        <>
          <p className="error">{t("sync.unfinished")}</p>
          <ErrorMessage error={created.unfinished} />
        </>
      )}
      <code className="account-key-text">{created.account_key}</code>
      <div className="buttons">
        <button
          onClick={() => {
            void navigator.clipboard.writeText(created.account_key).then(() => setCopied(true));
          }}
        >
          {copied ? t("sync.copied") : t("sync.copy")}
        </button>
      </div>
      {created.qr_svg && <img className="qr" src={svgDataUrl(created.qr_svg)} alt={t("sync.qrAlt")} />}
      <label className="check">
        <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
        {t("sync.keySaved")}
      </label>
      <div className="buttons">
        <button className="primary" disabled={!saved} onClick={onClose}>
          {t("sync.keySavedClose")}
        </button>
      </div>
    </div>
  );
}

import { useState } from "react";
import { accountKeyQr } from "../../api";
import { useT } from "../../i18n/i18n";
import { ErrorMessage } from "../../shell/ErrorMessage";
import { svgDataUrl } from "./syncView";

/** Shows a pasted account key as a QR code for a phone. Nothing is stored. */
export function AddDeviceDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [text, setText] = useState("");
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<unknown>(null);

  async function show() {
    setError(null);
    try {
      setSvg(await accountKeyQr(text));
    } catch (e) {
      setSvg(null);
      setError(e);
    }
  }

  return (
    <div className="dialog">
      <h2>{t("sync.addDevice")}</h2>
      <p className="hint">{t("sync.addDeviceHint")}</p>
      <label>
        {t("sync.accountKey")}
        <input type="password" value={text} onChange={(e) => setText(e.target.value)} autoComplete="off" />
      </label>
      {error !== null && <ErrorMessage error={error} />}
      {svg && <img className="qr" src={svgDataUrl(svg)} alt={t("sync.qrAlt")} />}
      <div className="buttons">
        <button onClick={onClose}>{t("common.close")}</button>
        <button className="primary" disabled={!text.trim()} onClick={() => void show()}>
          {t("sync.showQr")}
        </button>
      </div>
    </div>
  );
}

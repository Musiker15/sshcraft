import { useT } from "../i18n/i18n";

type Props = { message: string; confirmLabel: string; onConfirm: () => void; onCancel: () => void };

/** Replaces window.confirm: own look, translated buttons, "cancel" has the focus. */
export function ConfirmDialog({ message, confirmLabel, onConfirm, onCancel }: Props) {
  const t = useT();
  return (
    <div className="overlay" onKeyDown={(e) => e.key === "Escape" && onCancel()}>
      <div className="dialog" role="alertdialog" aria-modal="true">
        <p>{message}</p>
        <div className="buttons">
          <button onClick={onCancel} autoFocus>
            {t("common.cancel")}
          </button>
          <button className="danger" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

import { X } from "lucide-react";
import { useState } from "react";
import { useT } from "../i18n/i18n";

type Props = { value: string[]; onChange: (tags: string[]) => void };

const same = (a: string, b: string) => a.toLocaleLowerCase() === b.toLocaleLowerCase();

/** Tags as chips; Enter or a comma adds what was typed, a tag shows only once. */
export function TagInput({ value, onChange }: Props) {
  const t = useT();
  const [draft, setDraft] = useState("");

  function add(text: string) {
    const tag = text.trim();
    if (tag && !value.some((v) => same(v, tag))) onChange([...value, tag]);
    setDraft("");
  }

  return (
    <div className="tag-input">
      {value.map((tag) => (
        <span key={tag} className="chip">
          {tag}
          <button
            type="button"
            className="icon"
            aria-label={t("tags.remove", { tag })}
            onClick={() => onChange(value.filter((v) => v !== tag))}
          >
            <X size={12} aria-hidden="true" />
          </button>
        </span>
      ))}
      <input
          aria-label={t("form.tags")}
          value={draft}
          placeholder={t("tags.placeholder")}
          onChange={(e) => {
            const text = e.target.value;
            if (text.endsWith(",")) add(text.slice(0, -1));
            else setDraft(text);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add(draft);
            } else if (e.key === "Backspace" && !draft && value.length > 0) {
              onChange(value.slice(0, -1));
            }
          }}
        onBlur={() => draft && add(draft)}
      />
    </div>
  );
}

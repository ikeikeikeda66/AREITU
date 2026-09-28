import type { SortMode } from "../api/types";

interface Props {
  value: SortMode;
  onChange: (value: SortMode) => void;
}

export function SortToggle({ value, onChange }: Props) {
  const base = "rounded-md px-3 py-1.5 text-sm font-medium transition-colors";
  const active = "bg-slate-800 text-white";
  const inactive = "bg-slate-100 text-slate-600 hover:bg-slate-200";
  return (
    <div className="flex gap-2" role="group" aria-label="並び替え">
      <button
        type="button"
        className={`${base} ${value === "count" ? active : inactive}`}
        onClick={() => onChange("count")}
      >
        訪問回数
      </button>
      <button
        type="button"
        className={`${base} ${value === "recent" ? active : inactive}`}
        onClick={() => onChange("recent")}
      >
        最近訪問
      </button>
    </div>
  );
}

import { useEffect, useState } from "react";
import { renamePlace, visitsOf } from "../api/tauri";
import type { Place, Visit } from "../api/types";

interface Props {
  place: Place;
  onRenamed: (newPlaceId: number, newName: string) => void;
  onBack: () => void;
}

export function PlaceDetailScreen({ place, onRenamed, onBack }: Props) {
  const [visits, setVisits] = useState<Visit[]>([]);
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(place.name);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDraftName(place.name);
    setEditing(false);
    setError(null);
    visitsOf(place.id).then(setVisits);
  }, [place.id, place.name]);

  async function handleSave() {
    try {
      const newId = await renamePlace(place.id, draftName);
      setError(null);
      setEditing(false);
      onRenamed(newId, draftName.trim());
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="flex flex-col gap-4 p-6">
      <button type="button" onClick={onBack} className="self-start text-sm text-slate-500 hover:text-slate-700">
        一覧に戻る
      </button>

      {editing ? (
        <div className="flex items-center gap-2">
          <label className="sr-only" htmlFor="place-name-input">
            新しい名前
          </label>
          <input
            id="place-name-input"
            aria-label="新しい名前"
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            className="rounded-md border border-slate-300 px-3 py-2"
          />
          <button
            type="button"
            onClick={handleSave}
            className="rounded-md bg-slate-800 px-3 py-2 text-sm font-medium text-white hover:bg-slate-700"
          >
            保存
          </button>
        </div>
      ) : (
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-semibold text-slate-900">{place.name}</h1>
          <button
            type="button"
            onClick={() => setEditing(true)}
            className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
          >
            名前を編集
          </button>
        </div>
      )}

      {error !== null && <p className="text-sm text-red-600">{error}</p>}

      <p className="text-sm text-slate-600">{place.visitCount} 回</p>

      <ul className="flex flex-col gap-2">
        {visits.map((visit) => (
          <li key={visit.startedAt} className="rounded-md border border-slate-200 px-4 py-2 text-sm text-slate-700">
            {visit.startedAt.replace("T", " ")} 〜 {visit.endedAt.slice(11, 16)}
          </li>
        ))}
      </ul>
    </div>
  );
}

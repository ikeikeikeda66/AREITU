import type { Place } from "../api/types";

interface Props {
  place: Place;
  onSelect: (id: number) => void;
}

export function PlaceListItem({ place, onSelect }: Props) {
  return (
    <li>
      <button
        type="button"
        onClick={() => onSelect(place.id)}
        className="flex w-full items-center justify-between rounded-md border border-slate-200 bg-white px-4 py-3 text-left hover:border-slate-400"
      >
        <span className="font-medium text-slate-900">{place.name}</span>
        <span className="text-sm text-slate-500">{place.visitCount} 回訪問</span>
      </button>
    </li>
  );
}

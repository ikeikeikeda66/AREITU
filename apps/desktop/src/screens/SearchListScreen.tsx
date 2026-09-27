import { useEffect, useRef, useState } from "react";
import { listPlaces } from "../api/tauri";
import type { Place, SortMode } from "../api/types";
import { SearchBar } from "../components/SearchBar";
import { SortToggle } from "../components/SortToggle";
import { PlaceListItem } from "../components/PlaceListItem";

interface Props {
  onSelectPlace: (id: number) => void;
}

export function SearchListScreen({ onSelectPlace }: Props) {
  const [keyword, setKeyword] = useState("");
  const [sort, setSort] = useState<SortMode>("count");
  const [places, setPlaces] = useState<Place[]>([]);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  useEffect(() => {
    const id = ++requestId.current;
    listPlaces(sort, keyword)
      .then((result) => {
        if (id === requestId.current) {
          setPlaces(result);
          setError(null);
        }
      })
      .catch((e: unknown) => {
        if (id === requestId.current) {
          setError(String(e));
        }
      });
  }, [sort, keyword]);

  return (
    <div className="flex flex-col gap-4 p-6">
      <div className="flex items-center gap-3">
        <SearchBar value={keyword} onChange={setKeyword} />
        <SortToggle value={sort} onChange={setSort} />
      </div>
      {error !== null && <p className="text-sm text-red-600">{error}</p>}
      {places.length === 0 && error === null ? (
        <p className="text-sm text-slate-500">場所はまだありません</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {places.map((place) => (
            <PlaceListItem key={place.id} place={place} onSelect={onSelectPlace} />
          ))}
        </ul>
      )}
    </div>
  );
}

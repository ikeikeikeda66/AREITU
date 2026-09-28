import { useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";
import { PlaceDetailScreen } from "./screens/PlaceDetailScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import type { Place } from "./api/types";

type View = { kind: "list" } | { kind: "detail"; place: Place } | { kind: "settings" };

export default function App() {
  const [view, setView] = useState<View>({ kind: "list" });

  if (view.kind === "settings") {
    return <SettingsScreen onBack={() => setView({ kind: "list" })} />;
  }

  if (view.kind === "detail") {
    return (
      <PlaceDetailScreen
        place={view.place}
        onBack={() => setView({ kind: "list" })}
        onRenamed={(newPlaceId, newName) =>
          setView({ kind: "detail", place: { ...view.place, id: newPlaceId, name: newName } })
        }
      />
    );
  }

  return (
    <div className="flex flex-col">
      <div className="flex justify-end p-2">
        <button
          type="button"
          onClick={() => setView({ kind: "settings" })}
          className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
        >
          設定
        </button>
      </div>
      <SearchListScreen
        onSelectPlace={(id) => setView({ kind: "detail", place: { id, name: "", visitCount: 0, lastVisit: "" } })}
      />
    </div>
  );
}

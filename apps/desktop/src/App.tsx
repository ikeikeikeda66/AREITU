import { useEffect, useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";
import { PlaceDetailScreen } from "./screens/PlaceDetailScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { OnboardingScreen } from "./screens/OnboardingScreen";
import { setupCompleted } from "./api/tauri";
import type { Place } from "./api/types";

type View = { kind: "list" } | { kind: "detail"; place: Place } | { kind: "settings" };

export default function App() {
  const [needsOnboarding, setNeedsOnboarding] = useState<boolean | null>(null);
  const [view, setView] = useState<View>({ kind: "list" });

  useEffect(() => {
    let cancelled = false;
    setupCompleted()
      .then((completed) => {
        if (!cancelled) setNeedsOnboarding(!completed);
      })
      .catch(() => {
        if (!cancelled) setNeedsOnboarding(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (needsOnboarding === null) {
    return (
      <div role="status" className="flex min-h-screen items-center justify-center bg-slate-100 text-sm text-slate-500">
        読み込み中…
      </div>
    );
  }

  if (needsOnboarding) {
    return <OnboardingScreen onFinish={() => setNeedsOnboarding(false)} />;
  }

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

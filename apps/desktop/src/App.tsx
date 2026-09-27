import { useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";
import { PlaceDetailScreen } from "./screens/PlaceDetailScreen";
import type { Place } from "./api/types";

export default function App() {
  const [selectedPlace, setSelectedPlace] = useState<Place | null>(null);

  if (selectedPlace === null) {
    return (
      <SearchListScreen
        onSelectPlace={(id) => setSelectedPlace({ id, name: "", visitCount: 0, lastVisit: "" })}
      />
    );
  }

  return (
    <PlaceDetailScreen
      place={selectedPlace}
      onBack={() => setSelectedPlace(null)}
      onRenamed={(newPlaceId, newName) =>
        setSelectedPlace({ ...selectedPlace, id: newPlaceId, name: newName })
      }
    />
  );
}

import { useState } from "react";
import { SearchListScreen } from "./screens/SearchListScreen";

export default function App() {
  const [selectedPlaceId, setSelectedPlaceId] = useState<number | null>(null);

  if (selectedPlaceId === null) {
    return <SearchListScreen onSelectPlace={setSelectedPlaceId} />;
  }
  // PlaceDetailScreen は Task 9 で接続する
  return <SearchListScreen onSelectPlace={setSelectedPlaceId} />;
}

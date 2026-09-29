import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PlaceDetailScreen } from "./PlaceDetailScreen";
import * as tauriApi from "../api/tauri";
import type { Place } from "../api/types";

const place: Place = { id: 1, name: "カフェ丸の内", visitCount: 2, lastVisit: "2026-09-01T12:00:00" };

describe("PlaceDetailScreen", () => {
  it("shows name, visit count, and visits newest first", async () => {
    vi.spyOn(tauriApi, "visitsOf").mockResolvedValue([
      { startedAt: "2026-09-08T12:00:00", endedAt: "2026-09-08T12:45:00" },
      { startedAt: "2026-01-01T12:00:00", endedAt: "2026-01-01T12:45:00" },
    ]);
    render(<PlaceDetailScreen place={place} onRenamed={vi.fn()} onBack={vi.fn()} />);
    expect(screen.getByText("カフェ丸の内")).toBeInTheDocument();
    expect(screen.getByText("2 回")).toBeInTheDocument();
    const items = await screen.findAllByRole("listitem");
    expect(items[0].textContent).toContain("2026-09-08");
    expect(items[1].textContent).toContain("2026-01-01");
  });

  it("shows an error and keeps the input when renaming to blank fails", async () => {
    vi.spyOn(tauriApi, "visitsOf").mockResolvedValue([]);
    const renameSpy = vi
      .spyOn(tauriApi, "renamePlace")
      .mockRejectedValue("place name must not be empty");
    render(<PlaceDetailScreen place={place} onRenamed={vi.fn()} onBack={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "名前を編集" }));
    const input = screen.getByLabelText("新しい名前");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(renameSpy).toHaveBeenCalledWith(1, "   "));
    expect(await screen.findByText("place name must not be empty")).toBeInTheDocument();
    expect(screen.getByLabelText("新しい名前")).toHaveValue("   ");
  });

  it("shows an error when loading the visits fails", async () => {
    vi.spyOn(tauriApi, "visitsOf").mockRejectedValue("visits unavailable");
    render(<PlaceDetailScreen place={place} onRenamed={vi.fn()} onBack={vi.fn()} />);
    expect(await screen.findByText(/visits unavailable/)).toBeInTheDocument();
  });
});

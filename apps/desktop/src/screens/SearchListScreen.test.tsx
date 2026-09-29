import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SearchListScreen } from "./SearchListScreen";
import * as tauriApi from "../api/tauri";

describe("SearchListScreen", () => {
  it("loads and shows places on mount", async () => {
    vi.spyOn(tauriApi, "listPlaces").mockResolvedValue([
      { id: 1, name: "カフェ丸の内", visitCount: 3, lastVisit: "2026-09-01T12:00:00" },
    ]);
    render(<SearchListScreen onSelectPlace={vi.fn()} />);
    expect(await screen.findByText("カフェ丸の内")).toBeInTheDocument();
  });

  it("shows an error when loading places fails", async () => {
    vi.spyOn(tauriApi, "listPlaces").mockRejectedValue("places unavailable");
    render(<SearchListScreen onSelectPlace={vi.fn()} />);
    expect(await screen.findByText(/places unavailable/)).toBeInTheDocument();
  });

  it("keeps only the latest search result when requests resolve out of order", async () => {
    let resolveFirst: (places: Awaited<ReturnType<typeof tauriApi.listPlaces>>) => void = () => {};
    let resolveSecond: (places: Awaited<ReturnType<typeof tauriApi.listPlaces>>) => void = () => {};
    const spy = vi.spyOn(tauriApi, "listPlaces");
    spy.mockResolvedValueOnce([]); // 初回マウント時の読み込み
    render(<SearchListScreen onSelectPlace={vi.fn()} />);
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(1));

    spy.mockImplementationOnce(() => new Promise((resolve) => (resolveFirst = resolve)));
    fireEvent.change(screen.getByLabelText("店名で検索"), { target: { value: "コ" } });
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(2));

    spy.mockImplementationOnce(() => new Promise((resolve) => (resolveSecond = resolve)));
    fireEvent.change(screen.getByLabelText("店名で検索"), { target: { value: "コー" } });
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(3));

    // 新しい入力（「コー」）のリクエストより後に、古い入力（「コ」）のリクエストが解決する
    resolveSecond([{ id: 2, name: "コーヒー専門店", visitCount: 1, lastVisit: "2026-09-01T12:00:00" }]);
    await screen.findByText("コーヒー専門店");
    resolveFirst([{ id: 1, name: "コンビニ", visitCount: 5, lastVisit: "2026-09-01T12:00:00" }]);

    await waitFor(() => {
      expect(screen.queryByText("コンビニ")).not.toBeInTheDocument();
      expect(screen.getByText("コーヒー専門店")).toBeInTheDocument();
    });
  });
});

import { useEffect, useState } from "react";
import { getSettings, saveSettings } from "../api/tauri";
import type { KeyStatus, LlmProvider, Settings } from "../api/types";

function keyStatusLabel(status: KeyStatus): string {
  switch (status) {
    case "set":
      return "設定済み";
    case "unavailable":
      return "キーチェーンにアクセスできません";
    default:
      return "未設定";
  }
}

interface Props {
  onBack: () => void;
}

const defaultSettings: Settings = {
  watchedDirs: [],
  llmProvider: "none",
  ollamaUrl: "http://localhost:11434",
  ollamaModel: "",
  openaiModel: "gpt-4o-mini",
  geminiModel: "gemini-1.5-flash",
  googlePlacesEnabled: false,
  minConfidence: 0.6,
  pollIntervalMinutes: 30,
  calendarEnabled: false,
  openaiKeyStatus: "not_set",
  geminiKeyStatus: "not_set",
  googlePlacesKeyStatus: "not_set",
};

export function SettingsScreen({ onBack }: Props) {
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [newDir, setNewDir] = useState("");
  const [openaiKeyInput, setOpenaiKeyInput] = useState("");
  const [geminiKeyInput, setGeminiKeyInput] = useState("");
  const [placesKeyInput, setPlacesKeyInput] = useState("");
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    getSettings().then(setSettings);
  }, []);

  async function handleSave() {
    try {
      await saveSettings({
        watchedDirs: settings.watchedDirs,
        llmProvider: settings.llmProvider,
        ollamaUrl: settings.ollamaUrl,
        ollamaModel: settings.ollamaModel,
        openaiModel: settings.openaiModel,
        geminiModel: settings.geminiModel,
        googlePlacesEnabled: settings.googlePlacesEnabled,
        minConfidence: settings.minConfidence,
        pollIntervalMinutes: settings.pollIntervalMinutes,
        calendarEnabled: settings.calendarEnabled,
        openaiApiKey: openaiKeyInput === "" ? null : openaiKeyInput,
        geminiApiKey: geminiKeyInput === "" ? null : geminiKeyInput,
        googlePlacesApiKey: placesKeyInput === "" ? null : placesKeyInput,
      });
      setStatus("保存しました");
      setOpenaiKeyInput("");
      setGeminiKeyInput("");
      setPlacesKeyInput("");
      getSettings().then(setSettings);
    } catch (e) {
      setStatus(String(e));
    }
  }

  return (
    <div className="flex flex-col gap-6 p-6">
      <button type="button" onClick={onBack} className="self-start text-sm text-slate-500 hover:text-slate-700">
        一覧に戻る
      </button>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">監視フォルダ</h2>
        <ul className="flex flex-col gap-1">
          {settings.watchedDirs.map((dir) => (
            <li key={dir} className="flex items-center justify-between rounded-md border border-slate-200 px-3 py-2">
              <span className="text-sm text-slate-700">{dir}</span>
              <button
                type="button"
                onClick={() =>
                  setSettings({ ...settings, watchedDirs: settings.watchedDirs.filter((d) => d !== dir) })
                }
                className="text-sm text-red-600 hover:underline"
              >
                削除
              </button>
            </li>
          ))}
        </ul>
        <div className="flex gap-2">
          <input
            value={newDir}
            onChange={(e) => setNewDir(e.target.value)}
            placeholder="フォルダのパス"
            aria-label="フォルダのパス"
            className="flex-1 rounded-md border border-slate-300 px-3 py-2"
          />
          <button
            type="button"
            onClick={() => {
              if (newDir.trim() !== "") {
                setSettings({ ...settings, watchedDirs: [...settings.watchedDirs, newDir.trim()] });
                setNewDir("");
              }
            }}
            className="rounded-md bg-slate-800 px-3 py-2 text-sm font-medium text-white hover:bg-slate-700"
          >
            追加
          </button>
        </div>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">店舗名の推論（LLM）</h2>
        <label className="flex flex-col gap-1 text-sm text-slate-700">
          プロバイダ
          <select
            value={settings.llmProvider}
            onChange={(e) => setSettings({ ...settings, llmProvider: e.target.value as LlmProvider })}
            className="rounded-md border border-slate-300 px-3 py-2"
          >
            <option value="none">使わない</option>
            <option value="ollama">Ollama（ローカル）</option>
            <option value="openai">OpenAI</option>
            <option value="gemini">Gemini</option>
          </select>
        </label>

        {settings.llmProvider === "ollama" && (
          <>
            <label className="flex flex-col gap-1 text-sm text-slate-700">
              Ollama URL
              <input
                value={settings.ollamaUrl}
                onChange={(e) => setSettings({ ...settings, ollamaUrl: e.target.value })}
                className="rounded-md border border-slate-300 px-3 py-2"
              />
            </label>
            <label className="flex flex-col gap-1 text-sm text-slate-700">
              モデル名
              <input
                value={settings.ollamaModel}
                onChange={(e) => setSettings({ ...settings, ollamaModel: e.target.value })}
                className="rounded-md border border-slate-300 px-3 py-2"
              />
            </label>
          </>
        )}

        {settings.llmProvider === "openai" && (
          <label className="flex flex-col gap-1 text-sm text-slate-700">
            OpenAI API キー（{keyStatusLabel(settings.openaiKeyStatus)}）
            <input
              type="password"
              value={openaiKeyInput}
              onChange={(e) => setOpenaiKeyInput(e.target.value)}
              aria-label="OpenAI API キー"
              placeholder="変更する場合のみ入力"
              className="rounded-md border border-slate-300 px-3 py-2"
            />
          </label>
        )}

        {settings.llmProvider === "gemini" && (
          <label className="flex flex-col gap-1 text-sm text-slate-700">
            Gemini API キー（{keyStatusLabel(settings.geminiKeyStatus)}）
            <input
              type="password"
              value={geminiKeyInput}
              onChange={(e) => setGeminiKeyInput(e.target.value)}
              aria-label="Gemini API キー"
              placeholder="変更する場合のみ入力"
              className="rounded-md border border-slate-300 px-3 py-2"
            />
          </label>
        )}
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">場所の検索</h2>
        <label className="flex items-center gap-2 text-sm text-slate-700">
          <input
            type="checkbox"
            checked={settings.googlePlacesEnabled}
            onChange={(e) => setSettings({ ...settings, googlePlacesEnabled: e.target.checked })}
          />
          Google Places API を優先的に使う
        </label>
        <label className="flex flex-col gap-1 text-sm text-slate-700">
          Google Places API キー（{keyStatusLabel(settings.googlePlacesKeyStatus)}）
          <input
            type="password"
            value={placesKeyInput}
            onChange={(e) => setPlacesKeyInput(e.target.value)}
            aria-label="Google Places API キー"
            placeholder="変更する場合のみ入力"
            className="rounded-md border border-slate-300 px-3 py-2"
          />
        </label>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">同期</h2>
        <label className="flex flex-col gap-1 text-sm text-slate-700">
          ポーリング間隔（分）
          <input
            type="number"
            min={1}
            value={settings.pollIntervalMinutes}
            onChange={(e) => setSettings({ ...settings, pollIntervalMinutes: Number(e.target.value) })}
            className="w-32 rounded-md border border-slate-300 px-3 py-2"
          />
        </label>
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">Google アカウント</h2>
        <p className="text-sm text-slate-500">Google Drive 連携は今後のバージョンで対応予定です。</p>
      </section>

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={handleSave}
          className="rounded-md bg-slate-800 px-4 py-2 text-sm font-medium text-white hover:bg-slate-700"
        >
          保存
        </button>
        {status !== null && <span className="text-sm text-slate-600">{status}</span>}
      </div>
    </div>
  );
}

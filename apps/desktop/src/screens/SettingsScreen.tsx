import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  driveSyncNow,
  getSettings,
  googleSignIn,
  googleSignOut,
  googleStatus,
  importTimelineFile,
  saveSettings,
} from "../api/tauri";
import type { GoogleStatus, KeyStatus, LlmProvider, Settings } from "../api/types";

// areitu-google の Error::CalendarNotAuthorized のメッセージ先頭と一致させる。
const CALENDAR_NOT_AUTHORIZED_PREFIX = "calendar not authorized";

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
  calendarLastError: null,
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
  const [googleAccountStatus, setGoogleAccountStatus] = useState<GoogleStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  // getSettings が成功するまで保存させない（既定値で実際の config.json を上書きしないため）。
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [googleStatusFailed, setGoogleStatusFailed] = useState(false);
  const [googleBusy, setGoogleBusy] = useState(false);
  const [googleError, setGoogleError] = useState<string | null>(null);
  const [driveSyncResult, setDriveSyncResult] = useState<string | null>(null);
  const [timelineImporting, setTimelineImporting] = useState(false);
  const [timelineImportResult, setTimelineImportResult] = useState<string | null>(null);
  const [timelineImportError, setTimelineImportError] = useState<string | null>(null);

  useEffect(() => {
    getSettings()
      .then((loaded) => {
        setSettings(loaded);
        setSettingsLoaded(true);
      })
      .catch((e: unknown) => setLoadError(`設定を読み込めませんでした: ${String(e)}`));
    googleStatus()
      .then(setGoogleAccountStatus)
      .catch((e: unknown) => {
        setGoogleStatusFailed(true);
        setGoogleError(String(e));
      });
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
      getSettings()
        .then(setSettings)
        .catch((e: unknown) => setLoadError(`設定を読み込めませんでした: ${String(e)}`));
    } catch (e) {
      setStatus(String(e));
    }
  }

  async function handleGoogleSignIn() {
    setGoogleBusy(true);
    setGoogleError(null);
    try {
      await googleSignIn(settings.calendarEnabled);
      setGoogleAccountStatus(await googleStatus());
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      setGoogleBusy(false);
    }
  }

  async function handleGoogleSignOut() {
    setGoogleBusy(true);
    setGoogleError(null);
    try {
      await googleSignOut();
      setGoogleAccountStatus(await googleStatus());
      setDriveSyncResult(null);
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      setGoogleBusy(false);
    }
  }

  async function handleDriveSyncNow() {
    setGoogleBusy(true);
    setGoogleError(null);
    try {
      setDriveSyncResult(await driveSyncNow());
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      setGoogleBusy(false);
    }
  }

  async function handleImportTimeline() {
    setTimelineImportError(null);
    setTimelineImporting(true);
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Google Timeline / Takeout JSON", extensions: ["json"] }],
      });
      if (selected === null || Array.isArray(selected)) {
        return;
      }
      const count = await importTimelineFile(selected);
      setTimelineImportResult(`${count} 件の訪問を取り込みました`);
    } catch (e) {
      setTimelineImportError(String(e));
    } finally {
      setTimelineImporting(false);
    }
  }

  return (
    <div className="flex flex-col gap-6 p-6">
      <button type="button" onClick={onBack} className="self-start text-sm text-slate-500 hover:text-slate-700">
        一覧に戻る
      </button>

      {loadError !== null && <p className="text-sm text-red-600">{loadError}</p>}

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
        <h2 className="text-lg font-semibold text-slate-900">カレンダー連携</h2>
        <label className="flex items-center gap-2 text-sm text-slate-700">
          <input
            type="checkbox"
            checked={settings.calendarEnabled}
            onChange={(e) => setSettings({ ...settings, calendarEnabled: e.target.checked })}
          />
          Google カレンダーを自動で取り込む
        </label>
        {settings.calendarLastError !== null && (
          <div className="flex flex-col gap-1 rounded-md border border-red-200 bg-red-50 px-3 py-2">
            <p className="text-sm font-medium text-red-700">直近のカレンダー同期に失敗しました</p>
            <p className="text-sm text-red-700">{settings.calendarLastError}</p>
            {settings.calendarLastError.startsWith(CALENDAR_NOT_AUTHORIZED_PREFIX) && (
              <p className="text-sm text-slate-700">
                カレンダーへのアクセスが許可されていません。「カレンダーへのアクセスを許可」から Google で再度許可してください。
              </p>
            )}
          </div>
        )}
        {settings.calendarEnabled && googleAccountStatus === "signed_in" && (
          <>
            <p className="text-sm text-slate-500">
              初めて有効にした場合は、カレンダーへのアクセスを Google で許可する必要があります。
            </p>
            <button
              type="button"
              onClick={handleGoogleSignIn}
              disabled={googleBusy}
              className="self-start rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-50"
            >
              カレンダーへのアクセスを許可
            </button>
          </>
        )}
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">Google アカウント</h2>
        <p className="text-sm text-slate-700">
          {googleAccountStatus === "signed_in"
            ? "サインイン済み"
            : googleAccountStatus === "signed_out"
              ? "未サインイン"
              : googleStatusFailed
                ? "状態を確認できません"
                : "状態を確認しています…"}
        </p>
        <div className="flex items-center gap-3">
          {googleAccountStatus === "signed_in" ? (
            <>
              <button
                type="button"
                onClick={handleGoogleSignOut}
                disabled={googleBusy}
                className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-50"
              >
                サインアウト
              </button>
              <button
                type="button"
                onClick={handleDriveSyncNow}
                disabled={googleBusy}
                className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-50"
              >
                今すぐ Drive 同期
              </button>
            </>
          ) : (
            <button
              type="button"
              onClick={handleGoogleSignIn}
              disabled={googleBusy}
              className="rounded-md bg-slate-800 px-3 py-1.5 text-sm font-medium text-white hover:bg-slate-700 disabled:opacity-50"
            >
              Google でサインイン
            </button>
          )}
          {googleBusy && <span className="text-sm text-slate-500">処理中…</span>}
        </div>
        {driveSyncResult !== null && <p className="text-sm text-slate-600">{driveSyncResult}</p>}
        {googleError !== null && <p className="text-sm text-red-600">{googleError}</p>}
      </section>

      <section className="flex flex-col gap-2">
        <h2 className="text-lg font-semibold text-slate-900">データの取り込み</h2>
        <p className="text-sm text-slate-500">
          Google タイムラインのエクスポート（Timeline.json、または Google Takeout の位置情報履歴）を読み込みます。
        </p>
        <button
          type="button"
          onClick={handleImportTimeline}
          disabled={timelineImporting}
          className="self-start rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-50"
        >
          Google タイムラインを取り込む
        </button>
        {timelineImportResult !== null && <p className="text-sm text-slate-600">{timelineImportResult}</p>}
        {timelineImportError !== null && <p className="text-sm text-red-600">{timelineImportError}</p>}
      </section>

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={handleSave}
          disabled={!settingsLoaded}
          className="rounded-md bg-slate-800 px-4 py-2 text-sm font-medium text-white hover:bg-slate-700 disabled:opacity-50"
        >
          保存
        </button>
        {!settingsLoaded && loadError !== null && (
          <span className="text-sm text-red-600">設定を読み込めていないため保存できません</span>
        )}
        {status !== null && <span className="text-sm text-slate-600">{status}</span>}
      </div>
    </div>
  );
}

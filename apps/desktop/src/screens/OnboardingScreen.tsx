import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { googleSignIn, saveSettings } from "../api/tauri";
import type { LlmProvider } from "../api/types";

interface Props {
  onFinish: () => void;
}

export function OnboardingScreen({ onFinish }: Props) {
  const [watchedDirs, setWatchedDirs] = useState<string[]>([]);
  const [llmProvider, setLlmProvider] = useState<LlmProvider>("none");
  const [calendarEnabled, setCalendarEnabled] = useState(false);
  const [googleSignedIn, setGoogleSignedIn] = useState(false);
  const [googleBusy, setGoogleBusy] = useState(false);
  const [googleError, setGoogleError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handlePickFolder() {
    try {
      const selected = await open({ directory: true, multiple: true });
      if (selected === null) {
        return;
      }
      const picked = Array.isArray(selected) ? selected : [selected];
      setWatchedDirs((prev) => Array.from(new Set([...prev, ...picked])));
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleGoogleSignIn() {
    setGoogleBusy(true);
    setGoogleError(null);
    try {
      await googleSignIn(calendarEnabled);
      setGoogleSignedIn(true);
    } catch (e) {
      setGoogleError(String(e));
    } finally {
      setGoogleBusy(false);
    }
  }

  async function finishWith(dirs: string[], provider: LlmProvider, calendar: boolean) {
    setBusy(true);
    setError(null);
    try {
      await saveSettings({
        watchedDirs: dirs,
        llmProvider: provider,
        ollamaUrl: "http://localhost:11434",
        ollamaModel: "",
        openaiModel: "gpt-4o-mini",
        geminiModel: "gemini-1.5-flash",
        googlePlacesEnabled: false,
        minConfidence: 0.6,
        pollIntervalMinutes: 30,
        calendarEnabled: calendar,
        openaiApiKey: null,
        geminiApiKey: null,
        googlePlacesApiKey: null,
      });
      onFinish();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="min-h-screen bg-slate-100">
      <div className="mx-auto flex max-w-xl flex-col gap-6 p-8">
        <h1 className="text-xl font-semibold text-slate-900">AREITU へようこそ</h1>
        <p className="text-sm text-slate-600">
          写真の保存フォルダを選ぶだけで、いつどこに何回行ったかを検索できるようになります。Google 連携と店舗名の推論方法は後から設定画面でいつでも変更できます。
        </p>

        <section className="flex flex-col gap-2 rounded-lg bg-white p-4 shadow-sm">
          <h2 className="text-lg font-semibold text-slate-900">監視フォルダ</h2>
          <ul className="flex flex-col gap-1">
            {watchedDirs.map((dir) => (
              <li key={dir} className="rounded-md border border-slate-200 px-3 py-2 text-sm text-slate-700">
                {dir}
              </li>
            ))}
          </ul>
          <button
            type="button"
            onClick={handlePickFolder}
            className="self-start rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
          >
            フォルダを選択
          </button>
        </section>

        <section className="flex flex-col gap-2 rounded-lg bg-white p-4 shadow-sm">
          <h2 className="text-lg font-semibold text-slate-900">Google 連携（任意）</h2>
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={handleGoogleSignIn}
              disabled={googleBusy}
              className="rounded-md border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-50"
            >
              Google でサインイン
            </button>
            {googleBusy && <span className="text-sm text-slate-500">処理中…</span>}
          </div>
          {googleSignedIn && <p className="text-sm text-slate-600">サインインしました</p>}
          {googleError !== null && <p className="text-sm text-red-600">{googleError}</p>}
          <label className="flex items-center gap-2 text-sm text-slate-700">
            <input type="checkbox" checked={calendarEnabled} onChange={(e) => setCalendarEnabled(e.target.checked)} />
            Google カレンダーを自動で取り込む
          </label>
        </section>

        <section className="flex flex-col gap-2 rounded-lg bg-white p-4 shadow-sm">
          <h2 className="text-lg font-semibold text-slate-900">店舗名の推論（任意）</h2>
          <label className="flex flex-col gap-1 text-sm text-slate-700">
            プロバイダ
            <select
              value={llmProvider}
              onChange={(e) => setLlmProvider(e.target.value as LlmProvider)}
              className="rounded-md border border-slate-300 px-3 py-2"
            >
              <option value="none">使わない</option>
              <option value="ollama">Ollama（ローカル）</option>
              <option value="openai">OpenAI</option>
              <option value="gemini">Gemini</option>
            </select>
          </label>
          <p className="text-sm text-slate-500">API キーは設定画面から後で登録できます。</p>
        </section>

        {error !== null && <p className="text-sm text-red-600">{error}</p>}

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => finishWith(watchedDirs, llmProvider, calendarEnabled)}
            disabled={busy}
            className="rounded-md bg-slate-800 px-4 py-2 text-sm font-medium text-white hover:bg-slate-700 disabled:opacity-50"
          >
            はじめる
          </button>
          <button
            type="button"
            onClick={() => finishWith([], "none", false)}
            disabled={busy}
            className="text-sm text-slate-500 hover:text-slate-700 disabled:opacity-50"
          >
            スキップ
          </button>
        </div>
      </div>
    </div>
  );
}

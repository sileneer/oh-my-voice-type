import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { HistoryEntry, Settings } from "./api";

const now = Date.now();
export const mockSettings: Settings = {
  schema_version: 3,
  asr_profiles: [{
    id: "asr-demo", label: "OpenAI Whisper", engine: "openai_compatible", preset: "openai",
    base_url: "https://api.openai.com/v1", model: "gpt-transcribe",
    extra_options: [["response_format", "json"]], credential_id: "cred-asr", credential_hint: "1234", allow_insecure_http: false,
  }],
  llm_profiles: [{
    id: "llm-demo", label: "DeepSeek", engine: "custom", format: "open_ai_chat", preset: "deepseek",
    base_url: "https://api.deepseek.com/v1", model: "deepseek-flash",
    credential_id: "cred-llm", credential_hint: "5678", allow_insecure_http: false,
  }],
  active_asr_profile_id: "asr-demo",
  active_llm_profile_id: "llm-demo",
  cleanup: { max_tokens: 1024, temperature: 0 },
  hotkey: { dictate: "right-alt", smart_gestures: true, dictate_alt: "right-opt", translate: "f8", edit_selection: "f9" },
  dictation: {
    language: null, cleanup_enabled: true, preserve_wording: true, no_list_formatting: false,
    app_tones: [{ app: "Slack", style: " concise and friendly" }], translate_target: "English",
    default_mode: "dictate", default_tone: "Clear and natural",
  },
  context: { include_app_context: true, include_caret_context: false, max_caret_chars: 300 },
  history: { retention_days: 30, keep_audio: false, pending_retention_days: 7 },
  audio: { mic_device: null, mute_output_while_dictating: false, prewarm: true, max_recording_seconds: 540 },
  ui: { locale: "en", theme: "system", onboarding_completed: true, close_to_tray: true, check_for_updates: true },
};

const sampleTexts = [
  "Ship the release notes before the standup and keep the summary concise.",
  "The standby power consumption is 1.2 watts, so the battery estimate needs to be recalculated.",
  "Can you review the migration plan and flag any compatibility risks?",
  "今天的会议改到下午三点，请同步更新日历邀请。",
];
const history: HistoryEntry[] = Array.from({ length: 55 }, (_, index) => ({
  id: `history-${index}`,
  created_at_ms: now - index * 3_600_000,
  mode: index % 5 === 0 ? "translate" : index % 7 === 0 ? "edit_selection" : "dictate",
  app_name: index % 2 ? "Visual Studio Code" : "Slack",
  raw_text: sampleTexts[index % sampleTexts.length],
  final_text: sampleTexts[index % sampleTexts.length],
  asr_ms: 280 + index,
  cleanup_ms: index % 3 ? 420 + index : null,
  insert_ok: index % 9 !== 0,
  pinned: index === 2 || index === 8,
  target_language: index % 5 === 0 ? "English" : null,
  delivery_status: index % 9 === 0 ? "copied" : "inserted",
  warnings: index % 9 === 0 ? ["Clipboard fallback used"] : [],
}));

let activeMockSession: string | null = null;
const cancelledSessions = new Set<string>();

export function installTauriMock() {
  const params = new URLSearchParams(window.location.search);
  const state = structuredClone(mockSettings);
  if (params.get("locale") === "zh-CN") state.ui.locale = "zh-CN";
  const themeParam = params.get("theme");
  if (themeParam === "dark" || themeParam === "light") state.ui.theme = themeParam;
  if (params.get("onboarding") === "1") state.ui.onboarding_completed = false;
  const emptyMock = params.get("empty") === "1";
  if (emptyMock) {
    state.asr_profiles = [];
    state.llm_profiles = [];
    state.active_asr_profile_id = null;
    state.active_llm_profile_id = null;
  }
  const activeHistory = emptyMock ? [] : history;
  const pending = emptyMock ? [] : [
    { id: "pending-1", created_at_ms: now - 900_000, mode: "translate", target_language: "English", stage: "interrupted", attempts: 0, raw_text: "请把这段文字翻译成英文。", final_text: "", error: "Interrupted by restart", has_audio: true, delivery_uncertain: false },
    { id: "pending-2", created_at_ms: now - 1_800_000, mode: "dictate", target_language: null, stage: "needs_review", attempts: 1, raw_text: "Check the deployment status.", final_text: "Check the deployment status.", error: null, has_audio: false, delivery_uncertain: true },
  ];
  const readinessFor = (role: "asr" | "llm") => {
    const activeId = role === "asr" ? state.active_asr_profile_id : state.active_llm_profile_id;
    const list = role === "asr" ? state.asr_profiles : state.llm_profiles;
    const active = list.find((item) => item.id === activeId);
    if (!active) return { state: "missing_profile", profile_id: null, credential_id: null, message: null };
    if (!active.credential_id) return { state: "missing_key", profile_id: active.id, credential_id: null, message: null };
    return { state: "ready", profile_id: active.id, credential_id: active.credential_id, message: null };
  };
  mockWindows("main", "pill");
  mockIPC((command, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    switch (command) {
      case "get_settings": return structuredClone(state);
      case "set_settings": {
        Object.assign(state, structuredClone(args.settings));
        return structuredClone(state);
      }
      case "permission_status": return { platform: "windows", microphone: "granted_at_first_capture", accessibility: "not_required" };
      case "api_status": return {
        asr: readinessFor("asr"),
        llm: readinessFor("llm"),
      };
      case "list_mics": return ["Default microphone", "Studio Microphone"];
      case "history_list": {
        const limit = Number(args.limit ?? 50);
        const offset = Number(args.offset ?? 0);
        const query = typeof args.query === "string" ? args.query.toLowerCase() : "";
        const mode = typeof args.mode === "string" ? args.mode : null;
        const delivery = typeof args.delivery === "string" ? args.delivery : null;
        const pinned = args.pinned === true;
        // Read the camelCase keys the real Tauri bridge expects, so a
        // snake_case regression fails here instead of only in the app.
        const from = typeof args.fromMs === "number" ? args.fromMs : 0;
        const to = typeof args.toMs === "number" ? args.toMs : Number.MAX_SAFE_INTEGER;
        const filtered = activeHistory.filter((entry) =>
          (!query || entry.final_text.toLowerCase().includes(query) || entry.raw_text.toLowerCase().includes(query)) &&
          (!mode || entry.mode === mode) && (!delivery || entry.delivery_status === delivery) &&
          (!pinned || entry.pinned) && entry.created_at_ms >= from && entry.created_at_ms <= to,
        ).sort((a, b) => (args.newestFirst === true ? 0 : Number(b.pinned) - Number(a.pinned)) || b.created_at_ms - a.created_at_ms);
        return { entries: structuredClone(filtered.slice(offset, offset + limit)), total: filtered.length };
      }
      case "history_delete": activeHistory.splice(activeHistory.findIndex((entry) => entry.id === args.id), 1); return undefined;
      case "history_delete_all": activeHistory.splice(0); return undefined;
      case "history_set_pinned": {
        const entry = activeHistory.find((item) => item.id === args.id);
        if (entry) entry.pinned = Boolean(args.pinned);
        return undefined;
      }
      case "history_reprocess": {
        const entry = activeHistory.find((item) => item.id === args.id);
        if (entry) entry.final_text = `${entry.final_text} (reprocessed)`;
        return entry ? structuredClone(entry) : null;
      }
      case "history_export_audio": return false;
      case "copy_text": return undefined;
      case "pending_list": return structuredClone(pending);
      case "pending_retry":
      case "pending_copy": return undefined;
      // Like the backend, deletes emit `pending:changed` so every count updates.
      case "pending_discard": {
        const index = pending.findIndex((task) => task.id === args.id);
        if (index >= 0) pending.splice(index, 1);
        void emit("pending:changed", { count: pending.length });
        return undefined;
      }
      case "pending_cleanup": {
        const deleted = pending.splice(0).length;
        void emit("pending:changed", { count: 0 });
        return { deleted_count: deleted, freed_bytes: deleted * 512 };
      }
      case "pill_hide": return undefined;
      case "dictionary_list": return [{ term: "Qoder" }, { term: "WebView2" }, { term: "standby power" }];
      case "dictionary_add": return [{ term: String(args.term) }];
      case "dictionary_rename":
        if (typeof args.oldTerm !== "string" || typeof args.newTerm !== "string") throw new Error("command dictionary_rename missing required key oldTerm/newTerm");
        return [{ term: args.newTerm }];
      case "dictionary_remove": return [];
      case "updater_status": return { status: "idle", current_version: "0.1.0", update: null, progress: null, message: null, checked_at_ms: null };
      case "updater_check": return params.get("update") === "1"
        ? { status: "available", current_version: "0.1.0", update: { version: "0.2.0", notes: "## What's Changed\n- Faster dictation start\n- Signed in-app updates", pub_date: new Date().toISOString() }, progress: null, message: null, checked_at_ms: Date.now() }
        : { status: "up-to-date", current_version: "0.1.0", update: null, progress: null, message: null, checked_at_ms: Date.now() };
      case "updater_install": return undefined;
      case "updater_restart": return undefined;
      case "session_start": {
        const request = args.request as { mode?: string; target_language?: string };
        const sessionId = `session-${Date.now()}`;
        activeMockSession = sessionId;
        cancelledSessions.delete(sessionId);
        void (async () => {
          if (cancelledSessions.has(sessionId)) return;
          await emit("session:state", { session_id: sessionId, mode: request.mode, state: "starting" });
          if (cancelledSessions.has(sessionId)) return;
          await emit("session:state", { session_id: sessionId, mode: request.mode, state: "recording" });
          await new Promise((resolve) => setTimeout(resolve, 900));
          if (cancelledSessions.has(sessionId)) return;
          await emit("session:state", { session_id: sessionId, mode: request.mode, state: "processing" });
          if (cancelledSessions.has(sessionId)) return;
          await emit("session:outcome", { id: `outcome-${Date.now()}`, raw_text: "Ship the release notes.", final_text: "Ship the release notes before standup.", mode: request.mode, timings: { asr_ms: 280, cleanup_ms: 410, total_ms: 980 }, inserted: true, warnings: [] });
          await emit("session:state", { session_id: sessionId, mode: request.mode, state: "idle" });
          activeMockSession = null;
        })();
        return { session_id: sessionId };
      }
      case "dictation_toggle": return undefined;
      case "dictation_cancel":
        if (activeMockSession) {
          cancelledSessions.add(activeMockSession);
          void emit("session:state", { session_id: activeMockSession, state: "idle" });
          activeMockSession = null;
        }
        return undefined;
      case "save_asr_profile": {
        const incoming = structuredClone(args.profile) as Settings["asr_profiles"][number];
        if (!incoming.id) incoming.id = `asr-${Date.now()}`;
        if (typeof args.apiKey === "string" && args.apiKey.trim()) {
          incoming.credential_id = `cred-${Date.now()}`;
          incoming.credential_hint = args.apiKey.trim().slice(-4);
        }
        const index = state.asr_profiles.findIndex((item) => item.id === incoming.id);
        if (index >= 0) state.asr_profiles[index] = incoming;
        else state.asr_profiles.push(incoming);
        if (!state.active_asr_profile_id) state.active_asr_profile_id = incoming.id;
        return structuredClone(state);
      }
      case "save_llm_profile": {
        const incoming = structuredClone(args.profile) as Settings["llm_profiles"][number];
        if (!incoming.id) incoming.id = `llm-${Date.now()}`;
        if (typeof args.apiKey === "string" && args.apiKey.trim()) {
          incoming.credential_id = `cred-${Date.now()}`;
          incoming.credential_hint = args.apiKey.trim().slice(-4);
        }
        const index = state.llm_profiles.findIndex((item) => item.id === incoming.id);
        if (index >= 0) state.llm_profiles[index] = incoming;
        else state.llm_profiles.push(incoming);
        if (!state.active_llm_profile_id) state.active_llm_profile_id = incoming.id;
        return structuredClone(state);
      }
      case "delete_profile": {
        const role = args.role as "asr" | "llm";
        const id = String(args.id ?? "");
        if (role === "asr") {
          state.asr_profiles = state.asr_profiles.filter((item) => item.id !== id);
          if (state.active_asr_profile_id === id) state.active_asr_profile_id = null;
        } else {
          state.llm_profiles = state.llm_profiles.filter((item) => item.id !== id);
          if (state.active_llm_profile_id === id) state.active_llm_profile_id = null;
        }
        return structuredClone(state);
      }
      case "set_active_profile": {
        const role = args.role as "asr" | "llm";
        const id = String(args.id ?? "");
        if (role === "asr") state.active_asr_profile_id = id;
        else state.active_llm_profile_id = id;
        return structuredClone(state);
      }
      case "test_profile":
        return args.mode === "check"
          ? { ok: true, verified: false, message: "endpoint reachable (HTTP 200); key not verified", elapsed_ms: 85 }
          : { ok: true, verified: true, message: "Mock provider responded", elapsed_ms: 180 };
      case "list_models": {
        const req = (args.args ?? {}) as {
          role?: "asr" | "llm";
          protocol?: "open_ai" | "anthropic";
          base_url?: string | null;
          api_key?: string | null;
        };
        if (req.api_key === "invalid-key") {
          throw new Error("models: authentication failed — check the API key");
        }
        const ok = (models: Array<{ id: string; label?: string }>) => ({ outcome: "ok" as const, models });
        const url = (req.base_url ?? "").toLowerCase();
        if (req.protocol === "anthropic" || url.includes("anthropic")) {
          return ok([
            { id: "claude-haiku-5-5", label: "Claude Haiku 5.5" },
            { id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5" },
            { id: "claude-opus-5-5", label: "Claude Opus 5.5" },
          ]);
        }
        if (req.role === "asr") {
          if (url.includes("groq")) return ok([{ id: "whisper-large-v3-turbo" }, { id: "whisper-large-v3" }]);
          if (url.includes("mistral")) return ok([{ id: "voxtral-mini-transcribe-3" }, { id: "voxtral-small-transcribe-3" }]);
          if (url.includes("siliconflow")) return ok([{ id: "FunAudioLLM/SenseVoiceSmall" }, { id: "TeleAI/TeleSpeechASR" }]);
          if (url.includes("stepfun")) return ok([{ id: "stepaudio-2.5-asr" }, { id: "step-asr" }]);
          if (url.includes("unisound")) return { outcome: "no_matching_models" as const, models: [] };
          if (url.includes("bigmodel") || url.includes("z.ai")) return ok([{ id: "glm-asr-2512" }, { id: "glm-asr" }]);
          if (url.includes("openrouter")) return ok([{ id: "openai/whisper-large-v3-turbo" }, { id: "openai/whisper-large-v3" }, { id: "openai/whisper-1" }]);
          return ok([
            { id: "gpt-transcribe" },
            { id: "gpt-4o-mini-transcribe" },
            { id: "gpt-4o-transcribe" },
            { id: "whisper-1" },
          ]);
        }
        if (url.includes("minimax")) return ok([{ id: "MiniMax-M3.1-Flash" }, { id: "MiniMax-M3" }]);
        if (url.includes("deepseek")) return ok([{ id: "deepseek-flash" }, { id: "deepseek-v4-pro" }, { id: "deepseek-chat" }]);
        if (url.includes("googleapis")) return ok([{ id: "gemini-3.1-flash-lite" }, { id: "gemini-3-flash" }, { id: "gemini-2.5-flash" }]);
        if (url.includes("x.ai")) return ok([{ id: "grok-4.3" }, { id: "grok-4.7" }]);
        if (url.includes("moonshot")) return ok([{ id: "kimi-k2.5" }, { id: "kimi-k2.7-code" }]);
        if (url.includes("dashscope")) return ok([{ id: "qwen-flash" }, { id: "qwen-plus" }, { id: "qwen-max" }]);
        if (url.includes("groq")) return ok([{ id: "openai/gpt-oss-20b" }, { id: "llama-3.1-8b-instant" }, { id: "llama-3.3-70b-versatile" }]);
        if (url.includes("unisound")) return ok([{ id: "u2-flash" }, { id: "u2" }, { id: "u2-med" }, { id: "u2-radimed" }]);
        return ok([
          { id: "gpt-5.4-mini" },
          { id: "gpt-5.4-nano" },
          { id: "gpt-5.4" },
          { id: "gpt-4.1-mini" },
        ]);
      }
      case "export_config": return true;
      case "import_config": return structuredClone(state);
      case "open_settings_folder": return undefined;
      case "open_external_url": return undefined;
      default: return undefined;
    }
  }, { shouldMockEvents: true });

  if (params.get("view") === "pill") {
    void (async () => {
      await new Promise((resolve) => setTimeout(resolve, 250));
      await emit("session:state", { session_id: "pill-demo", mode: "dictate", state: "starting" });
      await emit("session:state", { session_id: "pill-demo", mode: "dictate", state: "recording" });
      await new Promise((resolve) => setTimeout(resolve, 1800));
      await emit("session:state", { session_id: "pill-demo", mode: "dictate", state: "processing" });
      await emit("session:outcome", { id: "pill-outcome", raw_text: "Hello", final_text: "Hello from the floating pill.", mode: "dictate", timings: { asr_ms: 200, cleanup_ms: 300, total_ms: 600 }, inserted: true, warnings: [] });
      await emit("session:state", { session_id: "pill-demo", mode: "dictate", state: "idle" });
    })();
  }
}

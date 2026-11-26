import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { HistoryEntry, Settings } from "./api";

const now = Date.now();
export const mockSettings: Settings = {
  schema_version: 2,
  asr_profiles: [{
    id: "asr-demo", label: "Local Whisper", engine: "openai_compatible",
    base_url: "http://127.0.0.1:8080/v1", model: "whisper-large-v3-turbo",
    extra_options: [], credential_id: "cred-asr", allow_insecure_http: true,
  }],
  llm_profiles: [{
    id: "llm-demo", label: "Local LLM", engine: "custom", format: "open_ai_chat",
    base_url: "http://127.0.0.1:8080/v1", model: "qwen2.5-7b-instruct",
    credential_id: "cred-llm", allow_insecure_http: true,
  }],
  active_asr_profile_id: "asr-demo",
  active_llm_profile_id: "llm-demo",
  credentials: [
    { id: "cred-asr", label: "Whisper key", role: "asr", hint: "1234", created_at_ms: now, updated_at_ms: now },
    { id: "cred-llm", label: "LLM key", role: "llm", hint: "5678", created_at_ms: now, updated_at_ms: now },
  ],
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
  ui: { locale: "en", theme: "system", onboarding_completed: true, close_to_tray: true },
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
  if (params.get("locale") === "zh-CN") mockSettings.ui.locale = "zh-CN";
  if (params.get("theme") === "dark") mockSettings.ui.theme = "dark";
  if (params.get("onboarding") === "1") mockSettings.ui.onboarding_completed = false;
  mockWindows("main", "pill");
  mockIPC((command, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    switch (command) {
      case "get_settings": return structuredClone(mockSettings);
      case "set_settings": return args.settings;
      case "permission_status": return { platform: "windows", microphone: "granted_at_first_capture", accessibility: "not_required" };
      case "api_status": return {
        asr: { state: "ready", profile_id: "asr-demo", credential_id: "cred-asr", message: null },
        llm: { state: "ready", profile_id: "llm-demo", credential_id: "cred-llm", message: null },
      };
      case "list_mics": return ["Default microphone", "Studio Microphone"];
      case "history_list": {
        const limit = Number(args.limit ?? 50);
        const offset = Number(args.offset ?? 0);
        const query = typeof args.query === "string" ? args.query.toLowerCase() : "";
        const mode = typeof args.mode === "string" ? args.mode : null;
        const delivery = typeof args.delivery === "string" ? args.delivery : null;
        const pinned = args.pinned === true;
        const from = typeof args.from_ms === "number" ? args.from_ms : 0;
        const to = typeof args.to_ms === "number" ? args.to_ms : Number.MAX_SAFE_INTEGER;
        const filtered = history.filter((entry) =>
          (!query || entry.final_text.toLowerCase().includes(query) || entry.raw_text.toLowerCase().includes(query)) &&
          (!mode || entry.mode === mode) && (!delivery || entry.delivery_status === delivery) &&
          (!pinned || entry.pinned) && entry.created_at_ms >= from && entry.created_at_ms <= to,
        ).sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.created_at_ms - a.created_at_ms);
        return { entries: structuredClone(filtered.slice(offset, offset + limit)), total: filtered.length };
      }
      case "history_delete": history.splice(history.findIndex((entry) => entry.id === args.id), 1); return undefined;
      case "history_delete_all": history.splice(0); return undefined;
      case "history_set_pinned": {
        const entry = history.find((item) => item.id === args.id);
        if (entry) entry.pinned = Boolean(args.pinned);
        return undefined;
      }
      case "history_reprocess": {
        const entry = history.find((item) => item.id === args.id);
        if (entry) entry.final_text = `${entry.final_text} (reprocessed)`;
        return entry ? structuredClone(entry) : null;
      }
      case "history_export_audio": return false;
      case "copy_text": return undefined;
      case "pending_list": return [
        { id: "pending-1", created_at_ms: now - 900_000, mode: "translate", target_language: "English", stage: "interrupted", attempts: 0, raw_text: "请把这段文字翻译成英文。", final_text: "", error: "Interrupted by restart", has_audio: true, delivery_uncertain: false },
        { id: "pending-2", created_at_ms: now - 1_800_000, mode: "dictate", target_language: null, stage: "needs_review", attempts: 1, raw_text: "Check the deployment status.", final_text: "Check the deployment status.", error: null, has_audio: false, delivery_uncertain: true },
      ];
      case "pending_retry":
      case "pending_copy":
      case "pending_discard":
      case "pending_cleanup": return command === "pending_cleanup" ? { deleted_count: 2, freed_bytes: 1024 } : undefined;
      case "dictionary_list": return [{ term: "Qoder" }, { term: "WebView2" }, { term: "standby power" }];
      case "dictionary_add": return [{ term: String(args.term) }];
      case "dictionary_rename": return [{ term: String(args.new_term) }];
      case "dictionary_remove": return [];
      case "updater_status": return { status: "unconfigured", current_version: "0.1.0", update: null, progress: null, message: null };
      case "updater_check": return { status: "unconfigured", current_version: "0.1.0", update: null, progress: null, message: null };
      case "updater_install": return undefined;
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
      case "save_asr_profile":
      case "save_llm_profile":
      case "create_credential":
      case "rename_credential":
      case "replace_credential":
      case "delete_profile":
      case "set_active_profile":
      case "delete_credential": return structuredClone(mockSettings);
      case "test_profile": return { ok: true, verified: true, message: "Mock provider responded", elapsed_ms: 180 };
      case "export_config": return true;
      case "import_config": return structuredClone(mockSettings);
      case "cleanup_legacy_keys": return [];
      case "open_settings_folder": return undefined;
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

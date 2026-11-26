import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Mode = "dictate" | "translate" | "edit_selection";
export type SessionState = "idle" | "starting" | "recording" | "processing" | "cancelling" | "error";

export interface AppTone {
  app: string;
  style: string;
}

export type ProfileRole = "asr" | "llm";
export type CredentialRole = ProfileRole | "both";
export type ReadinessState =
  | "ready"
  | "missing_profile"
  | "missing_key"
  | "vault_error"
  | "invalid_config";

export interface AsrProfile {
  id: string;
  label: string;
  engine: "unisound_u2" | "openai_compatible";
  base_url: string | null;
  model: string | null;
  extra_options: [string, string][];
  credential_id: string | null;
  allow_insecure_http: boolean;
}

export interface LlmProfile {
  id: string;
  label: string;
  engine: "unisound" | "custom";
  format: "open_ai_chat" | "open_ai_responses" | "anthropic_messages";
  base_url: string | null;
  model: string | null;
  credential_id: string | null;
  allow_insecure_http: boolean;
}

export interface CredentialMetadata {
  id: string;
  label: string;
  role: CredentialRole;
  hint: string | null;
  created_at_ms: number;
  updated_at_ms: number;
}

export interface Readiness {
  state: ReadinessState;
  profile_id: string | null;
  credential_id: string | null;
  message: string | null;
}

export interface ApiStatus {
  asr: Readiness;
  llm: Readiness;
}

export interface ProfileTestResult {
  ok: boolean;
  verified: boolean;
  message: string;
  elapsed_ms: number;
}

export interface Settings {
  schema_version: 2;
  asr_profiles: AsrProfile[];
  llm_profiles: LlmProfile[];
  active_asr_profile_id: string | null;
  active_llm_profile_id: string | null;
  credentials: CredentialMetadata[];
  cleanup: { max_tokens: number; temperature: number };
  hotkey: {
    dictate: string;
    smart_gestures: boolean;
    dictate_alt: string | null;
    translate?: string | null;
    edit_selection?: string | null;
  };
  dictation: {
    language: string | null;
    cleanup_enabled: boolean;
    preserve_wording: boolean;
    no_list_formatting: boolean;
    app_tones: AppTone[];
    translate_target: string;
    default_mode?: Mode;
    default_tone?: string | null;
  };
  context: {
    include_app_context: boolean;
    include_caret_context: boolean;
    max_caret_chars: number;
  };
  history: { retention_days: number | null; keep_audio: boolean; pending_retention_days?: number };
  audio: {
    mic_device: string | null;
    mute_output_while_dictating: boolean;
    prewarm?: boolean;
    max_recording_seconds?: number;
  };
  ui: { locale: string; theme: string; onboarding_completed?: boolean | null; close_to_tray?: boolean };
}

export interface HistoryEntry {
  id: string;
  created_at_ms: number;
  mode: string;
  app_name: string | null;
  raw_text: string;
  final_text: string;
  asr_ms: number | null;
  cleanup_ms: number | null;
  insert_ok: boolean;
  pinned: boolean;
  target_language?: string | null;
  audio_path?: string | null;
  delivery_status?: "inserted" | "copied" | "needs_review" | "failed";
  warnings?: string[];
  source_pending_id?: string | null;
}

export interface HistoryQuery {
  limit: number;
  offset: number;
  query?: string;
  mode?: Mode | null;
  delivery?: "inserted" | "copied" | "needs_review" | "failed" | null;
  pinned?: boolean | null;
  from_ms?: number | null;
  to_ms?: number | null;
}

export interface SessionRequest {
  mode: Mode;
  target_language?: string | null;
  source: "hotkey" | "ui" | "tray";
}

export interface SessionStateEvent {
  state: string;
  message?: string;
  session_id?: string;
  mode?: Mode;
  elapsed_ms?: number;
  pending_id?: string | null;
}

export interface PendingRecord {
  id: string;
  created_at_ms: number;
  mode: Mode;
  target_language: string | null;
  stage: string;
  attempts: number;
  raw_text: string;
  final_text: string;
  error: string | null;
  has_audio: boolean;
  delivery_uncertain: boolean;
}

export interface DictionaryEntry {
  term: string;
}

export type UpdaterStatus =
  | "unconfigured"
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "error"
  | "up-to-date";

export interface UpdateInfo {
  version: string;
  notes: string;
  pub_date: string | null;
}

export interface UpdaterSnapshot {
  status: UpdaterStatus;
  current_version: string;
  update: UpdateInfo | null;
  progress: number | null;
  message: string | null;
}

export interface HistoryPage {
  entries: HistoryEntry[];
  total: number;
}

export interface SessionOutcome {
  id: string;
  raw_text: string;
  final_text: string;
  mode: string;
  timings: { asr_ms: number | null; cleanup_ms: number | null; total_ms: number };
  inserted: boolean;
  warnings: string[];
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  listMics: () => invoke<string[]>("list_mics"),
  permissionStatus: () => invoke<Record<string, string>>("permission_status"),
  toggle: () => invoke<void>("dictation_toggle"),
  cancel: () => invoke<void>("dictation_cancel"),
  sessionStart: (request: SessionRequest) => invoke<{ session_id: string }>("session_start", { request }),
  historyList: (query: HistoryQuery) =>
    invoke<HistoryPage>("history_list", { ...query }),
  historyDelete: (id: string) => invoke<void>("history_delete", { id }),
  historyDeleteAll: () => invoke<void>("history_delete_all"),
  historyPin: (id: string, pinned: boolean) =>
    invoke<void>("history_set_pinned", { id, pinned }),
  historyReprocess: (id: string, mode: Mode, targetLanguage?: string | null) =>
    invoke<HistoryEntry>("history_reprocess", { id, mode, target_language: targetLanguage ?? null }),
  historyExportAudio: (id: string) => invoke<boolean>("history_export_audio", { id }),
  pendingList: () => invoke<PendingRecord[]>("pending_list"),
  pendingRetry: (id: string) => invoke<PendingRecord>("pending_retry", { id }),
  pendingCopy: (id: string) => invoke<void>("pending_copy", { id }),
  pendingDiscard: (id: string) => invoke<void>("pending_discard", { id }),
  pendingCleanup: () => invoke<{ deleted_count: number; freed_bytes: number }>("pending_cleanup"),
  dictionaryList: () => invoke<DictionaryEntry[]>("dictionary_list"),
  dictionaryAdd: (term: string) => invoke<DictionaryEntry[]>("dictionary_add", { term }),
  dictionaryRename: (oldTerm: string, newTerm: string) =>
    invoke<DictionaryEntry[]>("dictionary_rename", { old_term: oldTerm, new_term: newTerm }),
  dictionaryRemove: (term: string) => invoke<DictionaryEntry[]>("dictionary_remove", { term }),
  updaterStatus: () => invoke<UpdaterSnapshot>("updater_status"),
  updaterCheck: () => invoke<UpdaterSnapshot>("updater_check"),
  updaterInstall: () => invoke<void>("updater_install"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  apiStatus: () => invoke<ApiStatus>("api_status"),
  saveAsrProfile: (profile: AsrProfile, confirmHostChange: boolean) =>
    invoke<Settings>("save_asr_profile", { profile, confirmHostChange }),
  saveLlmProfile: (profile: LlmProfile, confirmHostChange: boolean) =>
    invoke<Settings>("save_llm_profile", { profile, confirmHostChange }),
  deleteProfile: (role: ProfileRole, id: string) =>
    invoke<Settings>("delete_profile", { role, id }),
  setActiveProfile: (role: ProfileRole, id: string) =>
    invoke<Settings>("set_active_profile", { role, id }),
  createCredential: (label: string, role: CredentialRole, key: string) =>
    invoke<Settings>("create_credential", { label, role, key }),
  renameCredential: (id: string, label: string) =>
    invoke<Settings>("rename_credential", { id, label }),
  deleteCredential: (id: string) =>
    invoke<Settings>("delete_credential", { id }),
  replaceCredential: (
    id: string,
    key: string,
    profileId: string | null,
    replaceAll: boolean,
  ) => invoke<Settings>("replace_credential", { id, key, profileId, replaceAll }),
  cleanupLegacyKeys: () => invoke<string[]>("cleanup_legacy_keys"),
  exportConfig: () => invoke<boolean>("export_config"),
  importConfig: () => invoke<Settings | null>("import_config"),
  testProfile: (role: ProfileRole, id: string, mode: "check" | "request") =>
    invoke<ProfileTestResult>("test_profile", { role, id, mode }),
  openSettingsFolder: () => invoke<void>("open_settings_folder"),
};

/** Safe status used before the first IPC response or when status loading fails. */
export function missingProfileStatus(): ApiStatus {
  const readiness = (): Readiness => ({
    state: "missing_profile",
    profile_id: null,
    credential_id: null,
    message: null,
  });
  return { asr: readiness(), llm: readiness() };
}

// -- Event payload decoding ---------------------------------------------------
// IPC payloads are untrusted at the boundary: check each consumed field with
// typeof/Array.isArray before reading it.

export function decodeSessionState(payload: unknown): SessionStateEvent {
  if (typeof payload !== "object" || payload === null) {
    return { state: "error" };
  }
  const state =
    "state" in payload && typeof payload.state === "string"
      ? payload.state
      : "error";
  const message =
    "message" in payload && typeof payload.message === "string"
      ? payload.message
      : undefined;
  const sessionId =
    "session_id" in payload && typeof payload.session_id === "string"
      ? payload.session_id
      : undefined;
  const mode =
    "mode" in payload &&
    (payload.mode === "dictate" || payload.mode === "translate" || payload.mode === "edit_selection")
      ? payload.mode
      : undefined;
  const elapsed =
    "elapsed_ms" in payload && typeof payload.elapsed_ms === "number"
      ? payload.elapsed_ms
      : undefined;
  const pendingId =
    "pending_id" in payload && typeof payload.pending_id === "string"
      ? payload.pending_id
      : null;
  return { state, message, session_id: sessionId, mode, elapsed_ms: elapsed, pending_id: pendingId };
}

export function decodeLevel(payload: unknown): number {
  if (
    typeof payload === "object" &&
    payload !== null &&
    "level" in payload &&
    typeof payload.level === "number" &&
    Number.isFinite(payload.level)
  ) {
    return Math.min(1, Math.max(0, payload.level));
  }
  return 0;
}

export function decodeOutcome(payload: unknown): SessionOutcome | null {
  if (typeof payload !== "object" || payload === null) {
    return null;
  }
  const p: object = payload;
  if (!("final_text" in p) || typeof p.final_text !== "string") {
    return null;
  }
  const finalText: string = p.final_text;
  const timings =
    "timings" in p && typeof p.timings === "object" && p.timings !== null
      ? p.timings
      : null;
  const num = (holder: object | null, key: string): number | null => {
    if (!holder || !(key in holder)) return null;
    const v: unknown = (holder as Record<string, unknown>)[key];
    return typeof v === "number" ? v : null;
  };
  return {
    id: "id" in p && typeof p.id === "string" ? p.id : "",
    raw_text: "raw_text" in p && typeof p.raw_text === "string" ? p.raw_text : "",
    final_text: finalText,
    mode: "mode" in p && typeof p.mode === "string" ? p.mode : "dictate",
    timings: {
      asr_ms: num(timings, "asr_ms"),
      cleanup_ms: num(timings, "cleanup_ms"),
      total_ms: num(timings, "total_ms") ?? 0,
    },
    inserted: "inserted" in p && p.inserted === true,
    warnings:
      "warnings" in p && Array.isArray(p.warnings)
        ? p.warnings.filter((w): w is string => typeof w === "string")
        : [],
  };
}

export function decodeMessage(payload: unknown): string {
  if (
    typeof payload === "object" &&
    payload !== null &&
    "message" in payload &&
    typeof payload.message === "string"
  ) {
    return payload.message;
  }
  return typeof payload === "string" ? payload : "Unknown error";
}

export const events = {
  onSessionState: (cb: (s: SessionStateEvent) => void) =>
    listen("session:state", (e) => cb(decodeSessionState(e.payload))),
  onSessionLevel: (cb: (level: number) => void) =>
    listen("session:level", (e) => cb(decodeLevel(e.payload))),
  onSessionOutcome: (cb: (o: SessionOutcome) => void) =>
    listen("session:outcome", (e) => {
      const o = decodeOutcome(e.payload);
      if (o) cb(o);
    }),
  onSessionWarning: (cb: (message: string) => void) =>
    listen("session:warning", (e) => cb(decodeMessage(e.payload))),
  onSessionError: (cb: (message: string) => void) =>
    listen("session:error", (e) => cb(decodeMessage(e.payload))),
  onSettingsChanged: (cb: (s: Settings) => void) =>
    listen("settings:changed", (e) => cb(e.payload as Settings)),
  onPendingChanged: (cb: (payload: { pending_count?: number; latest_id?: string }) => void) =>
    listen("pending:changed", (e) => cb(e.payload as { pending_count?: number; latest_id?: string })),
  onAppNavigate: (cb: (payload: { tab: string }) => void) =>
    listen("app:navigate", (e) => {
      const payload = e.payload as { tab?: unknown } | null;
      if (payload && typeof payload.tab === "string") cb({ tab: payload.tab });
    }),
  onUpdaterChanged: (cb: (snapshot: UpdaterSnapshot) => void) =>
    listen("updater:changed", (e) => cb(e.payload as UpdaterSnapshot)),
  onUpdaterProgress: (cb: (progress: number) => void) =>
    listen("updater:progress", (e) => {
      const payload = e.payload as { progress?: unknown } | number;
      const value = typeof payload === "number" ? payload : payload?.progress;
      if (typeof value === "number" && Number.isFinite(value)) cb(Math.min(1, Math.max(0, value)));
    }),
};

export type { UnlistenFn };

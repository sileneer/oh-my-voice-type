import { useCallback, useEffect, useRef, useState } from "react";
import { Toaster } from "sonner";
import { api, events, missingProfileStatus, type ApiStatus, type Mode, type SessionState, type Settings } from "./lib/api";
import { notify, installToastSweeper } from "./lib/notify";
import { useTheme, normalizeThemePreference } from "./lib/theme";
import { makeT, resolveLocale } from "./i18n";
import { localizeMessage } from "./lib/messages";
import { AppShell } from "./components/app/AppShell";
import { HomePage } from "./components/home/HomePage";
import HistoryList from "./components/HistoryList";
import SettingsPanel from "./components/SettingsPanel";
import Onboarding from "./components/Onboarding";
import { Skeleton } from "./components/ui/skeleton";
import { Button } from "./components/ui/button";

export type { ApiStatus, SessionState } from "./lib/api";
type Tab = "home" | "history" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("home");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [permissions, setPermissions] = useState<Record<string, string>>({});
  const [apiStatus, setApiStatus] = useState<ApiStatus>(missingProfileStatus);
  const [session, setSession] = useState<SessionState>("idle");
  const [level, setLevel] = useState(0);
  const [mode, setMode] = useState<Mode>("dictate");
  const [targetLanguage, setTargetLanguage] = useState("English");
  const [pendingCount, setPendingCount] = useState(0);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [onboardingDismissed, setOnboardingDismissed] = useState(false);
  const [settingsSection, setSettingsSection] = useState<{ id: string; nonce: number } | null>(null);
  const localeRef = useRef<string | undefined>(undefined);
  localeRef.current = settings?.ui.locale;
  const activeSessionId = useRef<string | null>(null);

  useTheme(settings?.ui.theme);

  const refreshSettings = useCallback(async () => {
    const next = await api.getSettings();
    setSettings(next);
    setMode(next.dictation.default_mode ?? "dictate");
    setTargetLanguage(next.dictation.translate_target || "English");
  }, []);
  const refreshStatus = useCallback(async () => {
    try { setApiStatus(await api.apiStatus()); } catch { setApiStatus(missingProfileStatus()); }
  }, []);
  const refreshPending = useCallback(async () => {
    try { setPendingCount((await api.pendingList()).length); } catch { setPendingCount(0); }
  }, []);
  const refreshPermissions = useCallback(async () => {
    try { setPermissions(await api.permissionStatus()); } catch { setPermissions({}); }
  }, []);

  const navigate = useCallback((next: Tab) => {
    setTab(next);
    window.requestAnimationFrame(() => document.querySelector<HTMLElement>("h1")?.focus());
  }, []);
  const openSettings = useCallback((section: string) => {
    navigate("settings");
    setSettingsSection({ id: section, nonce: Date.now() });
  }, [navigate]);

  useEffect(() => {
    installToastSweeper();
    void refreshSettings().catch((cause) => setLoadError(String(cause)));
    void refreshStatus();
    void refreshPending();
    void refreshPermissions();
  }, [refreshPending, refreshPermissions, refreshSettings, refreshStatus]);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    // Backend messages are English; translate the known ones for the UI locale.
    const localized = (message: string) => {
      const current = resolveLocale(localeRef.current);
      return localizeMessage(message, current, makeT(current));
    };
    void events.onSessionWarning((message) => notify.warning(localized(message))).then((u) => unlisteners.push(u));
    void events.onSessionError((message) => notify.error(localized(message))).then((u) => unlisteners.push(u));
    void events.onSettingsChanged((next) => {
      setSettings(next);
      setMode(next.dictation.default_mode ?? "dictate");
      setTargetLanguage(next.dictation.translate_target || "English");
    }).then((u) => unlisteners.push(u));
    void events.onSessionState((event) => {
      if (event.session_id) {
        if (event.state === "starting") activeSessionId.current = event.session_id;
        else if (activeSessionId.current && event.session_id !== activeSessionId.current) return;
      }
      const valid: SessionState[] = ["idle", "starting", "recording", "processing", "cancelling", "error"];
      setSession(valid.includes(event.state as SessionState) ? event.state as SessionState : "idle");
      if (event.state === "error" && event.message) notify.error(localized(event.message));
      if (event.state === "idle") { activeSessionId.current = null; setLevel(0); void refreshPending(); }
    }).then((u) => unlisteners.push(u));
    void events.onSessionLevel((value) => setLevel(value)).then((u) => unlisteners.push(u));
    void events.onSessionOutcome(() => { activeSessionId.current = null; setLevel(0); void refreshPending(); }).then((u) => unlisteners.push(u));
    void events.onPendingChanged(() => void refreshPending()).then((u) => unlisteners.push(u));
    void events.onAppNavigate(({ tab: next, section }) => {
      if (next === "home" || next === "history" || next === "settings") navigate(next);
      if (next === "settings" && section) setSettingsSection({ id: section, nonce: Date.now() });
    }).then((u) => unlisteners.push(u));
    void events.onUpdaterAvailable((update) => {
      const t = makeT(resolveLocale(localeRef.current));
      notify.info(t("settings.updateToast", { version: update.version }), {
        duration: 15000,
        action: {
          label: t("settings.updateToastAction"),
          onClick: () => openSettings("about"),
        },
      });
    }).then((u) => unlisteners.push(u));
    return () => unlisteners.forEach((u) => u());
  }, [navigate, openSettings, refreshPending]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const editing = target instanceof HTMLElement && target.matches("input, textarea, select, [contenteditable='true']");
      if ((event.ctrlKey || event.metaKey) && event.key === ",") { event.preventDefault(); navigate("settings"); return; }
      if (editing || event.ctrlKey || event.metaKey) return;
      if (event.altKey && event.key === "1") { event.preventDefault(); navigate("home"); }
      if (event.altKey && event.key === "2") { event.preventDefault(); navigate("history"); }
      if (event.altKey && event.key === "3") { event.preventDefault(); navigate("settings"); }
      if (!event.altKey && event.key === "/" && tab === "history") { event.preventDefault(); document.querySelector<HTMLInputElement>("input[placeholder]")?.focus(); }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate, tab]);

  const saveSettings = useCallback(async (next: Settings) => {
    const saved = await api.setSettings(next);
    setSettings(saved);
    setMode(saved.dictation.default_mode ?? "dictate");
    setTargetLanguage(saved.dictation.translate_target || "English");
    await refreshStatus();
  }, [refreshStatus]);

  const startSession = useCallback(async () => {
    try {
      const handle = await api.sessionStart({ mode, target_language: mode === "translate" ? targetLanguage : null, source: "ui" });
      activeSessionId.current = handle.session_id;
    } catch (cause) { notify.error(String(cause)); }
  }, [mode, targetLanguage]);
  const cancelSession = useCallback(async () => {
    try { await api.cancel(); } catch (cause) { notify.error(String(cause)); }
  }, []);

  if (loadError) {
    const t = makeT(resolveLocale("auto"));
    return <div className="flex h-full flex-col items-center justify-center gap-4 p-8 text-center"><p className="max-w-lg text-sm text-destructive" role="alert">{loadError}</p><Button type="button" onClick={() => { setLoadError(null); void refreshSettings().catch((cause) => setLoadError(String(cause))); }}>{t("common.retry")}</Button></div>;
  }
  if (!settings) {
    return <div className="flex h-full items-center justify-center"><Skeleton className="h-48 w-80 max-w-[80vw]" /></div>;
  }

  const locale = resolveLocale(settings.ui.locale);
  const t = makeT(locale);
  const showOnboarding = settings.ui.onboarding_completed === false && !onboardingDismissed;

  const activeAsrLabel = settings.asr_profiles.find((p) => p.id === settings.active_asr_profile_id)?.label;
  const activeLlmLabel = settings.llm_profiles.find((p) => p.id === settings.active_llm_profile_id)?.label;

  return (
    <>
      {showOnboarding ? (
        <Onboarding
          settings={settings}
          permissions={permissions}
          apiStatus={apiStatus}
          locale={locale}
          t={t}
          onOpenSettings={() => { setOnboardingDismissed(true); openSettings("providers"); }}
          onRefreshPermissions={() => void refreshPermissions()}
          onChanged={async () => { await refreshSettings(); await refreshStatus(); }}
          onSave={saveSettings}
          onFinish={() => setOnboardingDismissed(true)}
        />
      ) : (
        <AppShell
          tab={tab}
          apiStatus={apiStatus}
          activeAsrLabel={activeAsrLabel}
          activeLlmLabel={activeLlmLabel}
          pendingCount={pendingCount}
          t={t}
          onNavigate={navigate}
          onOpenProviders={() => openSettings("providers")}
        >
          {tab === "home" && <HomePage settings={settings} apiStatus={apiStatus} permissions={permissions} session={session} mode={mode} targetLanguage={targetLanguage} level={level} pendingCount={pendingCount} locale={locale} t={t} onOpenSettings={() => openSettings("providers")} onOpenHistory={() => navigate("history")} onModeChange={setMode} onTargetChange={setTargetLanguage} onStart={() => void startSession()} onCancel={() => void cancelSession()} onOpenHotkeys={() => openSettings("modes")} onRefreshPermissions={refreshPermissions} />}
          {tab === "history" && <HistoryList locale={locale} t={t} hotkey={settings.hotkey.dictate} />}
          {tab === "settings" && <SettingsPanel settings={settings} apiStatus={apiStatus} t={t} locale={locale} onSave={saveSettings} onError={(message) => notify.error(message)} onChanged={refreshSettings} onApiStatusChanged={refreshStatus} section={settingsSection} />}
        </AppShell>
      )}
      <Toaster theme={normalizeThemePreference(settings.ui.theme)} position="bottom-right" richColors closeButton />
    </>
  );
}

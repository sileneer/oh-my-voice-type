import { useCallback, useEffect, useState } from "react";
import { ArrowRight, RotateCcw } from "lucide-react";
import { api, type ApiStatus, type HistoryEntry, type Mode, type SessionState, type Settings } from "@/lib/api";
import type { Locale, TFunc } from "@/i18n";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/app/PageHeader";
import { PermissionList } from "@/components/app/PermissionList";
import { DictationPanel } from "./DictationPanel";
import SetupBanner from "../SetupBanner";

export function HomePage({
  settings,
  apiStatus,
  permissions,
  session,
  mode,
  targetLanguage,
  level,
  pendingCount,
  locale,
  t,
  onOpenSettings,
  onOpenHistory,
  onModeChange,
  onTargetChange,
  onStart,
  onCancel,
  onSave,
  onRefreshPermissions,
  onError,
}: {
  settings: Settings;
  apiStatus: ApiStatus;
  permissions: Record<string, string>;
  session: SessionState;
  mode: Mode;
  targetLanguage: string;
  level: number;
  pendingCount: number;
  locale: Locale;
  t: TFunc;
  onOpenSettings: () => void;
  onOpenHistory: () => void;
  onModeChange: (mode: Mode) => void;
  onTargetChange: (language: string) => void;
  onStart: () => void;
  onCancel: () => void;
  onSave: (settings: Settings) => Promise<void>;
  onRefreshPermissions: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const [recent, setRecent] = useState<HistoryEntry[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const loadRecent = useCallback(async () => {
    try {
      const page = await api.historyList({ limit: 3, offset: 0 });
      setRecent(page.entries);
    } catch {
      setRecent([]);
    }
  }, []);
  useEffect(() => { void loadRecent(); }, [loadRecent, session]);

  const refreshPermissions = async () => {
    setRefreshing(true);
    try { await onRefreshPermissions(); } finally { setRefreshing(false); }
  };

  return (
    <>
      <PageHeader title={t("home.title")} description={t("home.subtitle")} />
      {apiStatus.asr.state !== "ready" && <SetupBanner status={apiStatus} t={t} onOpenSettings={onOpenSettings} />}
      {pendingCount > 0 && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-warning/25 bg-warning/10 p-4" role="status">
          <div className="flex items-center gap-2 text-sm"><RotateCcw className="size-4 text-warning" aria-hidden="true" />{t("home.recovery")}</div>
          <Button type="button" variant="outline" size="sm" onClick={onOpenHistory}>{t("home.openRecovery")}</Button>
        </div>
      )}
      <DictationPanel
        settings={settings}
        apiStatus={apiStatus}
        session={session}
        mode={mode}
        targetLanguage={targetLanguage}
        level={level}
        locale={locale}
        t={t}
        onModeChange={onModeChange}
        onTargetChange={onTargetChange}
        onStart={onStart}
        onCancel={onCancel}
        onSave={onSave}
        onError={onError}
      />
      <div className="mt-5 grid gap-5 lg:grid-cols-2">
        <PermissionList permissions={permissions} locale={locale} t={t} onRefresh={() => void refreshPermissions()} refreshing={refreshing} />
        <section className="rounded-lg border border-border bg-card p-4">
          <div className="flex items-center justify-between gap-3">
            <h2 className="text-sm font-semibold">{t("home.recent")}</h2>
            <Button type="button" variant="ghost" size="sm" onClick={onOpenHistory}>{t("common.viewAll")}<ArrowRight className="size-4" aria-hidden="true" /></Button>
          </div>
          {recent.length === 0 ? (
            <p className="mt-4 text-sm text-muted-foreground">{t("history.empty")}</p>
          ) : (
            <ul className="mt-3 divide-y divide-border">
              {recent.map((entry) => (
                <li key={entry.id} className="py-3">
                  <p className="line-clamp-2 text-sm leading-relaxed text-secondary-foreground">{entry.final_text}</p>
                  <p className="mt-1 text-xs text-muted-foreground">{new Date(entry.created_at_ms).toLocaleString(locale)}{entry.app_name ? ` · ${entry.app_name}` : ""}</p>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </>
  );
}

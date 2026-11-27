import { useCallback, useEffect, useState } from "react";
import { ArrowRight, RotateCcw } from "lucide-react";
import { api, type ApiStatus, type HistoryEntry, type Mode, type SessionState, type Settings } from "@/lib/api";
import { localizeHotkey, type Locale, type TFunc } from "@/i18n";
import { formatWhen } from "@/lib/format";
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
  onOpenHotkeys,
  onRefreshPermissions,
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
  onOpenHotkeys: () => void;
  onRefreshPermissions: () => Promise<void>;
}) {
  const [recent, setRecent] = useState<HistoryEntry[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const loadRecent = useCallback(async () => {
    try {
      // Newest first: pinned-first ordering showed old pinned items as "recent".
      const page = await api.historyList({ limit: 3, offset: 0, newest_first: true });
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
      {apiStatus.asr.state !== "ready" && <div className="mb-4"><SetupBanner status={apiStatus} t={t} onOpenSettings={onOpenSettings} /></div>}
      {pendingCount > 0 && (
        <div className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-warning/25 bg-warning/10 p-4" role="status">
          <div className="flex items-center gap-2 text-sm"><RotateCcw className="size-4 text-warning" aria-hidden="true" />{pendingCount === 1 ? t("home.recoveryOne") : t("home.recovery", { n: pendingCount })}</div>
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
        onOpenHotkeys={onOpenHotkeys}
      />
      <div className="mt-5 grid gap-5 lg:grid-cols-2">
        <PermissionList permissions={permissions} locale={locale} t={t} onRefresh={() => void refreshPermissions()} refreshing={refreshing} />
        <section className="rounded-lg border border-border bg-card p-4">
          <div className="flex items-center justify-between gap-3">
            <h2 className="text-sm font-semibold">{t("home.recent")}</h2>
            <Button type="button" variant="ghost" size="sm" onClick={onOpenHistory}>{t("common.viewAll")}<ArrowRight className="size-4" aria-hidden="true" /></Button>
          </div>
          {recent.length === 0 ? (
            <p className="mt-4 text-sm text-muted-foreground">
              {t("history.empty", { hotkey: localizeHotkey(locale, settings.hotkey.dictate) })}
            </p>
          ) : (
            <ul className="mt-3 divide-y divide-border">
              {recent.map((entry) => (
                <li key={entry.id} className="py-3">
                  <p className="line-clamp-2 text-sm leading-relaxed text-secondary-foreground">{entry.final_text || entry.raw_text}</p>
                  <p className="mt-1 text-xs text-muted-foreground">{formatWhen(entry.created_at_ms, locale)}{entry.app_name ? ` · ${entry.app_name}` : ""}</p>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </>
  );
}

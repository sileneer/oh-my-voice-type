import { useEffect, useState } from "react";
import { Download, LoaderCircle, RefreshCw, RotateCw } from "lucide-react";
import { api, events, type Settings, type UpdaterSnapshot } from "../lib/api";
import type { Locale, TFunc } from "../i18n";
import { Button } from "./ui/button";
import { Switch } from "./ui/switch";

interface Props {
  settings: Settings;
  locale: Locale;
  t: TFunc;
  onSave: (settings: Settings) => Promise<void>;
  onError: (message: string) => void;
}

/** Update status, release notes and the install / restart actions. */
export function UpdatePanel({ settings, locale, t, onSave, onError }: Props) {
  const [snapshot, setSnapshot] = useState<UpdaterSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [autoCheck, setAutoCheck] = useState(settings.ui.check_for_updates ?? true);

  useEffect(() => setAutoCheck(settings.ui.check_for_updates ?? true), [settings.ui.check_for_updates]);
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void api.updaterStatus().then((value) => { if (active) setSnapshot(value); }).catch(() => undefined);
    void events.onUpdaterChanged((value) => setSnapshot(value)).then((stop) => {
      if (active) unlisten = stop; else stop();
    });
    return () => { active = false; unlisten?.(); };
  }, []);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try { await action(); } catch (cause) { onError(String(cause)); } finally { setBusy(false); }
  };
  const check = () => run(async () => setSnapshot(await api.updaterCheck()));
  const install = () => run(() => api.updaterInstall());
  const restart = () => run(() => api.updaterRestart());
  const toggleAuto = (checked: boolean) => run(async () => {
    setAutoCheck(checked);
    try {
      await onSave({ ...settings, ui: { ...settings.ui, check_for_updates: checked } });
    } catch (cause) {
      setAutoCheck(!checked);
      throw cause;
    }
  });

  const status = snapshot?.status ?? "idle";
  const update = snapshot?.update ?? null;
  const working = busy || status === "checking" || status === "downloading" || status === "installing";
  const canInstall = update !== null && (status === "available" || status === "error");
  const showProgress = status === "downloading" || status === "installing";
  const showNotes = update !== null && status !== "ready";

  return (
    <div className="space-y-4">
      <div className="rounded-md border border-border bg-muted/40 p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0">
            <p className="text-sm font-medium">{t("settings.updateCheck")}</p>
            <p className="mt-1 text-sm text-muted-foreground" role="status" aria-live="polite">{statusMessage(snapshot, t)}</p>
            {snapshot?.checked_at_ms != null && (
              <p className="mt-1 text-xs text-muted-foreground">{t("settings.updateLastChecked", { time: formatDate(snapshot.checked_at_ms, locale) })}</p>
            )}
          </div>
          <div className="flex flex-wrap gap-2">
            {status !== "ready" && (
              <Button type="button" variant="outline" size="sm" onClick={() => void check()} disabled={working}>
                {status === "checking" ? <LoaderCircle className="size-4 animate-spin" aria-hidden="true" /> : <RefreshCw className="size-4" aria-hidden="true" />}
                {t("settings.updateCheck")}
              </Button>
            )}
            {canInstall && (
              <Button type="button" size="sm" onClick={() => void install()} disabled={working}>
                <Download className="size-4" aria-hidden="true" />{t("settings.updateInstall")}
              </Button>
            )}
            {status === "ready" && (
              <Button type="button" size="sm" onClick={() => void restart()} disabled={busy}>
                <RotateCw className="size-4" aria-hidden="true" />{t("settings.updateRestart")}
              </Button>
            )}
          </div>
        </div>
        {showProgress && (
          <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-border" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent(snapshot?.progress)}>
            <div className={`h-full bg-primary transition-[width] ${snapshot?.progress == null ? "w-1/3 animate-pulse" : ""}`} style={snapshot?.progress == null ? undefined : { width: `${percent(snapshot.progress)}%` }} />
          </div>
        )}
        {canInstall && <p className="mt-3 text-xs leading-relaxed text-muted-foreground">{t("settings.updateInstallHint")}</p>}
        {showNotes && update && (
          <div className="mt-4 border-t border-border pt-4">
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <p className="text-sm font-medium">{t("settings.updateNotes", { version: update.version })}</p>
              {update.pub_date && <p className="text-xs text-muted-foreground">{t("settings.updatePublished", { date: formatDate(update.pub_date, locale) })}</p>}
            </div>
            <div className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-words text-sm leading-relaxed text-secondary-foreground">
              {plainNotes(update.notes) || t("settings.updateNoNotes")}
            </div>
          </div>
        )}
      </div>
      <div className="flex items-center justify-between gap-4">
        <div>
          <p className="text-sm font-medium text-secondary-foreground">{t("settings.updateAuto")}</p>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{t("settings.updateAutoHint")}</p>
        </div>
        <Switch checked={autoCheck} onCheckedChange={(checked) => void toggleAuto(checked)} disabled={busy} aria-label={t("settings.updateAuto")} />
      </div>
    </div>
  );
}

/** Release notes are Markdown; show them as readable plain text. */
export function plainNotes(notes: string) {
  return notes
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/\[([^\]]+)\]\((https?:[^)]+)\)/g, "$1 ($2)")
    .trim();
}

function percent(progress: number | null | undefined) {
  return progress == null ? 0 : Math.round(Math.min(1, Math.max(0, progress)) * 100);
}

function formatDate(value: number | string, locale: Locale) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? String(value) : date.toLocaleString(locale);
}

export function statusMessage(snapshot: UpdaterSnapshot | null, t: TFunc) {
  const version = snapshot?.update?.version ?? "";
  switch (snapshot?.status ?? "idle") {
    case "idle": return t("settings.updateIdle");
    case "checking": return t("settings.updateChecking");
    case "available": return t("settings.updateAvailable", { version });
    case "downloading": return t("settings.updateDownloading", { version, percent: snapshot?.progress == null ? "" : `${percent(snapshot.progress)}%` }).trim();
    case "installing": return t("settings.updateInstalling", { version });
    case "ready": return t("settings.updateReady", { version });
    case "up-to-date": return t("settings.updateUpToDate");
    case "error": return t("settings.updateError", { message: snapshot?.message ?? "" });
    default: return t("settings.updateIdle");
  }
}

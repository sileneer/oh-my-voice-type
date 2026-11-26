import { useEffect, useState } from "react";
import { CheckCircle2, LoaderCircle, Mic, PencilLine, Square, Languages, TriangleAlert } from "lucide-react";
import { type ApiStatus, type Mode, type SessionState, type Settings } from "@/lib/api";
import { dictationLanguages, localizeHotkey, type Locale, type TFunc } from "@/i18n";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { AudioLevel } from "./AudioLevel";

const HOTKEY_OPTIONS = ["fn", "right-opt", "right-cmd", "right-alt", "right-ctrl", "left-alt", "left-ctrl", "f6", "f7", "f8", "f9", "f10"];
const modes: Array<{ id: Mode; label: "home.mode.dictate" | "home.mode.translate" | "home.mode.edit_selection"; icon: typeof Mic }> = [
  { id: "dictate", label: "home.mode.dictate", icon: Mic },
  { id: "translate", label: "home.mode.translate", icon: Languages },
  { id: "edit_selection", label: "home.mode.edit_selection", icon: PencilLine },
];

export function DictationPanel({
  settings,
  apiStatus,
  session,
  mode,
  targetLanguage,
  level,
  locale,
  t,
  onModeChange,
  onTargetChange,
  onStart,
  onCancel,
  onSave,
  onError,
}: {
  settings: Settings;
  apiStatus: ApiStatus;
  session: SessionState;
  mode: Mode;
  targetLanguage: string;
  level: number;
  locale: Locale;
  t: TFunc;
  onModeChange: (mode: Mode) => void;
  onTargetChange: (language: string) => void;
  onStart: () => void;
  onCancel: () => void;
  onSave: (settings: Settings) => Promise<void>;
  onError: (message: string) => void;
}) {
  const [hotkey, setHotkey] = useState(settings.hotkey.dictate);
  const [smart, setSmart] = useState(settings.hotkey.smart_gestures);
  const [saving, setSaving] = useState(false);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => setHotkey(settings.hotkey.dictate), [settings.hotkey.dictate]);
  useEffect(() => setSmart(settings.hotkey.smart_gestures), [settings.hotkey.smart_gestures]);
  useEffect(() => {
    if (session !== "recording" && session !== "starting") {
      setElapsed(0);
      return;
    }
    const started = Date.now();
    const timer = window.setInterval(() => setElapsed(Math.floor((Date.now() - started) / 1000)), 250);
    return () => window.clearInterval(timer);
  }, [session]);

  const active = session === "recording" || session === "starting";
  const busy = session === "processing" || session === "cancelling";
  const unavailable = apiStatus.asr.state !== "ready" || (mode === "translate" && apiStatus.llm.state !== "ready");
  const needsSetup = unavailable && session === "idle";
  const status = session === "recording"
    ? t("hotkey.status.listening")
    : session === "starting"
      ? t("hotkey.status.starting")
      : session === "processing"
        ? t("hotkey.status.processing")
        : session === "cancelling"
          ? t("common.cancel")
          : session === "error"
            ? t("hotkey.status.error")
            : needsSetup
              ? t("common.notConfigured")
              : t("hotkey.status.ready");
  const StatusIcon = session === "recording"
    ? Mic
    : busy || session === "starting"
      ? LoaderCircle
      : session === "error" || needsSetup
        ? TriangleAlert
        : CheckCircle2;

  const saveHotkey = async () => {
    setSaving(true);
    try {
      await onSave({ ...settings, hotkey: { ...settings.hotkey, dictate: hotkey, smart_gestures: smart } });
    } catch (cause) {
      onError(String(cause));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="rounded-lg border border-border bg-card p-5 md:p-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="text-base font-semibold tracking-tight">{t("home.session")}</h2>
        </div>
        <div className={cn("flex items-center gap-2 text-sm", session === "error" ? "text-destructive" : needsSetup ? "text-warning" : "text-muted-foreground")} role="status" aria-live="polite">
          <StatusIcon className={cn("size-4", (busy || active) && "animate-pulse motion-reduce:animate-none")} aria-hidden="true" />
          {status}
          {active && <span className="font-mono text-xs tabular-nums">{formatTime(elapsed)}</span>}
        </div>
      </div>

      <div className="mt-5 grid gap-2 sm:grid-cols-3" role="radiogroup" aria-label={t("settings.defaultMode")}>
        {modes.map(({ id, label, icon: Icon }) => (
          <Button
            key={id}
            type="button"
            variant="ghost"
            role="radio"
            aria-checked={mode === id}
            disabled={active || busy}
            onClick={() => onModeChange(id)}
            className={cn(
              "flex h-10 items-center justify-center gap-2 rounded-md border text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50",
              mode === id ? "border-primary/40 bg-primary/10 text-primary" : "border-border text-muted-foreground hover:bg-accent hover:text-accent-foreground",
            )}
          >
            <Icon className="size-4" aria-hidden="true" />
            {t(label)}
          </Button>
        ))}
      </div>

      {mode === "translate" && (
        <div className="mt-4 max-w-xs">
          <Label htmlFor="translate-target" className="mb-1.5 block text-xs">{t("home.translateTarget")}</Label>
          <Select value={targetLanguage} onValueChange={onTargetChange} disabled={active || busy}>
            <SelectTrigger id="translate-target"><SelectValue /></SelectTrigger>
            <SelectContent>
              {dictationLanguages.map((language) => <SelectItem key={language.code} value={language.native}>{language.native}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
      )}

      <div className="mt-5 flex flex-wrap items-center gap-3 border-t border-border pt-5">
        <Button type="button" size="lg" onClick={active || busy ? onCancel : onStart} disabled={session === "cancelling" || (!active && !busy && unavailable)} variant={active || busy ? "destructive" : "default"}>
          {active || busy ? <Square className="size-4" aria-hidden="true" /> : <Mic className="size-4" aria-hidden="true" />}
          {active || busy ? t("home.cancel") : t("home.start", { mode: t(modes.find((item) => item.id === mode)!.label) })}
        </Button>
        <AudioLevel level={level} active={session === "recording"} />
        <div className="ml-auto flex items-center gap-2 text-xs text-muted-foreground">
          <span>{t("hotkey.title")}</span>
          <kbd className="rounded border border-border bg-muted px-2 py-1 font-mono text-foreground">{localizeHotkey(locale, hotkey)}</kbd>
        </div>
      </div>

      <div className="mt-4 flex flex-wrap items-end gap-3">
        <div className="min-w-40">
          <Label htmlFor="primary-hotkey" className="mb-1.5 block text-xs">{t("hotkey.title")}</Label>
          <Select value={hotkey} onValueChange={setHotkey} disabled={active || busy}>
            <SelectTrigger id="primary-hotkey"><SelectValue /></SelectTrigger>
            <SelectContent>
              {HOTKEY_OPTIONS.map((key) => <SelectItem key={key} value={key}>{localizeHotkey(locale, key)}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
        <div className="flex h-10 items-center gap-2">
          <Switch id="smart-gestures" checked={smart} onCheckedChange={setSmart} disabled={active || busy} />
          <Label htmlFor="smart-gestures" className="text-xs font-normal">{t("hotkey.smart")}</Label>
        </div>
        <Button type="button" variant="outline" onClick={() => void saveHotkey()} disabled={saving || active || busy}>{saving ? t("hotkey.saving") : t("hotkey.save")}</Button>
      </div>
      {mode === "translate" && apiStatus.llm.state !== "ready" && <p className="mt-3 text-sm text-warning">{t("home.translateNeedsLlm")}</p>}
    </section>
  );
}

function formatTime(totalSeconds: number) {
  const minutes = Math.floor(totalSeconds / 60).toString().padStart(2, "0");
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

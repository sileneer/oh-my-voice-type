import { useEffect, useState } from "react";
import { ArrowRight, CheckCircle2, Keyboard, LoaderCircle, Mic, PencilLine, Square, Languages, TriangleAlert } from "lucide-react";
import { type ApiStatus, type Mode, type SessionState, type Settings } from "@/lib/api";
import { localizeHotkey, translationTargets, type Locale, type MessageKey, type TFunc } from "@/i18n";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Label } from "@/components/ui/label";
import { AudioLevel } from "./AudioLevel";

const modes: Array<{ id: Mode; label: MessageKey; start: MessageKey; icon: typeof Mic }> = [
  { id: "dictate", label: "home.mode.dictate", start: "home.start.dictate", icon: Mic },
  { id: "translate", label: "home.mode.translate", start: "home.start.translate", icon: Languages },
  { id: "edit_selection", label: "home.mode.edit_selection", start: "home.start.edit_selection", icon: PencilLine },
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
  onOpenHotkeys,
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
  onOpenHotkeys: () => void;
}) {
  const [elapsed, setElapsed] = useState(0);
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

  const modeHotkey = mode === "translate"
    ? settings.hotkey.translate
    : mode === "edit_selection"
      ? settings.hotkey.edit_selection
      : settings.hotkey.dictate;
  const current = modes.find((item) => item.id === mode) ?? modes[0];

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
              {translationTargets(targetLanguage).map((language) => <SelectItem key={language} value={language}>{language}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
      )}

      <div className="mt-5 flex flex-wrap items-center gap-3 border-t border-border pt-5">
        <Button type="button" size="lg" onClick={active || busy ? onCancel : onStart} disabled={session === "cancelling" || (!active && !busy && unavailable)} variant={active || busy ? "destructive" : "default"}>
          {active || busy ? <Square className="size-4" aria-hidden="true" /> : <Mic className="size-4" aria-hidden="true" />}
          {active || busy ? t("home.cancel") : t(current.start)}
        </Button>
        <AudioLevel level={level} active={session === "recording"} />
      </div>

      <div className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-md bg-muted/50 px-3 py-2.5">
        <p className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
          <Keyboard className="size-4 shrink-0" aria-hidden="true" />
          {modeHotkey
            ? <HotkeyHint text={t("home.hotkeyHint")} hotkey={localizeHotkey(locale, modeHotkey)} />
            : t("common.notConfigured")}
        </p>
        <Button type="button" variant="ghost" size="sm" onClick={onOpenHotkeys}>{t("home.changeHotkey")}<ArrowRight className="size-4" aria-hidden="true" /></Button>
      </div>
      {mode === "translate" && apiStatus.llm.state !== "ready" && <p className="mt-3 text-sm text-warning">{t("home.translateNeedsLlm")}</p>}
    </section>
  );
}

/** Renders a translated sentence with the hotkey as a key cap in place of {hotkey}. */
function HotkeyHint({ text, hotkey }: { text: string; hotkey: string }) {
  const [before, after = ""] = text.split("{hotkey}");
  return <span>{before}<kbd className="mx-0.5 rounded border border-border bg-card px-1.5 py-0.5 font-mono text-xs text-foreground">{hotkey}</kbd>{after}</span>;
}

function formatTime(totalSeconds: number) {
  const minutes = Math.floor(totalSeconds / 60).toString().padStart(2, "0");
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

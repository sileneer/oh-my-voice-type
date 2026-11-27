import { useEffect, useRef, useState } from "react";
import { Check, Languages, LoaderCircle, Mic, PencilLine, Square, TriangleAlert, X } from "lucide-react";
import { isTauri } from "@tauri-apps/api/core";
import { api, events, type Mode } from "./lib/api";
import { useTheme } from "./lib/theme";
import { makeT, resolveLocale } from "./i18n";
import { cn } from "./lib/utils";
import { localizeMessage } from "./lib/messages";

type Phase =
  | { kind: "idle" }
  | { kind: "starting" | "recording" | "processing" | "cancelling"; mode: Mode; level: number }
  | { kind: "error" | "done"; mode: Mode; message: string };

const modeIcons = { dictate: Mic, translate: Languages, edit_selection: PencilLine };

export default function Pill() {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [locale, setLocale] = useState(resolveLocale("auto"));
  const [theme, setTheme] = useState("system");
  const [elapsed, setElapsed] = useState(0);
  const sessionId = useRef<string | null>(null);
  useTheme(theme);

  useEffect(() => {
    void api.getSettings().then((settings) => {
      setLocale(resolveLocale(settings.ui.locale));
      setTheme(settings.ui.theme);
    }).catch(() => undefined);
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    void events.onSettingsChanged((settings) => {
      setLocale(resolveLocale(settings.ui.locale));
      setTheme(settings.ui.theme);
    }).then((u) => unlisteners.push(u));
    void events.onSessionState((event) => {
      if (event.session_id) {
        if (event.state === "starting") sessionId.current = event.session_id;
        else if (sessionId.current && event.session_id !== sessionId.current) return;
      }
      const mode = event.mode ?? "dictate";
      if (event.state === "starting" || event.state === "recording" || event.state === "processing" || event.state === "cancelling") {
        setPhase({ kind: event.state, mode, level: 0 });
      } else if (event.state === "error") {
        setPhase({ kind: "error", mode, message: event.message ?? "Unknown error" });
      } else if (event.state === "idle") {
        sessionId.current = null;
        setPhase((current) => current.kind === "done" || current.kind === "error" ? current : { kind: "idle" });
      }
    }).then((u) => unlisteners.push(u));
    void events.onSessionLevel((level) => setPhase((current) => current.kind === "recording" ? { ...current, level } : current)).then((u) => unlisteners.push(u));
    void events.onSessionOutcome((outcome) => {
      const mode: Mode = outcome.mode === "translate" || outcome.mode === "edit_selection" ? outcome.mode : "dictate";
      setPhase({ kind: "done", mode, message: outcome.final_text.slice(0, 80) });
      sessionId.current = null;
    }).then((u) => unlisteners.push(u));
    return () => unlisteners.forEach((u) => u());
  }, []);

  useEffect(() => {
    if (phase.kind !== "recording" && phase.kind !== "starting" && phase.kind !== "processing" && phase.kind !== "cancelling") {
      setElapsed(0);
      return;
    }
    const started = Date.now();
    const timer = window.setInterval(() => setElapsed(Math.floor((Date.now() - started) / 1000)), 250);
    return () => window.clearInterval(timer);
  }, [phase.kind]);

  useEffect(() => {
    if (phase.kind !== "done" && phase.kind !== "error") return;
    // Hide through the backend: on Windows the pill is shown with a raw Win32
    // call, so `getCurrentWindow().hide()` silently does nothing.
    const timer = window.setTimeout(() => { if (isTauri()) void api.pillHide().catch(() => undefined); }, 1500);
    return () => window.clearTimeout(timer);
  }, [phase.kind]);

  const t = makeT(locale);
  const mode = phase.kind === "idle" ? "dictate" : phase.mode;
  const ModeIcon = modeIcons[mode];
  const active = phase.kind === "starting" || phase.kind === "recording" || phase.kind === "processing" || phase.kind === "cancelling";
  const label = (() => {
    switch (phase.kind) {
      case "idle": return t("pill.idle");
      case "starting": return t("pill.starting");
      case "recording": return t("pill.listening");
      case "processing": return t("pill.processing");
      case "cancelling": return t("common.cancel");
      case "done": return t("pill.done");
      case "error": return localizeMessage(phase.message, locale, t);
    }
  })();
  const StatusIcon = phase.kind === "done" ? Check : phase.kind === "error" ? TriangleAlert : active && phase.kind !== "recording" ? LoaderCircle : ModeIcon;

  return (
    <div className="flex h-full w-full items-center justify-center" role="status" aria-live="polite">
      {/* Fill the transparent window exactly: an outer shadow (or margin) gets
          clipped at the window edge and shows up as a faint rectangle. */}
      <div className={cn("flex h-full w-full items-center gap-3 rounded-full border bg-card px-4", phase.kind === "error" ? "border-destructive/40" : "border-border")}>
        <div className={cn("flex size-8 shrink-0 items-center justify-center rounded-full", phase.kind === "recording" ? "bg-destructive/10 text-destructive" : phase.kind === "error" ? "bg-destructive/10 text-destructive" : "bg-primary/10 text-primary")}>
          <StatusIcon className={cn("size-4", active && phase.kind !== "recording" && "animate-spin motion-reduce:animate-none")} aria-hidden="true" />
        </div>
        <div className="min-w-0 flex-1">
          <p className={cn("truncate text-xs font-medium", phase.kind === "error" ? "text-destructive" : "text-foreground")}>{label}</p>
          {active && <p className="mt-0.5 font-mono text-[11px] tabular-nums text-muted-foreground">{formatTime(elapsed)} · {t(`home.mode.${mode}`)}</p>}
        </div>
        {phase.kind === "recording" && <Waveform level={phase.level} />}
        {active && <button type="button" onClick={() => void api.cancel()} aria-label={t("pill.cancel")} className="flex size-8 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"><Square className="size-3.5" aria-hidden="true" /></button>}
        {phase.kind === "error" && <button type="button" onClick={() => void api.pillHide().catch(() => undefined)} aria-label={t("common.close")} className="flex size-8 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"><X className="size-4" aria-hidden="true" /></button>}
      </div>
    </div>
  );
}

function Waveform({ level }: { level: number }) {
  return <div className="flex h-6 shrink-0 items-center gap-0.5" aria-hidden="true">{Array.from({ length: 5 }).map((_, index) => <span key={index} className={cn("w-0.5 rounded-full transition-[height] duration-100 motion-reduce:transition-none", level > (index + 1) / 6 ? "bg-destructive" : "bg-border")} style={{ height: `${5 + index * 2}px` }} />)}</div>;
}

function formatTime(totalSeconds: number) {
  return `${Math.floor(totalSeconds / 60).toString().padStart(2, "0")}:${(totalSeconds % 60).toString().padStart(2, "0")}`;
}

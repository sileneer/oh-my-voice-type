import { useState } from "react";
import { ArrowLeft, ArrowRight, AudioLines, Check, KeyRound, Keyboard, Mic2, ShieldCheck } from "lucide-react";
import type { ApiStatus, Settings } from "../lib/api";
import { localizeHotkey, type Locale, type TFunc } from "../i18n";
import { Button } from "../components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { ReadinessBadge } from "./app/ReadinessBadge";
import { PermissionList } from "./app/PermissionList";

const HOTKEYS = ["fn", "right-opt", "right-cmd", "right-alt", "right-ctrl", "left-alt", "left-ctrl", "f6", "f7", "f8", "f9", "f10"];
const steps = ["welcome", "provider", "permission", "hotkey", "finish"] as const;

export default function Onboarding({ settings, permissions, apiStatus, locale, t, onOpenSettings, onRefreshPermissions, onSave, onFinish }: {
  settings: Settings;
  permissions: Record<string, string>;
  apiStatus: ApiStatus;
  locale: Locale;
  t: TFunc;
  onOpenSettings: () => void;
  onRefreshPermissions: () => void;
  onSave: (settings: Settings) => Promise<void>;
  onFinish: () => void;
}) {
  const [step, setStep] = useState(0);
  const [hotkey, setHotkey] = useState(settings.hotkey.dictate);
  const [saving, setSaving] = useState(false);
  const complete = async () => {
    setSaving(true);
    try { await onSave({ ...settings, hotkey: { ...settings.hotkey, dictate: hotkey }, ui: { ...settings.ui, onboarding_completed: true } }); onFinish(); }
    finally { setSaving(false); }
  };
  const skip = async () => { setSaving(true); try { await onSave({ ...settings, ui: { ...settings.ui, onboarding_completed: true } }); onFinish(); } finally { setSaving(false); } };
  const current = steps[step];
  const Icon = current === "provider" ? KeyRound : current === "permission" ? ShieldCheck : current === "hotkey" ? Keyboard : current === "finish" ? Check : AudioLines;
  const title = `onboarding.${current}Title` as const;
  const body = `onboarding.${current}Body` as const;
  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      <header className="flex items-center justify-between border-b border-border px-6 py-4"><div className="flex items-center gap-2.5"><AudioLines className="size-5 text-primary" aria-hidden="true" /><span className="text-sm font-semibold">oh-my-voice-type</span></div><Button type="button" variant="ghost" size="sm" onClick={() => void skip()} disabled={saving}>{t("onboarding.skip")}</Button></header>
      <main className="mx-auto flex w-full max-w-2xl flex-1 flex-col justify-center px-6 py-10">
        <div className="mb-8 flex gap-2" role="progressbar" aria-valuemin={1} aria-valuemax={steps.length} aria-valuenow={step + 1} aria-label={`${step + 1} / ${steps.length}`}>{steps.map((item, index) => <span key={item} className={`h-1.5 flex-1 rounded-full ${index <= step ? "bg-primary" : "bg-border"}`} />)}</div>
        <div className="flex size-12 items-center justify-center rounded-lg bg-primary/10 text-primary"><Icon className="size-6" aria-hidden="true" /></div>
        <h1 className="mt-5 text-2xl font-semibold tracking-tight">{t(title)}</h1>
        <p className="mt-2 max-w-xl text-sm leading-relaxed text-muted-foreground">{t(body)}</p>
        <div className="mt-7 min-h-48 rounded-lg border border-border bg-card p-5">
          {current === "provider" && <div className="space-y-3"><div className="flex items-center justify-between gap-3"><span className="text-sm font-medium">ASR</span><ReadinessBadge state={apiStatus.asr.state} t={t} /></div><p className="text-sm leading-relaxed text-muted-foreground">{t("setup.llmOptional")}</p><Button type="button" variant="outline" onClick={onOpenSettings}><KeyRound className="size-4" aria-hidden="true" />{t("setup.openSettings")}</Button></div>}
          {current === "permission" && <PermissionList permissions={permissions} locale={locale} t={t} onRefresh={onRefreshPermissions} />}
          {current === "hotkey" && <div className="max-w-xs"><label className="mb-1.5 block text-xs font-medium">{t("hotkey.title")}</label><Select value={hotkey} onValueChange={setHotkey}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{HOTKEYS.map((key) => <SelectItem key={key} value={key}>{localizeHotkey(locale, key)}</SelectItem>)}</SelectContent></Select><p className="mt-3 text-sm text-muted-foreground">{t("hotkey.hint.smart")}</p></div>}
          {current === "welcome" && <div className="flex items-center gap-3 text-sm text-muted-foreground"><Mic2 className="size-5 text-primary" aria-hidden="true" />{t("setup.hotkey")}</div>}
          {current === "finish" && <div className="flex items-center gap-3 text-sm"><Check className="size-5 text-success" aria-hidden="true" />{t("onboarding.finishBody")}</div>}
        </div>
        <div className="mt-8 flex items-center justify-between gap-3"><Button type="button" variant="ghost" onClick={() => setStep((value) => Math.max(0, value - 1))} disabled={step === 0 || saving}><ArrowLeft className="size-4" aria-hidden="true" />{t("onboarding.back")}</Button><Button type="button" onClick={() => step === steps.length - 1 ? void complete() : setStep((value) => value + 1)} disabled={saving}>{step === steps.length - 1 ? t("onboarding.finish") : t("onboarding.next")}<ArrowRight className="size-4" aria-hidden="true" /></Button></div>
      </main>
    </div>
  );
}

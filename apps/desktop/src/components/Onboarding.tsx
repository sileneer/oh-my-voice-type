import { useState, type FormEvent } from "react";
import { ArrowLeft, ArrowRight, AudioLines, Check, KeyRound, Keyboard, Loader2, ShieldCheck } from "lucide-react";
import { api, type ApiStatus, type Settings } from "../lib/api";
import { localizeHotkey, type Locale, type TFunc } from "../i18n";
import { asrPresets, findAsrPreset, resolvePresetKeyUrl } from "../lib/provider-presets";
import { cn } from "../lib/utils";
import { Button } from "../components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { ReadinessBadge } from "./app/ReadinessBadge";
import { PermissionList } from "./app/PermissionList";
import { ApiKeyField } from "./ProviderProfiles";
import { ModelPicker } from "./ModelPicker";

const HOTKEYS = ["fn", "right-opt", "right-cmd", "right-alt", "right-ctrl", "left-alt", "left-ctrl", "f6", "f7", "f8", "f9", "f10"];
const steps = ["welcome", "provider", "permission", "hotkey", "finish"] as const;

export default function Onboarding({
  settings,
  permissions,
  apiStatus,
  locale,
  t,
  onOpenSettings,
  onRefreshPermissions,
  onChanged,
  onSave,
  onFinish,
}: {
  settings: Settings;
  permissions: Record<string, string>;
  apiStatus: ApiStatus;
  locale: Locale;
  t: TFunc;
  onOpenSettings: () => void;
  onRefreshPermissions: () => void;
  onChanged: () => Promise<void>;
  onSave: (settings: Settings) => Promise<void>;
  onFinish: () => void;
}) {
  const [step, setStep] = useState(0);
  const [hotkey, setHotkey] = useState(settings.hotkey.dictate);
  const [saving, setSaving] = useState(false);

  // Inline quick ASR setup state for Step 2
  const [presetId, setPresetId] = useState("");
  const [regionUrl, setRegionUrl] = useState<string | null>(null);
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [providerBusy, setProviderBusy] = useState(false);
  const [providerError, setProviderError] = useState<string | null>(null);
  const [providerSaved, setProviderSaved] = useState(false);

  const selectedPreset = findAsrPreset(presetId);
  const effectiveBaseUrl = regionUrl ?? selectedPreset?.baseUrl ?? null;
  const activeAsrProfile = settings.asr_profiles.find((item) => item.id === settings.active_asr_profile_id);

  const saveQuickAsr = async (event: FormEvent) => {
    event.preventDefault();
    if (!selectedPreset || !apiKey.trim() || providerBusy) return;
    const effectiveModel = model.trim() || selectedPreset.model;
    setProviderBusy(true);
    setProviderError(null);
    setProviderSaved(false);
    try {
      await api.saveAsrProfile(
        {
          id: "",
          label: selectedPreset.label,
          engine: selectedPreset.engine,
          base_url: effectiveBaseUrl,
          model: effectiveModel,
          extra_options: selectedPreset.extraOptions
            ? selectedPreset.extraOptions.map(([k, v]) => [k, v])
            : [],
          preset: selectedPreset.id,
          credential_id: null,
          credential_hint: null,
          allow_insecure_http: false,
        },
        apiKey.trim(),
        false,
      );
      setApiKey("");
      setProviderSaved(true);
      await onChanged();
    } catch (error) {
      setProviderError(String(error));
    } finally {
      setProviderBusy(false);
    }
  };

  const complete = async () => {
    setSaving(true);
    try {
      await onSave({
        ...settings,
        hotkey: { ...settings.hotkey, dictate: hotkey },
        ui: { ...settings.ui, onboarding_completed: true },
      });
      onFinish();
    } finally {
      setSaving(false);
    }
  };
  const skip = async () => {
    setSaving(true);
    try {
      await onSave({ ...settings, ui: { ...settings.ui, onboarding_completed: true } });
      onFinish();
    } finally {
      setSaving(false);
    }
  };
  const current = steps[step];
  const Icon =
    current === "provider"
      ? KeyRound
      : current === "permission"
        ? ShieldCheck
        : current === "hotkey"
          ? Keyboard
          : current === "finish"
            ? Check
            : AudioLines;
  const title = `onboarding.${current}Title` as const;
  const body = `onboarding.${current}Body` as const;

  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      <header className="flex items-center justify-between border-b border-border px-6 py-4">
        <div className="flex items-center gap-2.5">
          <AudioLines className="size-5 text-primary" aria-hidden="true" />
          <span className="text-sm font-semibold">oh-my-voice-type</span>
        </div>
        <Button type="button" variant="ghost" size="sm" onClick={() => void skip()} disabled={saving || providerBusy}>
          {t("onboarding.skip")}
        </Button>
      </header>
      <main className="min-h-0 flex-1 overflow-y-auto px-6 py-8">
        <div className="mx-auto flex min-h-full w-full max-w-2xl flex-col justify-center">
        <div
          className="mb-8 flex shrink-0 gap-2"
          role="progressbar"
          aria-valuemin={1}
          aria-valuemax={steps.length}
          aria-valuenow={step + 1}
          aria-label={`${step + 1} / ${steps.length}`}
        >
          {steps.map((item, index) => (
            <span key={item} className={`h-1.5 flex-1 rounded-full ${index <= step ? "bg-primary" : "bg-border"}`} />
          ))}
        </div>
        <p className="-mt-5 mb-6 shrink-0 text-xs text-muted-foreground">
          {t("onboarding.stepOf", { n: step + 1, total: steps.length })}
        </p>
        <div className="flex size-12 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <Icon className="size-6" aria-hidden="true" />
        </div>
        <h1 className="mt-5 shrink-0 text-2xl font-semibold tracking-tight">{t(title)}</h1>
        <p className="mt-2 max-w-xl shrink-0 text-sm leading-relaxed text-muted-foreground">{t(body)}</p>
        <div className="mt-7 shrink-0 rounded-lg border border-border bg-card p-5">
          {current === "provider" && (
            <div className="space-y-4">
              <div className="flex items-center justify-between gap-3">
                <span className="text-sm font-medium">{t("nav.asr")}</span>
                <ReadinessBadge state={apiStatus.asr.state} t={t} />
              </div>

              {apiStatus.asr.state === "ready" && activeAsrProfile ? (
                <div className="space-y-3">
                  <div className="rounded-md border border-success/25 bg-success/10 px-3.5 py-3 text-sm text-foreground">
                    <p className="font-medium">
                      {t("onboarding.providerConfigured", { label: activeAsrProfile.label })}
                    </p>
                    {activeAsrProfile.model && (
                      <p className="mt-1 font-mono text-xs text-muted-foreground">{activeAsrProfile.model}</p>
                    )}
                  </div>
                  {providerSaved && (
                    <p className="text-xs text-success" role="status">{t("onboarding.providerSaved")}</p>
                  )}
                  <p className="text-xs leading-relaxed text-muted-foreground">{t("setup.llmOptional")}</p>
                  <Button type="button" variant="outline" size="sm" onClick={onOpenSettings}>
                    <KeyRound className="size-4" aria-hidden="true" />
                    {t("onboarding.providerAdvanced")}
                  </Button>
                </div>
              ) : (
                <form className="space-y-3.5" onSubmit={(event) => void saveQuickAsr(event)}>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("onboarding.providerQuickHint")}
                  </p>
                  <div className="space-y-1.5">
                    <label className="block text-xs font-medium">{t("profiles.provider")}</label>
                    <Select
                      value={presetId}
                      disabled={providerBusy}
                      onValueChange={(value) => {
                        const next = findAsrPreset(value);
                        setPresetId(value);
                        setRegionUrl(next?.baseUrl ?? null);
                        setModel(next?.model ?? "");
                        setApiKey("");
                        setProviderError(null);
                      }}
                    >
                      <SelectTrigger aria-label={t("profiles.provider")}>
                        <SelectValue placeholder={t("profiles.selectProvider")} />
                      </SelectTrigger>
                      <SelectContent>
                        {asrPresets.map((preset) => (
                          <SelectItem key={preset.id} value={preset.id}>
                            {preset.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>

                  {selectedPreset?.endpoints && (
                    <div className="grid grid-cols-2 gap-2" role="group" aria-label={t("profiles.region")}>
                      {selectedPreset.endpoints.map((option) => {
                        const active = effectiveBaseUrl === option.baseUrl;
                        return (
                          <button
                            key={option.id}
                            type="button"
                            disabled={providerBusy}
                            aria-pressed={active}
                            onClick={() => setRegionUrl(option.baseUrl)}
                            className={cn(
                              "rounded-md border px-3 py-1.5 text-left text-xs transition-colors",
                              active
                                ? "border-primary/50 bg-primary/[0.06] font-medium text-foreground"
                                : "border-border bg-background text-muted-foreground hover:text-foreground",
                            )}
                          >
                            {t(option.labelKey)}
                          </button>
                        );
                      })}
                    </div>
                  )}

                  {selectedPreset && (
                    <>
                      <ApiKeyField
                        apiKey={apiKey}
                        savedHint={null}
                        hasSavedKey={false}
                        keyUrl={resolvePresetKeyUrl(selectedPreset, effectiveBaseUrl)}
                        busy={providerBusy}
                        t={t}
                        onChange={setApiKey}
                      />
                      <ModelPicker
                        role="asr"
                        protocol="open_ai"
                        baseUrl={effectiveBaseUrl}
                        allowInsecureHttp={false}
                        apiKey={apiKey}
                        hasSavedKey={false}
                        value={model}
                        fallbackModels={selectedPreset.models}
                        preferredDefault={selectedPreset.model}
                        disabled={providerBusy}
                        t={t}
                        onChange={setModel}
                      />
                    </>
                  )}

                  {providerError && (
                    <p className="text-xs text-destructive" role="alert">{providerError}</p>
                  )}

                  <div className="flex flex-wrap items-center justify-between gap-2 pt-1">
                    <Button type="button" variant="ghost" size="sm" onClick={onOpenSettings} disabled={providerBusy}>
                      <KeyRound className="size-4" aria-hidden="true" />
                      {t("onboarding.providerAdvanced")}
                    </Button>
                    {selectedPreset && (
                      <Button type="submit" size="sm" disabled={providerBusy || !apiKey.trim()}>
                        {providerBusy && <Loader2 className="animate-spin" aria-hidden="true" />}
                        {providerBusy ? t("settings.saving") : t("common.save")}
                      </Button>
                    )}
                  </div>
                </form>
              )}
            </div>
          )}
          {current === "permission" && (
            <PermissionList permissions={permissions} locale={locale} t={t} onRefresh={onRefreshPermissions} />
          )}
          {current === "hotkey" && (
            <div className="max-w-xs">
              <label className="mb-1.5 block text-xs font-medium">{t("hotkey.title")}</label>
              <Select value={hotkey} onValueChange={setHotkey}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {HOTKEYS.map((key) => (
                    <SelectItem key={key} value={key}>{localizeHotkey(locale, key)}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="mt-3 text-sm text-muted-foreground">{t("hotkey.hint.smart")}</p>
            </div>
          )}
          {current === "welcome" && (
            <ol className="space-y-3">
              {([
                [KeyRound, "onboarding.providerTitle", "onboarding.providerBody"],
                [ShieldCheck, "onboarding.permissionTitle", "onboarding.permissionBody"],
                [Keyboard, "onboarding.hotkeyTitle", "onboarding.hotkeyBody"],
              ] as const).map(([StepIcon, stepTitle, stepBody], index) => (
                <li key={stepTitle} className="flex items-start gap-3">
                  <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
                    <StepIcon className="size-4" aria-hidden="true" />
                  </span>
                  <div>
                    <p className="text-sm font-medium">{index + 1}. {t(stepTitle)}</p>
                    <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">{t(stepBody)}</p>
                  </div>
                </li>
              ))}
            </ol>
          )}
          {current === "finish" && (
            <div className="flex items-center gap-3 text-sm">
              <Check className="size-5 text-success" aria-hidden="true" />
              {t("onboarding.finishBody")}
            </div>
          )}
        </div>
        <div className="mt-8 flex shrink-0 items-center justify-between gap-3">
          <Button
            type="button"
            variant="ghost"
            onClick={() => setStep((value) => Math.max(0, value - 1))}
            disabled={step === 0 || saving || providerBusy}
          >
            <ArrowLeft className="size-4" aria-hidden="true" />
            {t("onboarding.back")}
          </Button>
          <Button
            type="button"
            onClick={() => (step === steps.length - 1 ? void complete() : setStep((value) => value + 1))}
            disabled={saving || providerBusy}
          >
            {step === steps.length - 1 ? t("onboarding.finish") : t("onboarding.next")}
            <ArrowRight className="size-4" aria-hidden="true" />
          </Button>
        </div>
        </div>
      </main>
    </div>
  );
}

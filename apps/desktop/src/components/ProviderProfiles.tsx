import { useId, useState, type FormEvent, type ReactNode } from "react";
import { isTauri } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  Download,
  ExternalLink,
  Eye,
  EyeOff,
  FlaskConical,
  KeyRound,
  Loader2,
  Mic,
  Pencil,
  Play,
  Plus,
  RotateCcw,
  Sparkles,
  Trash2,
  Upload,
  XCircle,
} from "lucide-react";
import {
  api,
  type ApiStatus,
  type AsrProfile,
  type LlmProfile,
  type ProfileRole,
  type ProfileTestResult,
  type Settings,
} from "../lib/api";
import type { TFunc } from "../i18n";
import { cn } from "../lib/utils";
import {
  asrPresets,
  findAsrPreset,
  findLlmPreset,
  llmPresets,
  resolvePresetKeyUrl,
  type AsrPreset,
  type EndpointOption,
  type LlmPreset,
} from "../lib/provider-presets";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "./ui/alert-dialog";
import { Badge } from "./ui/badge";
import { Button, buttonVariants } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./ui/dialog";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { RadioGroup, RadioGroupItem } from "./ui/radio-group";
import { Select, SelectContent, SelectItem, SelectSeparator, SelectTrigger, SelectValue } from "./ui/select";
import { ReadinessBadge } from "./app/ReadinessBadge";
import { ModelPicker } from "./ModelPicker";

type AnyProfile = AsrProfile | LlmProfile;
type EditorMode = "new" | "edit" | "copy";

interface OriginalKeyRef {
  preset: string | null;
  base_url: string | null;
  credential_id: string | null;
  credential_hint: string | null;
}

type EditorState =
  | {
      role: "asr";
      profile: AsrProfile;
      selection: string;
      apiKey: string;
      mode: EditorMode;
      original?: OriginalKeyRef;
    }
  | {
      role: "llm";
      profile: LlmProfile;
      selection: string;
      apiKey: string;
      mode: EditorMode;
      original?: OriginalKeyRef;
    };

type DeleteTarget = { role: ProfileRole; profile: AnyProfile };
type TestTarget = { role: ProfileRole; id: string };

interface Props {
  settings: Settings;
  apiStatus: ApiStatus;
  t: TFunc;
  onChanged: () => Promise<void>;
  onError: (message: string) => void;
}

const iconButtonClass = "h-8 w-8 text-muted-foreground";
const compactButtonClass = "h-8 px-2.5";

const quickStartAsrIds = ["groq", "mistral", "openai", "siliconflow"];
const quickStartLlmIds = ["dashscope", "deepseek", "gemini", "openai"];

function profileFromAsrPreset(preset: AsrPreset): AsrProfile {
  return {
    id: "",
    label: preset.label,
    engine: preset.engine,
    base_url: preset.baseUrl,
    model: preset.model,
    extra_options: preset.extraOptions ? preset.extraOptions.map(([k, v]) => [k, v]) : [],
    preset: preset.id,
    credential_id: null,
    credential_hint: null,
    allow_insecure_http: false,
  };
}

function profileFromLlmPreset(preset: LlmPreset): LlmProfile {
  return {
    id: "",
    label: preset.label,
    engine: preset.engine,
    format: preset.format,
    base_url: preset.baseUrl,
    model: preset.model,
    preset: preset.id,
    credential_id: null,
    credential_hint: null,
    allow_insecure_http: false,
  };
}

export default function ProviderProfiles({
  settings,
  apiStatus,
  t,
  onChanged,
  onError,
}: Props) {
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [busyRole, setBusyRole] = useState<ProfileRole | null>(null);
  const [testBusy, setTestBusy] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, ProfileTestResult>>({});
  const [transferBusy, setTransferBusy] = useState(false);
  const [transferMessage, setTransferMessage] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget | null>(null);
  const [testTarget, setTestTarget] = useState<TestTarget | null>(null);
  const [transferConfirmation, setTransferConfirmation] = useState<"export" | "import" | null>(null);
  const [hostChange, setHostChange] = useState<{ host: string } | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const transferTitleId = useId();

  const reportError = (error: unknown) => {
    const message = String(error);
    setErrorMessage(message);
    onError(message);
  };

  const newAsr = (presetId?: string) => {
    setErrorMessage(null);
    const preset = findAsrPreset(presetId);
    setEditor({
      mode: "new",
      role: "asr",
      selection: preset ? preset.id : "",
      apiKey: "",
      profile: preset
        ? profileFromAsrPreset(preset)
        : {
            id: "",
            label: "",
            engine: "openai_compatible",
            base_url: null,
            model: null,
            extra_options: [],
            preset: null,
            credential_id: null,
            credential_hint: null,
            allow_insecure_http: false,
          },
    });
  };

  const newLlm = (presetId?: string) => {
    setErrorMessage(null);
    const preset = findLlmPreset(presetId);
    setEditor({
      mode: "new",
      role: "llm",
      selection: preset ? preset.id : "",
      apiKey: "",
      profile: preset
        ? profileFromLlmPreset(preset)
        : {
            id: "",
            label: "",
            engine: "custom",
            format: "open_ai_chat",
            base_url: null,
            model: null,
            preset: null,
            credential_id: null,
            credential_hint: null,
            allow_insecure_http: false,
          },
    });
  };

  const copyProfile = (profile: AnyProfile) => {
    setErrorMessage(null);
    if ("format" in profile) {
      setEditor({
        mode: "copy",
        role: "llm",
        selection: profile.preset ?? "__custom__",
        apiKey: "",
        profile: {
          ...profile,
          id: "",
          label: t("profiles.copyLabel", { label: profile.label }),
          credential_id: null,
          credential_hint: null,
        },
      });
    } else {
      setEditor({
        mode: "copy",
        role: "asr",
        selection: profile.preset ?? "__custom__",
        apiKey: "",
        profile: {
          ...profile,
          id: "",
          label: t("profiles.copyLabel", { label: profile.label }),
          extra_options: profile.extra_options.map(([key, value]) => [key, value]),
          credential_id: null,
          credential_hint: null,
        },
      });
    }
  };

  const persistProfile = async (confirmHostChange: boolean) => {
    if (!editor) return;
    const { role, profile, apiKey } = editor;
    const normalized = {
      ...profile,
      label: profile.label.trim(),
      base_url: profile.base_url?.trim() || null,
      model: profile.model?.trim() || null,
    };
    const secret = apiKey.trim() || null;
    setBusyRole(role);
    setErrorMessage(null);
    try {
      if (role === "llm" && "format" in normalized) {
        await api.saveLlmProfile(normalized, secret, confirmHostChange);
      } else if (role === "asr" && "extra_options" in normalized) {
        await api.saveAsrProfile(normalized, secret, confirmHostChange);
      }
      setEditor(null);
      await onChanged();
    } catch (error) {
      reportError(error);
    } finally {
      setBusyRole(null);
    }
  };

  const saveProfile = () => {
    if (!editor || busyRole) return;
    const { role, profile, apiKey } = editor;
    const existing = profile.id
      ? role === "asr"
        ? settings.asr_profiles.find((item) => item.id === profile.id)
        : settings.llm_profiles.find((item) => item.id === profile.id)
      : undefined;
    const hostChanged = Boolean(
      existing && endpointHost(existing.base_url) !== endpointHost(profile.base_url),
    );
    if (existing && (existing.credential_id || profile.credential_id || apiKey.trim()) && hostChanged) {
      const host = endpointHost(profile.base_url)
        ?? profile.base_url?.trim()
        ?? t("profiles.defaultEndpoint");
      setHostChange({ host });
      return;
    }
    void persistProfile(false);
  };

  const activateProfile = async (role: ProfileRole, id: string) => {
    setBusyRole(role);
    setErrorMessage(null);
    try {
      await api.setActiveProfile(role, id);
      await onChanged();
    } catch (error) {
      reportError(error);
    } finally {
      setBusyRole(null);
    }
  };

  const deleteProfile = async (role: ProfileRole, profile: AnyProfile) => {
    setDeleteTarget(null);
    setBusyRole(role);
    setErrorMessage(null);
    try {
      await api.deleteProfile(role, profile.id);
      await onChanged();
      setEditor((current) => (current?.profile.id === profile.id ? null : current));
      setTestResults((current) => {
        const next = { ...current };
        delete next[`${role}:${profile.id}`];
        return next;
      });
    } catch (error) {
      reportError(error);
    } finally {
      setBusyRole(null);
    }
  };

  const testProfile = async (role: ProfileRole, id: string, mode: "check" | "request") => {
    setTestTarget(null);
    const key = `${role}:${id}`;
    setTestBusy(key);
    setErrorMessage(null);
    try {
      const result = await api.testProfile(role, id, mode);
      setTestResults((current) => ({ ...current, [key]: result }));
    } catch (error) {
      const message = String(error);
      setTestResults((current) => ({
        ...current,
        [key]: { ok: false, verified: false, message, elapsed_ms: 0 },
      }));
      onError(message);
    } finally {
      setTestBusy(null);
    }
  };

  const transferConfig = async (mode: "export" | "import") => {
    setTransferConfirmation(null);
    setTransferBusy(true);
    setTransferMessage(null);
    setErrorMessage(null);
    try {
      if (mode === "export") {
        const exported = await api.exportConfig();
        if (exported) setTransferMessage(t("profiles.transfer.exported"));
      } else {
        const imported = await api.importConfig();
        if (imported) {
          await onChanged();
          setTransferMessage(t("profiles.transfer.imported"));
        }
      }
    } catch (error) {
      reportError(error);
    } finally {
      setTransferBusy(false);
    }
  };

  const editorTitle = editor
    ? editor.mode === "edit"
      ? `${t("common.edit")} · ${editor.profile.label || t(editor.role === "asr" ? "settings.asr.title" : "settings.llm.title")}`
      : editor.mode === "copy"
        ? t("common.copy")
        : t(editor.role === "asr" ? "profiles.newAsr" : "profiles.newLlm")
    : "";

  const deleteQuestion = deleteTarget
    ? (deleteTarget.role === "asr"
      ? settings.active_asr_profile_id
      : settings.active_llm_profile_id) === deleteTarget.profile.id
      ? t("profiles.confirmDeleteActive", { label: deleteTarget.profile.label })
      : t("profiles.confirmDelete", { label: deleteTarget.profile.label })
    : "";

  return (
    <div className="space-y-5">
      {errorMessage && !editor && <InlineError message={errorMessage} />}

      <ProfileSection
        role="asr"
        profiles={settings.asr_profiles}
        activeId={settings.active_asr_profile_id}
        readiness={apiStatus.asr}
        busy={busyRole === "asr"}
        testBusy={testBusy}
        testResults={testResults}
        quickStartPresets={asrPresets.filter((item) => quickStartAsrIds.includes(item.id))}
        t={t}
        onNew={newAsr}
        onEdit={(profile) => {
          setErrorMessage(null);
          setEditor({
            role: "asr",
            mode: "edit",
            selection: profile.preset ?? "__custom__",
            apiKey: "",
            profile: { ...profile, extra_options: profile.extra_options.map(([k, v]) => [k, v]) },
            original: {
              preset: profile.preset,
              base_url: profile.base_url,
              credential_id: profile.credential_id,
              credential_hint: profile.credential_hint,
            },
          });
        }}
        onCopy={copyProfile}
        onDelete={(profile) => setDeleteTarget({ role: "asr", profile })}
        onActivate={(id) => void activateProfile("asr", id)}
        onTest={(id, mode) =>
          mode === "request"
            ? setTestTarget({ role: "asr", id })
            : void testProfile("asr", id, mode)
        }
      />

      <ProfileSection
        role="llm"
        profiles={settings.llm_profiles}
        activeId={settings.active_llm_profile_id}
        readiness={apiStatus.llm}
        cleanupDisabled={!settings.dictation.cleanup_enabled}
        busy={busyRole === "llm"}
        testBusy={testBusy}
        testResults={testResults}
        quickStartPresets={llmPresets.filter((item) => quickStartLlmIds.includes(item.id))}
        t={t}
        onNew={newLlm}
        onEdit={(profile) => {
          setErrorMessage(null);
          setEditor({
            role: "llm",
            mode: "edit",
            selection: profile.preset ?? "__custom__",
            apiKey: "",
            profile: { ...profile },
            original: {
              preset: profile.preset,
              base_url: profile.base_url,
              credential_id: profile.credential_id,
              credential_hint: profile.credential_hint,
            },
          });
        }}
        onCopy={copyProfile}
        onDelete={(profile) => setDeleteTarget({ role: "llm", profile })}
        onActivate={(id) => void activateProfile("llm", id)}
        onTest={(id, mode) =>
          mode === "request"
            ? setTestTarget({ role: "llm", id })
            : void testProfile("llm", id, mode)
        }
      />

      <section
        className="rounded-lg border border-border bg-card p-5"
        aria-labelledby={transferTitleId}
      >
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="min-w-0">
            <h2 id={transferTitleId} className="text-sm font-semibold text-foreground">
              {t("profiles.transfer.title")}
            </h2>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
              {t("profiles.transfer.hint")}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={transferBusy}
              onClick={() => setTransferConfirmation("export")}
            >
              {transferBusy ? <Loader2 className="animate-spin" aria-hidden="true" /> : <Download aria-hidden="true" />}
              {t("profiles.transfer.export")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={transferBusy}
              onClick={() => setTransferConfirmation("import")}
            >
              {transferBusy ? <Loader2 className="animate-spin" aria-hidden="true" /> : <Upload aria-hidden="true" />}
              {t("profiles.transfer.import")}
            </Button>
          </div>
        </div>
        {transferBusy && <BusyLabel t={t} />}
        {transferMessage && (
          <p className="mt-2.5 text-xs text-muted-foreground" role="status">
            {transferMessage}
          </p>
        )}
      </section>

      <Dialog
        open={Boolean(editor)}
        onOpenChange={(open) => {
          if (!open && !busyRole) setEditor(null);
        }}
      >
        <DialogContent
          className="max-h-[calc(100vh-2rem)] max-w-xl overflow-y-auto p-6"
          onEscapeKeyDown={(event) => {
            if (busyRole) event.preventDefault();
          }}
          onInteractOutside={(event) => {
            if (busyRole) event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle className="text-base">{editorTitle}</DialogTitle>
            <DialogDescription>{t("profiles.hint")}</DialogDescription>
          </DialogHeader>
          {errorMessage && <InlineError message={errorMessage} />}
          {editor?.role === "asr" && (
            <AsrProfileEditor
              profile={editor.profile}
              selection={editor.selection}
              original={editor.original}
              apiKey={editor.apiKey}
              busy={busyRole === "asr"}
              t={t}
              onChange={(profile, selection) =>
                setEditor((current) => {
                  if (!current || current.role !== "asr") return current;
                  const nextSelection = selection ?? current.selection;
                  const presetChanged = profile.preset !== current.profile.preset;
                  return {
                    ...current,
                    selection: nextSelection,
                    apiKey: presetChanged ? "" : current.apiKey,
                    profile,
                  };
                })
              }
              onApiKeyChange={(apiKey) =>
                setEditor((current) => (current?.role === "asr" ? { ...current, apiKey } : current))
              }
              onSave={saveProfile}
              onCancel={() => setEditor(null)}
            />
          )}
          {editor?.role === "llm" && (
            <LlmProfileEditor
              profile={editor.profile}
              selection={editor.selection}
              original={editor.original}
              apiKey={editor.apiKey}
              busy={busyRole === "llm"}
              t={t}
              onChange={(profile, selection) =>
                setEditor((current) => {
                  if (!current || current.role !== "llm") return current;
                  const nextSelection = selection ?? current.selection;
                  const presetChanged = profile.preset !== current.profile.preset;
                  return {
                    ...current,
                    selection: nextSelection,
                    apiKey: presetChanged ? "" : current.apiKey,
                    profile,
                  };
                })
              }
              onApiKeyChange={(apiKey) =>
                setEditor((current) => (current?.role === "llm" ? { ...current, apiKey } : current))
              }
              onSave={saveProfile}
              onCancel={() => setEditor(null)}
            />
          )}
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={Boolean(hostChange)}
        onOpenChange={(open) => {
          if (!open && !busyRole) setHostChange(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("profiles.title")}</AlertDialogTitle>
            <AlertDialogDescription>
              {hostChange ? t("profiles.confirmHost", { host: hostChange.host }) : ""}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={Boolean(busyRole)}>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={Boolean(busyRole)}
              className={buttonVariants()}
              onClick={() => {
                setHostChange(null);
                void persistProfile(true);
              }}
            >
              {busyRole && <Loader2 className="animate-spin" aria-hidden="true" />}
              {t("common.save")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={Boolean(deleteTarget)}
        onOpenChange={(open) => {
          if (!open && !busyRole) setDeleteTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("common.delete")}</AlertDialogTitle>
            <AlertDialogDescription>{deleteQuestion}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={Boolean(busyRole)}>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={Boolean(busyRole)}
              className={buttonVariants({ variant: "destructive" })}
              onClick={() => {
                if (deleteTarget) void deleteProfile(deleteTarget.role, deleteTarget.profile);
              }}
            >
              {busyRole && <Loader2 className="animate-spin" aria-hidden="true" />}
              {t("common.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={Boolean(testTarget)}
        onOpenChange={(open) => {
          if (!open && !testBusy) setTestTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("profiles.test.request")}</AlertDialogTitle>
            <AlertDialogDescription>{t("profiles.test.confirmRequest")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={Boolean(testBusy)}>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={Boolean(testBusy)}
              className={buttonVariants()}
              onClick={() => {
                if (testTarget) void testProfile(testTarget.role, testTarget.id, "request");
              }}
            >
              {testBusy && <Loader2 className="animate-spin" aria-hidden="true" />}
              {t("profiles.test.request")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={Boolean(transferConfirmation)}
        onOpenChange={(open) => {
          if (!open && !transferBusy) setTransferConfirmation(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("profiles.transfer.title")}</AlertDialogTitle>
            <AlertDialogDescription>
              {transferConfirmation === "import"
                ? t("profiles.transfer.importConfirm")
                : t("profiles.transfer.exportConfirm")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={transferBusy}>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={transferBusy}
              className={buttonVariants()}
              onClick={() => {
                if (transferConfirmation) void transferConfig(transferConfirmation);
              }}
            >
              {transferBusy && <Loader2 className="animate-spin" aria-hidden="true" />}
              {t(transferConfirmation === "import" ? "profiles.transfer.import" : "profiles.transfer.export")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

interface ProfileSectionProps<T extends AnyProfile> {
  role: ProfileRole;
  profiles: T[];
  activeId: string | null;
  readiness: ApiStatus[ProfileRole];
  cleanupDisabled?: boolean;
  busy: boolean;
  testBusy: string | null;
  testResults: Record<string, ProfileTestResult>;
  quickStartPresets: Array<{ id: string; label: string }>;
  t: TFunc;
  onNew: (presetId?: string) => void;
  onEdit: (profile: T) => void;
  onCopy: (profile: T) => void;
  onDelete: (profile: T) => void;
  onActivate: (id: string) => void;
  onTest: (id: string, mode: "check" | "request") => void;
}

function ProfileSection<T extends AnyProfile>({
  role,
  profiles,
  activeId,
  readiness,
  cleanupDisabled = false,
  busy,
  testBusy,
  testResults,
  quickStartPresets,
  t,
  onNew,
  onEdit,
  onCopy,
  onDelete,
  onActivate,
  onTest,
}: ProfileSectionProps<T>) {
  const isAsr = role === "asr";
  const titleId = useId();
  const title = t(isAsr ? "settings.asr.title" : "settings.llm.title");
  const hasReadinessError = readiness.state === "vault_error" || readiness.state === "invalid_config";

  return (
    <section className="rounded-lg border border-border bg-card" aria-labelledby={titleId}>
      <div className="flex flex-wrap items-start justify-between gap-3 border-b border-border p-5">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2.5">
            <h2 id={titleId} className="text-base font-semibold text-foreground">{title}</h2>
            <ReadinessBadge state={readiness.state} t={t} compact />
          </div>
          <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
            {t(isAsr ? "settings.asr.caps" : "settings.llm.optional")}
          </p>
        </div>
        <Button type="button" variant="outline" size="sm" onClick={() => onNew()} disabled={busy}>
          {busy ? <Loader2 className="animate-spin" aria-hidden="true" /> : <Plus aria-hidden="true" />}
          {t(isAsr ? "profiles.newAsr" : "profiles.newLlm")}
        </Button>
      </div>

      <div className="space-y-3 p-5" aria-busy={busy || Boolean(testBusy)}>
        {cleanupDisabled && (
          <p className="rounded-md border border-border bg-muted/50 px-3 py-2 text-xs leading-relaxed text-muted-foreground">
            {t("profiles.cleanupOffNotice")}
          </p>
        )}

        {readiness.message && (
          <p
            className={cn(
              "rounded-md border px-3 py-2 text-xs leading-relaxed",
              hasReadinessError
                ? "border-destructive/30 bg-destructive/10 text-destructive"
                : "border-warning/30 bg-warning/10 text-warning",
            )}
            role={hasReadinessError ? "alert" : "status"}
          >
            {readiness.message}
          </p>
        )}

        {profiles.length === 0 ? (
          <div className="flex flex-col items-center gap-3 rounded-lg border border-dashed border-border bg-background/40 px-4 py-6 text-center">
            <div className="flex size-10 items-center justify-center rounded-full bg-muted text-muted-foreground">
              {isAsr ? <Mic className="size-4" aria-hidden="true" /> : <Sparkles className="size-4" aria-hidden="true" />}
            </div>
            <p className="text-sm text-muted-foreground">{t(isAsr ? "profiles.emptyAsr" : "profiles.emptyLlm")}</p>
            <div className="flex flex-col items-center gap-2 pt-1">
              <span className="text-xs text-muted-foreground">{t("profiles.quickStart")}</span>
              <div className="flex flex-wrap items-center justify-center gap-1.5">
                {quickStartPresets.map((preset) => (
                  <Button
                    key={preset.id}
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-7 px-2.5 text-xs"
                    disabled={busy}
                    onClick={() => onNew(preset.id)}
                  >
                    {preset.label}
                  </Button>
                ))}
              </div>
            </div>
          </div>
        ) : (
          <RadioGroup
            value={activeId ?? ""}
            disabled={busy}
            onValueChange={(value) => {
              if (value) onActivate(value);
            }}
            aria-label={title}
            className="space-y-2.5"
          >
            {profiles.map((profile) => {
              const active = profile.id === activeId;
              const radioId = `${titleId}-${profile.id}`;
              const credentialLabel = profile.credential_hint
                ? t("profiles.keySaved", { hint: profile.credential_hint })
                : profile.credential_id
                  ? t("profiles.credential.missing")
                  : t("profiles.credential.none");
              const missingKey = !profile.credential_id;
              const testKey = `${role}:${profile.id}`;
              const result = testResults[testKey];
              const isTesting = testBusy === testKey;
              return (
                <div
                  key={profile.id}
                  className={cn(
                    "rounded-lg border p-3.5 transition-colors",
                    active
                      ? "border-primary/45 bg-primary/[0.04]"
                      : "border-border bg-background/50 hover:border-border/90",
                  )}
                >
                  <div className="flex items-start justify-between gap-3">
                    <label
                      htmlFor={radioId}
                      className="flex min-w-0 flex-1 cursor-pointer items-start gap-3"
                    >
                      <RadioGroupItem
                        id={radioId}
                        value={profile.id}
                        className="mt-0.5 shrink-0"
                        aria-label={`${t("profiles.active")}: ${profile.label}`}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-sm font-semibold text-foreground">{profile.label}</span>
                          {active && (
                            <Badge variant="outline" className="border-success/30 bg-success/10 text-[11px] text-success">
                              {t("profiles.active")}
                            </Badge>
                          )}
                          <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-secondary-foreground">
                            {profile.model?.trim() || t("profiles.defaultModel")}
                          </span>
                        </div>
                        <p className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
                          <span className="break-all">{profileEndpoint(profile, t)}</span>
                          <span aria-hidden="true">·</span>
                          <span className={cn(missingKey && "font-medium text-warning")}>{credentialLabel}</span>
                        </p>
                      </div>
                    </label>
                    <div className="flex shrink-0 gap-0.5">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className={iconButtonClass}
                        disabled={busy}
                        onClick={() => onEdit(profile)}
                        aria-label={`${t("common.edit")}: ${profile.label}`}
                        title={t("common.edit")}
                      >
                        <Pencil aria-hidden="true" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className={iconButtonClass}
                        disabled={busy}
                        onClick={() => onCopy(profile)}
                        aria-label={`${t("common.copy")}: ${profile.label}`}
                        title={t("common.copy")}
                      >
                        <Copy aria-hidden="true" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className={cn(iconButtonClass, "hover:text-destructive")}
                        disabled={busy}
                        onClick={() => onDelete(profile)}
                        aria-label={`${t("common.delete")}: ${profile.label}`}
                        title={t("common.delete")}
                      >
                        <Trash2 aria-hidden="true" />
                      </Button>
                    </div>
                  </div>

                  <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-border/80 pt-2.5">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className={compactButtonClass}
                      disabled={Boolean(testBusy) || busy || missingKey}
                      onClick={() => onTest(profile.id, "request")}
                    >
                      {isTesting ? <Loader2 className="animate-spin" aria-hidden="true" /> : <Play aria-hidden="true" />}
                      {isTesting ? t("profiles.test.testing") : t("profiles.test.request")}
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className={cn(compactButtonClass, "text-muted-foreground")}
                      disabled={Boolean(testBusy) || busy || missingKey}
                      onClick={() => onTest(profile.id, "check")}
                    >
                      <FlaskConical aria-hidden="true" />
                      {t("profiles.test.check")}
                    </Button>
                    {result && (
                      <span className="flex min-w-0 flex-wrap items-center gap-1.5 text-xs" role="status" aria-live="polite">
                        {result.ok && result.verified ? (
                          <CheckCircle2 className="size-3.5 shrink-0 text-success" aria-hidden="true" />
                        ) : result.ok ? (
                          <AlertTriangle className="size-3.5 shrink-0 text-warning" aria-hidden="true" />
                        ) : (
                          <XCircle className="size-3.5 shrink-0 text-destructive" aria-hidden="true" />
                        )}
                        <span
                          className={cn(
                            "break-words",
                            result.ok && result.verified
                              ? "text-success"
                              : result.ok
                                ? "text-muted-foreground"
                                : "text-destructive",
                          )}
                        >
                          {result.message}
                        </span>
                        {result.elapsed_ms > 0 && (
                          <span className="tabular-nums text-muted-foreground">
                            · {t("profiles.test.elapsed", { ms: result.elapsed_ms })}
                          </span>
                        )}
                      </span>
                    )}
                  </div>
                </div>
              );
            })}
          </RadioGroup>
        )}
        {busy && <BusyLabel t={t} />}
      </div>
    </section>
  );
}

function AsrProfileEditor({
  profile,
  selection,
  original,
  apiKey,
  busy,
  t,
  onChange,
  onApiKeyChange,
  onSave,
  onCancel,
}: {
  profile: AsrProfile;
  selection: string;
  original?: OriginalKeyRef;
  apiKey: string;
  busy: boolean;
  t: TFunc;
  onChange: (profile: AsrProfile, selection?: string) => void;
  onApiKeyChange: (key: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const preset = findAsrPreset(profile.preset);
  const isCustom = selection === "__custom__";
  const hasSelection = selection !== "";
  const keyUrl = resolvePresetKeyUrl(preset, profile.base_url);
  const providerSwitched = Boolean(original && original.preset !== profile.preset);
  const canUseSavedKeyForModels = Boolean(
    profile.credential_id &&
      (!original || endpointHost(original.base_url) === endpointHost(profile.base_url)),
  );
  const needsEndpoint = profile.engine === "openai_compatible";
  const isCustomized = Boolean(
    preset &&
      (profile.label !== preset.label ||
        profile.engine !== preset.engine ||
        (profile.base_url ?? null) !== preset.baseUrl ||
        (profile.model ?? "") !== preset.model ||
        profile.allow_insecure_http),
  );

  const validationError = (() => {
    if (!hasSelection) return t("profiles.validation.provider");
    if (!profile.label.trim()) return t("profiles.validation.label");
    if (needsEndpoint && !profile.base_url?.trim()) return t("profiles.validation.baseUrl");
    if (profile.base_url?.trim()) {
      const urlError = validateBaseUrl(profile.base_url, profile.allow_insecure_http, t);
      if (urlError) return urlError;
    }
    if (needsEndpoint && !profile.model?.trim()) return t("profiles.validation.model");
    if (!apiKey.trim() && !profile.credential_id) {
      return providerSwitched
        ? t("profiles.validation.newKeyForProvider")
        : t("profiles.validation.apiKey");
    }
    return null;
  })();

  const selectProvider = (value: string) => {
    if (value === "__custom__") {
      const restoreOriginal = original && original.preset === null;
      onChange(
        {
          ...profile,
          preset: null,
          label: isCustom ? profile.label : t("profiles.customAsr"),
          engine: "openai_compatible",
          base_url: null,
          model: null,
          extra_options: [],
          credential_id: restoreOriginal ? original.credential_id : null,
          credential_hint: restoreOriginal ? original.credential_hint : null,
        },
        "__custom__",
      );
      return;
    }
    const next = findAsrPreset(value);
    if (!next) return;
    const restoreOriginal = original && original.preset === next.id;
    onChange(
      {
        ...profile,
        preset: next.id,
        label: next.label,
        engine: next.engine,
        base_url: next.baseUrl,
        model: next.model,
        extra_options: next.extraOptions ? next.extraOptions.map(([k, v]) => [k, v]) : [],
        credential_id: restoreOriginal ? original.credential_id : null,
        credential_hint: restoreOriginal ? original.credential_hint : null,
      },
      next.id,
    );
  };

  return (
    <form
      onSubmit={(event: FormEvent) => {
        event.preventDefault();
        if (!validationError) onSave();
      }}
    >
      <div className="space-y-4">
        <Field label={t("profiles.provider")}>
          {(field) => (
            <Select value={selection} disabled={busy} onValueChange={selectProvider}>
              <SelectTrigger {...field}>
                <SelectValue placeholder={t("profiles.selectProvider")} />
              </SelectTrigger>
              <SelectContent>
                {asrPresets.map((item) => (
                  <SelectItem key={item.id} value={item.id}>
                    {item.label}
                  </SelectItem>
                ))}
                <SelectSeparator />
                <SelectItem value="__custom__">{t("profiles.custom")}</SelectItem>
              </SelectContent>
            </Select>
          )}
        </Field>

        {preset?.endpoints && (
          <RegionSelector
            endpoints={preset.endpoints}
            currentUrl={profile.base_url}
            disabled={busy}
            t={t}
            onSelect={(baseUrl) => onChange({ ...profile, base_url: baseUrl })}
          />
        )}

        {hasSelection && (
          <ApiKeyField
            apiKey={apiKey}
            savedHint={profile.credential_hint}
            hasSavedKey={Boolean(profile.credential_id)}
            keyUrl={keyUrl}
            busy={busy}
            t={t}
            onChange={onApiKeyChange}
          />
        )}

        {preset && (
          <ModelPicker
            role="asr"
            protocol="open_ai"
            baseUrl={profile.base_url}
            allowInsecureHttp={profile.allow_insecure_http}
            apiKey={apiKey}
            profileId={profile.id || null}
            hasSavedKey={canUseSavedKeyForModels}
            value={profile.model ?? ""}
            fallbackModels={preset.models}
            preferredDefault={preset.model}
            disabled={busy}
            t={t}
            onChange={(model) => onChange({ ...profile, model })}
          />
        )}

        {isCustom && (
          <div className="space-y-3 rounded-md border border-border bg-muted/20 p-3.5">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label={t("profiles.label")}>
                {(field) => (
                  <Input
                    {...field}
                    value={profile.label}
                    disabled={busy}
                    onChange={(event) => onChange({ ...profile, label: event.target.value })}
                  />
                )}
              </Field>
              <Field label={t("settings.asr.engine")}>
                {(field) => (
                  <Select
                    value={profile.engine}
                    disabled={busy}
                    onValueChange={(value) => onChange({ ...profile, engine: value as AsrProfile["engine"] })}
                  >
                    <SelectTrigger {...field}><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="openai_compatible">{t("settings.asr.engine.openai")}</SelectItem>
                      <SelectItem value="unisound_u2">{t("settings.asr.engine.u2")}</SelectItem>
                    </SelectContent>
                  </Select>
                )}
              </Field>
              <Field label={t("profiles.baseUrl")} hint={t("profiles.customBaseUrlRequired")}>
                {(field) => (
                  <Input
                    {...field}
                    value={profile.base_url ?? ""}
                    disabled={busy}
                    onChange={(event) => onChange({ ...profile, base_url: event.target.value })}
                    placeholder="https://api.example.com/v1"
                  />
                )}
              </Field>
              <ModelPicker
                role="asr"
                protocol="open_ai"
                baseUrl={profile.base_url}
                allowInsecureHttp={profile.allow_insecure_http}
                allowUnfilteredFallback
                apiKey={apiKey}
                profileId={profile.id || null}
                hasSavedKey={canUseSavedKeyForModels}
                value={profile.model ?? ""}
                fallbackModels={[]}
                disabled={busy}
                t={t}
                onChange={(model) => onChange({ ...profile, model })}
              />
            </div>
            <InsecureHttpToggle
              checked={profile.allow_insecure_http}
              disabled={busy}
              t={t}
              onChange={(checked) => onChange({ ...profile, allow_insecure_http: checked })}
            />
          </div>
        )}

        {preset && (
          <details className="rounded-md border border-border bg-muted/20 p-3">
            <summary className="flex cursor-pointer items-center justify-between gap-2 text-xs font-medium text-foreground">
              <span>{t("profiles.advanced")}</span>
              <span className="truncate font-normal text-muted-foreground">
                {profileEndpoint(profile, t)}
              </span>
            </summary>
            <div className="mt-3 grid gap-3 sm:grid-cols-2">
              <Field label={t("profiles.label")}>
                {(field) => (
                  <Input
                    {...field}
                    value={profile.label}
                    disabled={busy}
                    onChange={(event) => onChange({ ...profile, label: event.target.value })}
                  />
                )}
              </Field>
              <Field label={t("settings.asr.engine")}>
                {(field) => (
                  <Select
                    value={profile.engine}
                    disabled={busy}
                    onValueChange={(value) => onChange({ ...profile, engine: value as AsrProfile["engine"] })}
                  >
                    <SelectTrigger {...field}><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="openai_compatible">{t("settings.asr.engine.openai")}</SelectItem>
                      <SelectItem value="unisound_u2">{t("settings.asr.engine.u2")}</SelectItem>
                    </SelectContent>
                  </Select>
                )}
              </Field>
              <Field label={t("profiles.baseUrl")} hint={t("profiles.baseUrlHint")}>
                {(field) => (
                  <Input
                    {...field}
                    value={profile.base_url ?? ""}
                    disabled={busy}
                    onChange={(event) => onChange({ ...profile, base_url: event.target.value })}
                    placeholder={preset.baseUrl}
                  />
                )}
              </Field>
            </div>
            <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
              <InsecureHttpToggle
                checked={profile.allow_insecure_http}
                disabled={busy}
                t={t}
                onChange={(checked) => onChange({ ...profile, allow_insecure_http: checked })}
                compact
              />
              {isCustomized && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-xs text-muted-foreground"
                  disabled={busy}
                  onClick={() =>
                    onChange({
                      ...profile,
                      label: preset.label,
                      engine: preset.engine,
                      base_url: preset.baseUrl,
                      model: preset.model,
                      extra_options: preset.extraOptions ? preset.extraOptions.map(([k, v]) => [k, v]) : [],
                      allow_insecure_http: false,
                    })
                  }
                >
                  <RotateCcw className="size-3.5" aria-hidden="true" />
                  {t("profiles.resetPreset")}
                </Button>
              )}
            </div>
          </details>
        )}
      </div>

      <EditorActions
        busy={busy}
        canSave={validationError === null}
        hint={validationError}
        t={t}
        onCancel={onCancel}
      />
    </form>
  );
}

function LlmProfileEditor({
  profile,
  selection,
  original,
  apiKey,
  busy,
  t,
  onChange,
  onApiKeyChange,
  onSave,
  onCancel,
}: {
  profile: LlmProfile;
  selection: string;
  original?: OriginalKeyRef;
  apiKey: string;
  busy: boolean;
  t: TFunc;
  onChange: (profile: LlmProfile, selection?: string) => void;
  onApiKeyChange: (key: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const preset = findLlmPreset(profile.preset);
  const isCustom = selection === "__custom__";
  const hasSelection = selection !== "";
  const keyUrl = resolvePresetKeyUrl(preset, profile.base_url);
  const providerSwitched = Boolean(original && original.preset !== profile.preset);
  const canUseSavedKeyForModels = Boolean(
    profile.credential_id &&
      (!original || endpointHost(original.base_url) === endpointHost(profile.base_url)),
  );
  const needsEndpoint = profile.engine === "custom";
  const isCustomized = Boolean(
    preset &&
      (profile.label !== preset.label ||
        profile.engine !== preset.engine ||
        profile.format !== preset.format ||
        (profile.base_url ?? null) !== preset.baseUrl ||
        (profile.model ?? "") !== preset.model ||
        profile.allow_insecure_http),
  );

  const validationError = (() => {
    if (!hasSelection) return t("profiles.validation.provider");
    if (!profile.label.trim()) return t("profiles.validation.label");
    if (needsEndpoint && !profile.base_url?.trim()) return t("profiles.validation.baseUrl");
    if (profile.base_url?.trim()) {
      const urlError = validateBaseUrl(profile.base_url, profile.allow_insecure_http, t);
      if (urlError) return urlError;
    }
    if (needsEndpoint && !profile.model?.trim()) return t("profiles.validation.model");
    if (!apiKey.trim() && !profile.credential_id) {
      return providerSwitched
        ? t("profiles.validation.newKeyForProvider")
        : t("profiles.validation.apiKey");
    }
    return null;
  })();

  const selectProvider = (value: string) => {
    if (value === "__custom__") {
      const restoreOriginal = original && original.preset === null;
      onChange(
        {
          ...profile,
          preset: null,
          label: isCustom ? profile.label : t("profiles.customLlm"),
          engine: "custom",
          format: "open_ai_chat",
          base_url: null,
          model: null,
          credential_id: restoreOriginal ? original.credential_id : null,
          credential_hint: restoreOriginal ? original.credential_hint : null,
        },
        "__custom__",
      );
      return;
    }
    const next = findLlmPreset(value);
    if (!next) return;
    const restoreOriginal = original && original.preset === next.id;
    onChange(
      {
        ...profile,
        preset: next.id,
        label: next.label,
        engine: next.engine,
        format: next.format,
        base_url: next.baseUrl,
        model: next.model,
        credential_id: restoreOriginal ? original.credential_id : null,
        credential_hint: restoreOriginal ? original.credential_hint : null,
      },
      next.id,
    );
  };

  return (
    <form
      onSubmit={(event: FormEvent) => {
        event.preventDefault();
        if (!validationError) onSave();
      }}
    >
      <div className="space-y-4">
        <Field label={t("profiles.provider")}>
          {(field) => (
            <Select value={selection} disabled={busy} onValueChange={selectProvider}>
              <SelectTrigger {...field}>
                <SelectValue placeholder={t("profiles.selectProvider")} />
              </SelectTrigger>
              <SelectContent>
                {llmPresets.map((item) => (
                  <SelectItem key={item.id} value={item.id}>
                    {item.label}
                  </SelectItem>
                ))}
                <SelectSeparator />
                <SelectItem value="__custom__">{t("profiles.custom")}</SelectItem>
              </SelectContent>
            </Select>
          )}
        </Field>

        {preset?.endpoints && (
          <RegionSelector
            endpoints={preset.endpoints}
            currentUrl={profile.base_url}
            disabled={busy}
            t={t}
            onSelect={(baseUrl) => onChange({ ...profile, base_url: baseUrl })}
          />
        )}

        {hasSelection && (
          <ApiKeyField
            apiKey={apiKey}
            savedHint={profile.credential_hint}
            hasSavedKey={Boolean(profile.credential_id)}
            keyUrl={keyUrl}
            busy={busy}
            t={t}
            onChange={onApiKeyChange}
          />
        )}

        {preset && (
          <ModelPicker
            role="llm"
            protocol={profile.format === "anthropic_messages" ? "anthropic" : "open_ai"}
            baseUrl={profile.base_url}
            allowInsecureHttp={profile.allow_insecure_http}
            apiKey={apiKey}
            profileId={profile.id || null}
            hasSavedKey={canUseSavedKeyForModels}
            value={profile.model ?? ""}
            fallbackModels={preset.models}
            preferredDefault={preset.model}
            disabled={busy}
            t={t}
            onChange={(model) => onChange({ ...profile, model })}
          />
        )}

        {isCustom && (
          <div className="space-y-3 rounded-md border border-border bg-muted/20 p-3.5">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label={t("profiles.label")}>
                {(field) => (
                  <Input
                    {...field}
                    value={profile.label}
                    disabled={busy}
                    onChange={(event) => onChange({ ...profile, label: event.target.value })}
                  />
                )}
              </Field>
              <Field label={t("settings.llm.format")}>
                {(field) => (
                  <Select
                    value={profile.format}
                    disabled={busy}
                    onValueChange={(value) => onChange({ ...profile, format: value as LlmProfile["format"] })}
                  >
                    <SelectTrigger {...field}><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="open_ai_chat">{t("settings.llm.format.chat")}</SelectItem>
                      <SelectItem value="open_ai_responses">{t("settings.llm.format.responses")}</SelectItem>
                      <SelectItem value="anthropic_messages">{t("settings.llm.format.anthropic")}</SelectItem>
                    </SelectContent>
                  </Select>
                )}
              </Field>
              <Field
                label={t("profiles.baseUrl")}
                hint={t(
                  profile.format === "anthropic_messages"
                    ? "settings.llm.baseHelp.anthropic"
                    : "settings.llm.baseHelp.chat",
                )}
              >
                {(field) => (
                  <Input
                    {...field}
                    value={profile.base_url ?? ""}
                    disabled={busy}
                    onChange={(event) => onChange({ ...profile, base_url: event.target.value })}
                    placeholder={
                      profile.format === "anthropic_messages"
                        ? "https://api.anthropic.com"
                        : "https://api.example.com/v1"
                    }
                  />
                )}
              </Field>
              <ModelPicker
                role="llm"
                protocol={profile.format === "anthropic_messages" ? "anthropic" : "open_ai"}
                baseUrl={profile.base_url}
                allowInsecureHttp={profile.allow_insecure_http}
                allowUnfilteredFallback
                apiKey={apiKey}
                profileId={profile.id || null}
                hasSavedKey={canUseSavedKeyForModels}
                value={profile.model ?? ""}
                fallbackModels={[]}
                disabled={busy}
                t={t}
                onChange={(model) => onChange({ ...profile, model })}
              />
            </div>
            <InsecureHttpToggle
              checked={profile.allow_insecure_http}
              disabled={busy}
              t={t}
              onChange={(checked) => onChange({ ...profile, allow_insecure_http: checked })}
            />
          </div>
        )}

        {preset && (
          <details className="rounded-md border border-border bg-muted/20 p-3">
            <summary className="flex cursor-pointer items-center justify-between gap-2 text-xs font-medium text-foreground">
              <span>{t("profiles.advanced")}</span>
              <span className="truncate font-normal text-muted-foreground">
                {profileEndpoint(profile, t)}
              </span>
            </summary>
            <div className="mt-3 grid gap-3 sm:grid-cols-2">
              <Field label={t("profiles.label")}>
                {(field) => (
                  <Input
                    {...field}
                    value={profile.label}
                    disabled={busy}
                    onChange={(event) => onChange({ ...profile, label: event.target.value })}
                  />
                )}
              </Field>
              <Field label={t("settings.llm.format")}>
                {(field) => (
                  <Select
                    value={profile.format}
                    disabled={busy}
                    onValueChange={(value) => onChange({ ...profile, format: value as LlmProfile["format"] })}
                  >
                    <SelectTrigger {...field}><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="open_ai_chat">{t("settings.llm.format.chat")}</SelectItem>
                      <SelectItem value="open_ai_responses">{t("settings.llm.format.responses")}</SelectItem>
                      <SelectItem value="anthropic_messages">{t("settings.llm.format.anthropic")}</SelectItem>
                    </SelectContent>
                  </Select>
                )}
              </Field>
              <Field label={t("profiles.baseUrl")} hint={t("profiles.baseUrlHint")}>
                {(field) => (
                  <Input
                    {...field}
                    value={profile.base_url ?? ""}
                    disabled={busy}
                    onChange={(event) => onChange({ ...profile, base_url: event.target.value })}
                    placeholder={preset.baseUrl}
                  />
                )}
              </Field>
            </div>
            <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
              <InsecureHttpToggle
                checked={profile.allow_insecure_http}
                disabled={busy}
                t={t}
                onChange={(checked) => onChange({ ...profile, allow_insecure_http: checked })}
                compact
              />
              {isCustomized && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-xs text-muted-foreground"
                  disabled={busy}
                  onClick={() =>
                    onChange({
                      ...profile,
                      label: preset.label,
                      engine: preset.engine,
                      format: preset.format,
                      base_url: preset.baseUrl,
                      model: preset.model,
                      allow_insecure_http: false,
                    })
                  }
                >
                  <RotateCcw className="size-3.5" aria-hidden="true" />
                  {t("profiles.resetPreset")}
                </Button>
              )}
            </div>
          </details>
        )}
      </div>

      <EditorActions
        busy={busy}
        canSave={validationError === null}
        hint={validationError}
        t={t}
        onCancel={onCancel}
      />
    </form>
  );
}

function RegionSelector({
  endpoints,
  currentUrl,
  disabled,
  t,
  onSelect,
}: {
  endpoints: EndpointOption[];
  currentUrl: string | null;
  disabled: boolean;
  t: TFunc;
  onSelect: (baseUrl: string) => void;
}) {
  const normalized = (currentUrl ?? "").trim().replace(/\/+$/, "");
  return (
    <div className="space-y-1.5">
      <span className="block text-sm font-medium leading-none">{t("profiles.region")}</span>
      <div className="grid grid-cols-2 gap-2" role="group" aria-label={t("profiles.region")}>
        {endpoints.map((option) => {
          const active = option.baseUrl.replace(/\/+$/, "") === normalized;
          return (
            <button
              key={option.id}
              type="button"
              disabled={disabled}
              aria-pressed={active}
              onClick={() => onSelect(option.baseUrl)}
              className={cn(
                "flex flex-col items-start rounded-md border px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50",
                active
                  ? "border-primary/50 bg-primary/[0.06] text-foreground"
                  : "border-border bg-background text-muted-foreground hover:border-border/80 hover:text-foreground",
              )}
            >
              <span className="text-xs font-medium">{t(option.labelKey)}</span>
              <span className="mt-0.5 max-w-full truncate font-mono text-[11px] opacity-80">
                {endpointHost(option.baseUrl) ?? option.baseUrl}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function ApiKeyField({
  apiKey,
  savedHint,
  hasSavedKey,
  keyUrl,
  busy,
  t,
  onChange,
}: {
  apiKey: string;
  savedHint: string | null;
  hasSavedKey: boolean;
  keyUrl?: string;
  busy: boolean;
  t: TFunc;
  onChange: (key: string) => void;
}) {
  const [visible, setVisible] = useState(false);
  return (
    <Field
      label={t("profiles.apiKey")}
      labelAction={
        keyUrl ? (
          <a
            className="inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline"
            href={keyUrl}
            target="_blank"
            rel="noreferrer"
            onClick={(event) => {
              if (isTauri()) {
                event.preventDefault();
                void api.openExternalUrl(keyUrl);
              }
            }}
          >
            {t("profiles.getKey")}
            <ExternalLink className="size-3" aria-hidden="true" />
          </a>
        ) : undefined
      }
      hint={hasSavedKey ? t("profiles.apiKeyKeep", { hint: savedHint ?? "••••" }) : t("profiles.apiKeyRequired")}
    >
      {(field) => (
        <div className="relative">
          <KeyRound
            className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            {...field}
            className="pl-9 pr-10"
            type={visible ? "text" : "password"}
            value={apiKey}
            disabled={busy}
            autoComplete="new-password"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            onChange={(event) => onChange(event.target.value)}
            placeholder={hasSavedKey ? t("profiles.apiKeyPlaceholderSaved") : "sk-…"}
          />
          <button
            type="button"
            disabled={busy}
            onClick={() => setVisible((v) => !v)}
            aria-label={t(visible ? "profiles.hideKey" : "profiles.showKey")}
            title={t(visible ? "profiles.hideKey" : "profiles.showKey")}
            className="absolute right-1.5 top-1/2 flex size-7 -translate-y-1/2 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
          >
            {visible ? <EyeOff className="size-4" aria-hidden="true" /> : <Eye className="size-4" aria-hidden="true" />}
          </button>
        </div>
      )}
    </Field>
  );
}

function InsecureHttpToggle({
  checked,
  disabled,
  t,
  onChange,
  compact = false,
}: {
  checked: boolean;
  disabled: boolean;
  t: TFunc;
  onChange: (checked: boolean) => void;
  compact?: boolean;
}) {
  const id = useId();
  return (
    <div className={compact ? undefined : "mt-3"}>
      <div className="flex items-center gap-2">
        <Checkbox id={id} checked={checked} disabled={disabled} onCheckedChange={(value) => onChange(value === true)} />
        <Label htmlFor={id} className="text-xs text-muted-foreground">{t("profiles.insecure")}</Label>
      </div>
      {checked && (
        <p className="mt-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs leading-relaxed text-destructive" role="alert">
          {t("profiles.insecureWarning")}
        </p>
      )}
    </div>
  );
}

function EditorActions({
  busy,
  canSave,
  hint,
  t,
  onCancel,
}: {
  busy: boolean;
  canSave: boolean;
  hint: string | null;
  t: TFunc;
  onCancel: () => void;
}) {
  return (
    <div className="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
      <p className="min-w-0 flex-1 text-xs text-muted-foreground" role="status">
        {!canSave && hint ? hint : ""}
      </p>
      <DialogFooter className="mt-0">
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button type="submit" size="sm" disabled={busy || !canSave}>
          {busy && <Loader2 className="animate-spin" aria-hidden="true" />}
          {busy ? t("settings.saving") : t("common.save")}
        </Button>
      </DialogFooter>
    </div>
  );
}

type FieldControlProps = { id: string; "aria-describedby"?: string };

function Field({
  label,
  labelAction,
  hint,
  children,
}: {
  label: string;
  labelAction?: ReactNode;
  hint?: string;
  children: (props: FieldControlProps) => ReactNode;
}) {
  const id = useId();
  const hintId = `${id}-hint`;
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={id}>{label}</Label>
        {labelAction}
      </div>
      {children({ id, "aria-describedby": hint ? hintId : undefined })}
      {hint && <p id={hintId} className="text-xs leading-relaxed text-muted-foreground">{hint}</p>}
    </div>
  );
}

function InlineError({ message }: { message: string }) {
  return (
    <div className="mb-3 flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs leading-relaxed text-destructive" role="alert">
      <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      <span className="min-w-0 break-words">{message}</span>
    </div>
  );
}

function BusyLabel({ t }: { t: TFunc }) {
  return (
    <span className="mt-2 inline-flex items-center gap-1.5 text-xs text-muted-foreground" role="status">
      <Loader2 className="animate-spin" aria-hidden="true" />
      {t("settings.saving")}
    </span>
  );
}

function profileEndpoint(profile: AnyProfile, t: TFunc): string {
  if (profile.base_url?.trim()) return profile.base_url.trim();
  if (profile.engine === "unisound_u2") return "https://maas-api.unisound.com";
  if ("format" in profile && profile.engine === "unisound") {
    return profile.format === "anthropic_messages"
      ? "https://maas-api.unisound.com/anthropic"
      : "https://maas-api.unisound.com/v1";
  }
  return t("profiles.defaultEndpoint");
}

function endpointHost(url: string | null): string | null {
  if (!url?.trim()) return null;
  try {
    return new URL(url.trim()).hostname || null;
  } catch {
    return null;
  }
}

function isLoopbackHostname(hostname: string): boolean {
  const lower = hostname.toLowerCase();
  if (lower === "localhost" || lower === "[::1]" || lower === "::1") return true;
  return /^127\.(?:\d{1,3}\.){2}\d{1,3}$/.test(lower);
}

function validateBaseUrl(raw: string, allowInsecureHttp: boolean, t: TFunc): string | null {
  const trimmed = raw.trim();
  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    return t("profiles.validation.invalidUrl");
  }
  if ((parsed.protocol !== "https:" && parsed.protocol !== "http:") || !parsed.hostname) {
    return t("profiles.validation.invalidUrl");
  }
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    return t("profiles.validation.invalidUrl");
  }
  if (parsed.protocol === "http:" && !allowInsecureHttp && !isLoopbackHostname(parsed.hostname)) {
    return t("profiles.validation.httpsRequired");
  }
  return null;
}

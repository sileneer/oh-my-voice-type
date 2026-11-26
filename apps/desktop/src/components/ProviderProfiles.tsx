import { useId, useState, type FormEvent, type ReactNode } from "react";
import {
  AlertTriangle,
  Copy,
  Download,
  FlaskConical,
  Loader2,
  Mic,
  Pencil,
  Play,
  Plus,
  Sparkles,
  Trash2,
  Upload,
} from "lucide-react";
import {
  api,
  type ApiStatus,
  type AsrProfile,
  type CredentialMetadata,
  type LlmProfile,
  type ProfileRole,
  type ProfileTestResult,
  type ReadinessState,
  type Settings,
} from "../lib/api";
import type { MessageKey, TFunc } from "../i18n";
import { cn } from "../lib/utils";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "./ui/alert-dialog";
import { Badge } from "./ui/badge";
import { Button, buttonVariants } from "./ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "./ui/card";
import { Checkbox } from "./ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./ui/dialog";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { RadioGroup, RadioGroupItem } from "./ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Separator } from "./ui/separator";

type AnyProfile = AsrProfile | LlmProfile;
type EditorMode = "new" | "edit" | "copy";
type EditorState =
  | { role: "asr"; profile: AsrProfile; mode: EditorMode }
  | { role: "llm"; profile: LlmProfile; mode: EditorMode };
type DeleteTarget = { role: ProfileRole; profile: AnyProfile };
type TestTarget = { role: ProfileRole; id: string };

interface Props {
  settings: Settings;
  apiStatus: ApiStatus;
  t: TFunc;
  onChanged: () => Promise<void>;
  onError: (message: string) => void;
}

const readinessLabels: Record<ReadinessState, MessageKey> = {
  ready: "profiles.state.ready",
  missing_profile: "profiles.state.missing_profile",
  missing_key: "profiles.state.missing_key",
  vault_error: "profiles.state.vault_error",
  invalid_config: "profiles.state.invalid_config",
};

const readinessTones: Record<ReadinessState, string> = {
  ready: "border-success/30 bg-success/10 text-success",
  missing_profile: "border-border bg-muted text-muted-foreground",
  missing_key: "border-warning/30 bg-warning/10 text-warning",
  vault_error: "border-destructive/30 bg-destructive/10 text-destructive",
  invalid_config: "border-warning/30 bg-warning/10 text-warning",
};

const UNISOUND_MODELS = ["u2-flash", "u2", "u2-med", "u2-radimed"];
const iconButtonClass = "h-8 w-8 text-muted-foreground";
const compactButtonClass = "h-8 px-2.5";

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

  const newAsr = () => {
    setErrorMessage(null);
    setEditor({
      mode: "new",
      role: "asr",
      profile: {
        id: "",
        label: "",
        engine: "unisound_u2",
        base_url: null,
        model: null,
        extra_options: [],
        credential_id: null,
        allow_insecure_http: false,
      },
    });
  };

  const newLlm = () => {
    setErrorMessage(null);
    setEditor({
      mode: "new",
      role: "llm",
      profile: {
        id: "",
        label: "",
        engine: "unisound",
        format: "open_ai_chat",
        base_url: null,
        model: null,
        credential_id: null,
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
        profile: {
          ...profile,
          id: "",
          label: t("profiles.copyLabel", { label: profile.label }),
          credential_id: null,
        },
      });
    } else {
      setEditor({
        mode: "copy",
        role: "asr",
        profile: {
          ...profile,
          id: "",
          label: t("profiles.copyLabel", { label: profile.label }),
          extra_options: profile.extra_options.map(([key, value]) => [key, value]),
          credential_id: null,
        },
      });
    }
  };

  const persistProfile = async (confirmHostChange: boolean) => {
    if (!editor) return;
    const { role, profile } = editor;
    const normalized = {
      ...profile,
      label: profile.label.trim(),
      base_url: profile.base_url?.trim() || null,
      model: profile.model?.trim() || null,
    };
    setBusyRole(role);
    setErrorMessage(null);
    try {
      if (role === "llm" && "format" in normalized) {
        await api.saveLlmProfile(normalized, confirmHostChange);
      } else if (role === "asr" && "extra_options" in normalized) {
        await api.saveAsrProfile(normalized, confirmHostChange);
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
    const { role, profile } = editor;
    const existing = profile.id
      ? role === "asr"
        ? settings.asr_profiles.find((item) => item.id === profile.id)
        : settings.llm_profiles.find((item) => item.id === profile.id)
      : undefined;
    const hostChanged = Boolean(
      existing && endpointHost(existing.base_url) !== endpointHost(profile.base_url),
    );
    if (existing && (existing.credential_id || profile.credential_id) && hostChanged) {
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
      setEditor((current) => current?.profile.id === profile.id ? null : current);
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
      reportError(error);
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
      ? t("common.edit")
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
    <Card className="border-border/80 bg-card/70 shadow-none">
      <CardHeader className="space-y-1 p-4 pb-0">
        <CardTitle role="heading" aria-level={2} className="text-sm">{t("profiles.title")}</CardTitle>
        <p className="text-xs leading-relaxed text-muted-foreground">{t("profiles.hint")}</p>
      </CardHeader>
      <CardContent className="space-y-4 p-4">
        {errorMessage && !editor && <InlineError message={errorMessage} />}

        <ProfileSection
          role="asr"
          profiles={settings.asr_profiles}
          activeId={settings.active_asr_profile_id}
          readiness={apiStatus.asr}
          credentials={settings.credentials}
          busy={busyRole === "asr"}
          testBusy={testBusy}
          testResults={testResults}
          t={t}
          onNew={newAsr}
          onEdit={(profile) => {
            setErrorMessage(null);
            setEditor({ role: "asr", mode: "edit", profile: { ...profile } });
          }}
          onCopy={copyProfile}
          onDelete={(profile) => setDeleteTarget({ role: "asr", profile })}
          onActivate={(id) => void activateProfile("asr", id)}
          onTest={(id, mode) => mode === "request"
            ? setTestTarget({ role: "asr", id })
            : void testProfile("asr", id, mode)}
        />

        <ProfileSection
          role="llm"
          profiles={settings.llm_profiles}
          activeId={settings.active_llm_profile_id}
          readiness={apiStatus.llm}
          credentials={settings.credentials}
          busy={busyRole === "llm"}
          testBusy={testBusy}
          testResults={testResults}
          t={t}
          onNew={newLlm}
          onEdit={(profile) => {
            setErrorMessage(null);
            setEditor({ role: "llm", mode: "edit", profile: { ...profile } });
          }}
          onCopy={copyProfile}
          onDelete={(profile) => setDeleteTarget({ role: "llm", profile })}
          onActivate={(id) => void activateProfile("llm", id)}
          onTest={(id, mode) => mode === "request"
            ? setTestTarget({ role: "llm", id })
            : void testProfile("llm", id, mode)}
        />

        <Separator />
        <section aria-labelledby={transferTitleId}>
          <h3 id={transferTitleId} className="text-xs font-semibold text-foreground">{t("profiles.transfer.title")}</h3>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{t("profiles.transfer.hint")}</p>
          <div className="mt-3 flex flex-wrap gap-2">
            <Button type="button" variant="outline" size="sm" disabled={transferBusy} onClick={() => setTransferConfirmation("export")}>
              {transferBusy ? <Loader2 className="animate-spin" aria-hidden="true" /> : <Download aria-hidden="true" />}
              {t("profiles.transfer.export")}
            </Button>
            <Button type="button" variant="outline" size="sm" disabled={transferBusy} onClick={() => setTransferConfirmation("import")}>
              {transferBusy ? <Loader2 className="animate-spin" aria-hidden="true" /> : <Upload aria-hidden="true" />}
              {t("profiles.transfer.import")}
            </Button>
          </div>
          {transferBusy && <BusyLabel t={t} />}
          {transferMessage && <p className="mt-2 text-xs text-muted-foreground" role="status">{transferMessage}</p>}
        </section>
      </CardContent>

      <Dialog
        open={Boolean(editor)}
        onOpenChange={(open) => {
          if (!open && !busyRole) setEditor(null);
        }}
      >
        <DialogContent
          className="max-h-[calc(100vh-2rem)] max-w-xl overflow-y-auto p-5"
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
              credentials={settings.credentials}
              busy={busyRole === "asr"}
              t={t}
              onChange={(profile) => setEditor({ role: "asr", mode: editor.mode, profile })}
              onSave={saveProfile}
              onCancel={() => setEditor(null)}
            />
          )}
          {editor?.role === "llm" && (
            <LlmProfileEditor
              profile={editor.profile}
              credentials={settings.credentials}
              busy={busyRole === "llm"}
              t={t}
              onChange={(profile) => setEditor({ role: "llm", mode: editor.mode, profile })}
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
            <AlertDialogDescription>{hostChange ? t("profiles.confirmHost", { host: hostChange.host }) : ""}</AlertDialogDescription>
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

      <AlertDialog open={Boolean(deleteTarget)} onOpenChange={(open) => {
        if (!open && !busyRole) setDeleteTarget(null);
      }}>
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

      <AlertDialog open={Boolean(testTarget)} onOpenChange={(open) => {
        if (!open && !testBusy) setTestTarget(null);
      }}>
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

      <AlertDialog open={Boolean(transferConfirmation)} onOpenChange={(open) => {
        if (!open && !transferBusy) setTransferConfirmation(null);
      }}>
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
    </Card>
  );
}

interface ProfileSectionProps<T extends AnyProfile> {
  role: ProfileRole;
  profiles: T[];
  activeId: string | null;
  readiness: ApiStatus[ProfileRole];
  credentials: CredentialMetadata[];
  busy: boolean;
  testBusy: string | null;
  testResults: Record<string, ProfileTestResult>;
  t: TFunc;
  onNew: () => void;
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
  credentials,
  busy,
  testBusy,
  testResults,
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
  const showReadiness = Boolean(activeId) || hasReadinessError;
  return (
    <section className="rounded-md border border-border bg-background/60 p-3" aria-labelledby={titleId}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 id={titleId} className="text-sm font-semibold text-foreground">{title}</h3>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{t(isAsr ? "settings.asr.caps" : "settings.llm.optional")}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="outline" className={cn("text-xs", readinessTones[readiness.state])}>
            {showReadiness ? t(readinessLabels[readiness.state]) : t("profiles.noneSelected")}
          </Badge>
          <Button type="button" variant="outline" size="sm" onClick={onNew} disabled={busy}>
            {busy ? <Loader2 className="animate-spin" aria-hidden="true" /> : <Plus aria-hidden="true" />}
            {t(isAsr ? "profiles.newAsr" : "profiles.newLlm")}
          </Button>
        </div>
      </div>
      {readiness.message && (
        <p
          className={cn("mt-2 text-xs leading-relaxed", hasReadinessError ? "text-destructive" : "text-muted-foreground")}
          role={hasReadinessError ? "alert" : "status"}
        >
          {readiness.message}
        </p>
      )}

      <div className="mt-3 space-y-2" aria-busy={busy || Boolean(testBusy)}>
        {profiles.length === 0 ? (
          <div className="flex flex-col items-center gap-2 rounded-md border border-dashed border-border px-3 py-5 text-center">
            {isAsr ? <Mic className="text-muted-foreground" aria-hidden="true" /> : <Sparkles className="text-muted-foreground" aria-hidden="true" />}
            <p className="text-xs text-muted-foreground">{t(isAsr ? "profiles.emptyAsr" : "profiles.emptyLlm")}</p>
          </div>
        ) : (
          <RadioGroup
            value={activeId ?? ""}
            disabled={busy}
            onValueChange={(value) => {
              if (value) onActivate(value);
            }}
            aria-label={title}
            className="space-y-2"
          >
            {profiles.map((profile) => {
              const active = profile.id === activeId;
              const credential = credentials.find((item) => item.id === profile.credential_id);
              const credentialLabel = profile.credential_id
                ? credential?.label ?? t("profiles.credential.missing")
                : t("profiles.credential.none");
              const testKey = `${role}:${profile.id}`;
              const result = testResults[testKey];
              return (
                <div
                  key={profile.id}
                  className={cn(
                    "rounded-md border p-3 transition-colors",
                    active ? "border-primary/40 bg-primary/[0.04]" : "border-border bg-card/60",
                  )}
                >
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start">
                    <div className="flex min-w-0 flex-1 items-start gap-2.5">
                      <RadioGroupItem value={profile.id} className="mt-0.5" aria-label={`${t("profiles.active")}: ${profile.label}`} />
                      <div className="min-w-0">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-sm font-semibold text-foreground">{profile.label}</span>
                          {active && <Badge variant="outline" className="border-success/30 bg-success/10 text-xs text-success">{t("profiles.active")}</Badge>}
                        </div>
                        <p className="mt-1 break-all text-xs text-muted-foreground">{profileEndpoint(profile, t)}</p>
                        <p className="mt-1 text-xs text-muted-foreground">
                          {profile.model?.trim() || t("profiles.defaultModel")}
                          <span aria-hidden="true"> · </span>
                          {credentialLabel}
                        </p>
                      </div>
                    </div>
                    <div className="flex shrink-0 gap-0.5">
                      <Button type="button" variant="ghost" size="icon" className={iconButtonClass} disabled={busy} onClick={() => onEdit(profile)} aria-label={`${t("common.edit")}: ${profile.label}`} title={t("common.edit")}>
                        <Pencil aria-hidden="true" />
                      </Button>
                      <Button type="button" variant="ghost" size="icon" className={iconButtonClass} disabled={busy} onClick={() => onCopy(profile)} aria-label={`${t("common.copy")}: ${profile.label}`} title={t("common.copy")}>
                        <Copy aria-hidden="true" />
                      </Button>
                      <Button type="button" variant="ghost" size="icon" className={cn(iconButtonClass, "hover:text-destructive")} disabled={busy} onClick={() => onDelete(profile)} aria-label={`${t("common.delete")}: ${profile.label}`} title={t("common.delete")}>
                        <Trash2 aria-hidden="true" />
                      </Button>
                    </div>
                  </div>
                  <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-border pt-3">
                    <Button type="button" variant="outline" size="sm" className={compactButtonClass} disabled={Boolean(testBusy) || busy} onClick={() => onTest(profile.id, "check")}>
                      {testBusy === testKey ? <Loader2 className="animate-spin" aria-hidden="true" /> : <FlaskConical aria-hidden="true" />}
                      {testBusy === testKey ? t("profiles.test.testing") : t("profiles.test.check")}
                    </Button>
                    <Button type="button" variant="outline" size="sm" className={compactButtonClass} disabled={Boolean(testBusy) || busy} onClick={() => onTest(profile.id, "request")}>
                      <Play aria-hidden="true" />
                      {t("profiles.test.request")}
                    </Button>
                    {result && (
                      <span className="flex flex-wrap items-center gap-2 text-xs" role="status" aria-live="polite">
                        <Badge
                          variant="outline"
                          className={cn(
                            "text-xs",
                            result.ok && result.verified
                              ? "border-success/30 bg-success/10 text-success"
                              : result.ok
                                ? "border-warning/30 bg-warning/10 text-warning"
                                : "border-destructive/30 bg-destructive/10 text-destructive",
                          )}
                        >
                          {t(result.verified ? "profiles.test.verified" : "profiles.test.unverified")}
                        </Badge>
                        <span className="text-muted-foreground">{result.message}</span>
                        <span className="text-muted-foreground">{t("profiles.test.elapsed", { ms: result.elapsed_ms })}</span>
                      </span>
                    )}
                  </div>
                </div>
              );
            })}
          </RadioGroup>
        )}
      </div>
      {busy && <BusyLabel t={t} />}
    </section>
  );
}

function AsrProfileEditor({
  profile,
  credentials,
  busy,
  t,
  onChange,
  onSave,
  onCancel,
}: {
  profile: AsrProfile;
  credentials: CredentialMetadata[];
  busy: boolean;
  t: TFunc;
  onChange: (profile: AsrProfile) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const builtIn = profile.engine === "unisound_u2";
  return (
    <form onSubmit={(event: FormEvent) => {
      event.preventDefault();
      onSave();
    }}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label={t("profiles.label")}>
          {(field) => <Input {...field} value={profile.label} disabled={busy} onChange={(event) => onChange({ ...profile, label: event.target.value })} />}
        </Field>
        <Field label={t("settings.asr.engine")}>
          {(field) => (
            <Select value={profile.engine} disabled={busy} onValueChange={(value) => onChange({ ...profile, engine: value as AsrProfile["engine"] })}>
              <SelectTrigger {...field}><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="unisound_u2">{t("settings.asr.engine.u2")}</SelectItem>
                <SelectItem value="openai_compatible">{t("settings.asr.engine.openai")}</SelectItem>
              </SelectContent>
            </Select>
          )}
        </Field>
        <Field label={t("settings.asr.baseUrl")} hint={builtIn ? t("profiles.baseUrlHint") : t("profiles.customBaseUrlRequired")}>
          {(field) => (
            <Input
              {...field}
              value={profile.base_url ?? ""}
              disabled={busy}
              onChange={(event) => onChange({ ...profile, base_url: event.target.value })}
              placeholder={builtIn ? "https://maas-api.unisound.com" : "https://api.example.com/v1"}
            />
          )}
        </Field>
        <Field label={t("settings.asr.model")}>
          {(field) => (
            <Input
              {...field}
              value={profile.model ?? ""}
              disabled={busy}
              onChange={(event) => onChange({ ...profile, model: event.target.value })}
              placeholder={builtIn ? "u2-asr" : "whisper-large-v3-turbo"}
            />
          )}
        </Field>
      </div>
      {builtIn && <p className="mt-3 rounded-md border border-border bg-muted/50 px-3 py-2 text-xs leading-relaxed text-muted-foreground">{t("settings.asr.u2Info")}</p>}
      <div className="mt-3">
        <CredentialSelect profile={profile} role="asr" credentials={credentials} busy={busy} t={t} onChange={(credentialId) => onChange({ ...profile, credential_id: credentialId })} />
      </div>
      <InsecureHttpToggle checked={profile.allow_insecure_http} disabled={busy} t={t} onChange={(checked) => onChange({ ...profile, allow_insecure_http: checked })} />
      <EditorActions busy={busy} canSave={Boolean(profile.label.trim())} t={t} onCancel={onCancel} />
    </form>
  );
}

function LlmProfileEditor({
  profile,
  credentials,
  busy,
  t,
  onChange,
  onSave,
  onCancel,
}: {
  profile: LlmProfile;
  credentials: CredentialMetadata[];
  busy: boolean;
  t: TFunc;
  onChange: (profile: LlmProfile) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const builtIn = profile.engine === "unisound";
  const anthropic = profile.format === "anthropic_messages";
  const modelListId = useId();
  return (
    <form onSubmit={(event: FormEvent) => {
      event.preventDefault();
      onSave();
    }}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label={t("profiles.label")}>
          {(field) => <Input {...field} value={profile.label} disabled={busy} onChange={(event) => onChange({ ...profile, label: event.target.value })} />}
        </Field>
        <Field label={t("settings.llm.engine")}>
          {(field) => (
            <Select value={profile.engine} disabled={busy} onValueChange={(value) => onChange({ ...profile, engine: value as LlmProfile["engine"] })}>
              <SelectTrigger {...field}><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="unisound">{t("settings.llm.engine.unisound")}</SelectItem>
                <SelectItem value="custom">{t("settings.llm.engine.custom")}</SelectItem>
              </SelectContent>
            </Select>
          )}
        </Field>
        <Field label={t("settings.llm.format")}>
          {(field) => (
            <Select value={profile.format} disabled={busy} onValueChange={(value) => onChange({ ...profile, format: value as LlmProfile["format"] })}>
              <SelectTrigger {...field}><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="open_ai_chat">{t("settings.llm.format.chat")}</SelectItem>
                <SelectItem value="open_ai_responses">{t("settings.llm.format.responses")}</SelectItem>
                <SelectItem value="anthropic_messages">{t("settings.llm.format.anthropic")}</SelectItem>
              </SelectContent>
            </Select>
          )}
        </Field>
        <Field label={t("settings.asr.model")} hint={builtIn ? t("profiles.defaultModel") : undefined}>
          {(field) => (
            <Input
              {...field}
              list={modelListId}
              value={profile.model ?? ""}
              disabled={busy}
              onChange={(event) => onChange({ ...profile, model: event.target.value })}
              placeholder={builtIn ? "u2-flash" : anthropic ? "claude-3-5-haiku-latest" : "gpt-4o-mini"}
            />
          )}
        </Field>
      </div>
      {builtIn && (
        <datalist id={modelListId}>
          {UNISOUND_MODELS.map((model) => <option key={model} value={model} />)}
        </datalist>
      )}
      <div className="mt-3">
        <Field
          label={t("settings.asr.baseUrl")}
          hint={builtIn
            ? t("profiles.baseUrlHint")
            : `${t("profiles.customBaseUrlRequired")} ${t(anthropic ? "settings.llm.baseHelp.anthropic" : "settings.llm.baseHelp.chat")}`}
        >
          {(field) => (
            <Input
              {...field}
              value={profile.base_url ?? ""}
              disabled={busy}
              onChange={(event) => onChange({ ...profile, base_url: event.target.value })}
              placeholder={anthropic ? "https://api.anthropic.com" : "https://api.example.com/v1"}
            />
          )}
        </Field>
      </div>
      {builtIn ? (
        <p className="mt-3 rounded-md border border-border bg-muted/50 px-3 py-2 text-xs leading-relaxed text-muted-foreground">{t("settings.llm.unisoundInfo")}</p>
      ) : (
        <p className="mt-2 text-xs leading-relaxed text-muted-foreground">{t("settings.llm.customNote")}</p>
      )}
      <div className="mt-3">
        <CredentialSelect profile={profile} role="llm" credentials={credentials} busy={busy} t={t} onChange={(credentialId) => onChange({ ...profile, credential_id: credentialId })} />
      </div>
      <InsecureHttpToggle checked={profile.allow_insecure_http} disabled={busy} t={t} onChange={(checked) => onChange({ ...profile, allow_insecure_http: checked })} />
      <EditorActions busy={busy} canSave={Boolean(profile.label.trim())} t={t} onCancel={onCancel} />
    </form>
  );
}

function CredentialSelect({
  profile,
  role,
  credentials,
  busy,
  t,
  onChange,
}: {
  profile: AnyProfile;
  role: ProfileRole;
  credentials: CredentialMetadata[];
  busy: boolean;
  t: TFunc;
  onChange: (id: string | null) => void;
}) {
  const compatible = credentials.filter((credential) =>
    role === "asr" ? credential.role !== "llm" : credential.role !== "asr",
  );
  const bound = credentials.find((credential) => credential.id === profile.credential_id);
  if (bound && !compatible.some((credential) => credential.id === bound.id)) compatible.unshift(bound);
  return (
    <Field label={t("profiles.credential")}>
      {(field) => (
        <Select
          value={profile.credential_id ?? ""}
          disabled={busy}
          onValueChange={(value) => onChange(value === "none" ? null : value)}
        >
          <SelectTrigger {...field}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="none">{t("profiles.credential.none")}</SelectItem>
            {compatible.map((credential) => (
              <SelectItem key={credential.id} value={credential.id}>{credential.label}</SelectItem>
            ))}
            {profile.credential_id && !bound && <SelectItem value={profile.credential_id}>{t("profiles.credential.missing")}</SelectItem>}
          </SelectContent>
        </Select>
      )}
    </Field>
  );
}

function InsecureHttpToggle({
  checked,
  disabled,
  t,
  onChange,
}: {
  checked: boolean;
  disabled: boolean;
  t: TFunc;
  onChange: (checked: boolean) => void;
}) {
  const id = useId();
  return (
    <div className="mt-3">
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
  t,
  onCancel,
}: {
  busy: boolean;
  canSave: boolean;
  t: TFunc;
  onCancel: () => void;
}) {
  return (
    <DialogFooter className="mt-4">
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onCancel}>{t("common.cancel")}</Button>
      <Button type="submit" size="sm" disabled={busy || !canSave}>
        {busy && <Loader2 className="animate-spin" aria-hidden="true" />}
        {busy ? t("settings.saving") : t("common.save")}
      </Button>
    </DialogFooter>
  );
}

type FieldControlProps = { id: string; "aria-describedby"?: string };

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: (props: FieldControlProps) => ReactNode;
}) {
  const id = useId();
  const hintId = `${id}-hint`;
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children({ id, "aria-describedby": hint ? hintId : undefined })}
      {hint && <p id={hintId} className="text-xs leading-relaxed text-muted-foreground">{hint}</p>}
    </div>
  );
}

function InlineError({ message }: { message: string }) {
  return (
    <div className="mb-3 flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs leading-relaxed text-destructive" role="alert">
      <AlertTriangle className="mt-0.5 shrink-0" aria-hidden="true" />
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

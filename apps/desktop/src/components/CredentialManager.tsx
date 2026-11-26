import { useId, useState, type FormEvent, type ReactNode } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  KeyRound,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { api, type CredentialMetadata, type CredentialRole, type ProfileRole, type Settings } from "../lib/api";
import type { MessageKey, TFunc } from "../i18n";
import { cn } from "../lib/utils";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./ui/alert-dialog";
import { Badge } from "./ui/badge";
import { Button, buttonVariants } from "./ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "./ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./ui/dialog";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { RadioGroup, RadioGroupItem } from "./ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Separator } from "./ui/separator";

interface Props {
  settings: Settings;
  t: TFunc;
  onChanged: () => Promise<void>;
  onError: (message: string) => void;
}

interface ProfileReference {
  id: string;
  label: string;
  role: ProfileRole;
}

type DeleteTarget = { credential: CredentialMetadata; references: ProfileReference[] };

const roleLabels: Record<CredentialRole, MessageKey> = {
  asr: "credentials.role.asr",
  llm: "credentials.role.llm",
  both: "credentials.role.both",
};

const iconButtonClass = "h-8 w-8 text-muted-foreground";

export default function CredentialManager({ settings, t, onChanged, onError }: Props) {
  const [label, setLabel] = useState("");
  const [role, setRole] = useState<CredentialRole>("both");
  const [key, setKey] = useState("");
  const [adding, setAdding] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [renameTarget, setRenameTarget] = useState<CredentialMetadata | null>(null);
  const [renameLabel, setRenameLabel] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget | null>(null);
  const [replacingId, setReplacingId] = useState<string | null>(null);
  const [replacementKey, setReplacementKey] = useState("");
  const [replaceScope, setReplaceScope] = useState<"all" | "one">("all");
  const [replaceProfileId, setReplaceProfileId] = useState("");
  const [cleanupBusy, setCleanupBusy] = useState(false);
  const [cleanupOpen, setCleanupOpen] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const replaceGroupId = useId();

  const hasLegacyCredentials = settings.credentials.some((credential) =>
    credential.id === "legacy-asr-credential" || credential.id === "legacy-llm-credential",
  );
  const replacingCredential = settings.credentials.find((credential) => credential.id === replacingId) ?? null;
  const replacingReferences = replacingCredential ? referencesFor(settings, replacingCredential.id) : [];
  const operationBusy = adding || Boolean(busyId) || cleanupBusy;

  const reportError = (error: unknown) => {
    const message = String(error);
    setErrorMessage(message);
    onError(message);
  };

  const closeAdd = () => {
    setAddOpen(false);
    setLabel("");
    setRole("both");
    setKey("");
    setErrorMessage(null);
  };

  const addCredential = async (event: FormEvent) => {
    event.preventDefault();
    if (!label.trim() || !key || adding) return;
    setAdding(true);
    setMessage(null);
    setErrorMessage(null);
    try {
      await api.createCredential(label.trim(), role, key);
      setLabel("");
      setKey("");
      setAddOpen(false);
      await onChanged();
    } catch (error) {
      reportError(error);
    } finally {
      setAdding(false);
    }
  };

  const openRename = (credential: CredentialMetadata) => {
    setRenameTarget(credential);
    setRenameLabel(credential.label);
    setErrorMessage(null);
  };

  const closeRename = () => {
    if (busyId) return;
    setRenameTarget(null);
    setRenameLabel("");
  };

  const renameCredential = async (event: FormEvent) => {
    event.preventDefault();
    if (!renameTarget || !renameLabel.trim() || renameLabel.trim() === renameTarget.label || busyId) return;
    setBusyId(renameTarget.id);
    setErrorMessage(null);
    try {
      await api.renameCredential(renameTarget.id, renameLabel.trim());
      setRenameTarget(null);
      setRenameLabel("");
      await onChanged();
    } catch (error) {
      reportError(error);
    } finally {
      setBusyId(null);
    }
  };

  const deleteCredential = async (credential: CredentialMetadata) => {
    setDeleteTarget(null);
    setBusyId(credential.id);
    setErrorMessage(null);
    try {
      await api.deleteCredential(credential.id);
      if (replacingId === credential.id) closeReplace();
      await onChanged();
    } catch (error) {
      reportError(error);
    } finally {
      setBusyId(null);
    }
  };

  const openReplace = (credential: CredentialMetadata, references: ProfileReference[]) => {
    if (references.length === 0) return;
    setReplacingId(credential.id);
    setReplacementKey("");
    setReplaceScope("all");
    setReplaceProfileId(references[0]?.id ?? "");
    setMessage(null);
    setErrorMessage(null);
  };

  const closeReplace = () => {
    setReplacingId(null);
    setReplacementKey("");
    setReplaceScope("all");
    setReplaceProfileId("");
  };

  const replaceCredential = async (event: FormEvent) => {
    event.preventDefault();
    if (!replacingId || !replacementKey || busyId) return;
    if (replaceScope === "one" && !replaceProfileId) return;
    setBusyId(replacingId);
    setMessage(null);
    setErrorMessage(null);
    try {
      await api.replaceCredential(
        replacingId,
        replacementKey,
        replaceScope === "all" ? null : replaceProfileId,
        replaceScope === "all",
      );
      setReplacingId(null);
      setReplacementKey("");
      setReplaceScope("all");
      setReplaceProfileId("");
      setMessage(t("credentials.replaced"));
      await onChanged();
    } catch (error) {
      reportError(error);
    } finally {
      setBusyId(null);
    }
  };

  const cleanupLegacyKeys = async () => {
    setCleanupOpen(false);
    setCleanupBusy(true);
    setMessage(null);
    setErrorMessage(null);
    try {
      const cleaned = await api.cleanupLegacyKeys();
      setMessage(cleaned.length > 0
        ? t("credentials.cleanupResult", { ids: cleaned.join(", ") })
        : t("credentials.cleanupNone"));
    } catch (error) {
      reportError(error);
    } finally {
      setCleanupBusy(false);
    }
  };

  const deleteQuestion = deleteTarget
    ? deleteTarget.references.length > 0
      ? t("credentials.confirmReferencedDelete", { label: deleteTarget.credential.label, n: deleteTarget.references.length })
      : t("credentials.confirmDelete", { label: deleteTarget.credential.label })
    : "";

  return (
    <Card className="border-border/80 bg-card/70 shadow-none">
      <CardHeader className="flex-row items-start justify-between space-y-0 p-4 pb-0">
        <div className="min-w-0 space-y-1">
          <CardTitle role="heading" aria-level={2} className="text-sm">{t("credentials.title")}</CardTitle>
          <p className="max-w-xl text-xs leading-relaxed text-muted-foreground">{t("credentials.hint")}</p>
        </div>
        <Button type="button" size="sm" disabled={operationBusy} onClick={() => {
          setLabel("");
          setRole("both");
          setKey("");
          setErrorMessage(null);
          setAddOpen(true);
        }}>
          <Plus aria-hidden="true" />
          {t("credentials.add")}
        </Button>
      </CardHeader>
      <CardContent className="p-4">
        {errorMessage && !addOpen && !renameTarget && !replacingCredential && <InlineError message={errorMessage} />}

        <div className="space-y-2" aria-busy={operationBusy}>
          {settings.credentials.length === 0 ? (
            <div className="flex flex-col items-center gap-2 rounded-md border border-dashed border-border px-3 py-6 text-center">
              <KeyRound className="text-muted-foreground" aria-hidden="true" />
              <p className="text-xs text-muted-foreground">{t("credentials.empty")}</p>
            </div>
          ) : (
            settings.credentials.map((credential) => {
              const references = referencesFor(settings, credential.id);
              const rowBusy = busyId === credential.id;
              return (
                <div key={credential.id} className="rounded-md border border-border bg-background/60 p-3">
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="text-sm font-semibold text-foreground">{credential.label}</span>
                        <Badge variant="secondary" className="text-xs">{t(roleLabels[credential.role])}</Badge>
                        {rowBusy && (
                          <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground" role="status">
                            <Loader2 className="animate-spin" aria-hidden="true" />
                            {t("settings.saving")}
                          </span>
                        )}
                      </div>
                      <p className="mt-1 text-xs text-muted-foreground">
                        {t("credentials.hintLabel")}: {credential.hint ? `…${credential.hint}` : t("credentials.hintNone")}
                        <span aria-hidden="true"> · </span>
                        {t("credentials.references", { n: references.length })}
                      </p>
                    </div>
                    <div className="flex shrink-0 gap-0.5">
                      <Button type="button" variant="ghost" size="icon" className={iconButtonClass} disabled={operationBusy} onClick={() => openRename(credential)} aria-label={`${t("common.rename")}: ${credential.label}`} title={t("credentials.rename")}>
                        <Pencil aria-hidden="true" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className={iconButtonClass}
                        disabled={operationBusy || references.length === 0}
                        title={references.length === 0 ? t("credentials.noReferences") : t("credentials.replace")}
                        aria-label={`${t("credentials.replace")}: ${credential.label}`}
                        onClick={() => openReplace(credential, references)}
                      >
                        <RefreshCw aria-hidden="true" />
                      </Button>
                      <Button type="button" variant="ghost" size="icon" className={cn(iconButtonClass, "hover:text-destructive")} disabled={operationBusy} onClick={() => {
                        setErrorMessage(null);
                        setDeleteTarget({ credential, references });
                      }} aria-label={`${t("common.delete")}: ${credential.label}`} title={t("credentials.delete")}>
                        <Trash2 aria-hidden="true" />
                      </Button>
                    </div>
                  </div>
                  {references.length === 0 && (
                    <p className="mt-2 text-xs text-muted-foreground">{t("credentials.noReferences")}</p>
                  )}
                </div>
              );
            })
          )}
        </div>

        {(hasLegacyCredentials || message) && (
          <div className="mt-4">
            <Separator />
            <div className="pt-3">
              {hasLegacyCredentials && (
                <Button type="button" variant="outline" size="sm" disabled={operationBusy} onClick={() => setCleanupOpen(true)}>
                  {cleanupBusy ? <Loader2 className="animate-spin" aria-hidden="true" /> : <ShieldCheck aria-hidden="true" />}
                  {t("credentials.cleanup")}
                </Button>
              )}
              {message && (
                <p className="mt-2 flex items-start gap-2 text-xs leading-relaxed text-muted-foreground" role="status">
                  <CheckCircle2 className="shrink-0 text-success" aria-hidden="true" />
                  {message}
                </p>
              )}
            </div>
          </div>
        )}
      </CardContent>

      <Dialog
        open={addOpen}
        onOpenChange={(open) => {
          if (!open && !adding) closeAdd();
        }}
      >
        <DialogContent
          className="max-h-[calc(100vh-2rem)] max-w-md overflow-y-auto p-5"
          onEscapeKeyDown={(event) => {
            if (adding) event.preventDefault();
          }}
          onInteractOutside={(event) => {
            if (adding) event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle className="text-base">{t("credentials.add")}</DialogTitle>
            <DialogDescription>{t("credentials.hint")}</DialogDescription>
          </DialogHeader>
          {errorMessage && <InlineError message={errorMessage} />}
          <form onSubmit={(event) => void addCredential(event)}>
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label={t("credentials.label")}>
                {(field) => <Input {...field} value={label} disabled={adding} autoComplete="off" onChange={(event) => setLabel(event.target.value)} />}
              </Field>
              <Field label={t("credentials.role")}>
                {(field) => (
                  <Select value={role} disabled={adding} onValueChange={(value) => setRole(value as CredentialRole)}>
                    <SelectTrigger {...field}><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="both">{t("credentials.role.both")}</SelectItem>
                      <SelectItem value="asr">{t("credentials.role.asr")}</SelectItem>
                      <SelectItem value="llm">{t("credentials.role.llm")}</SelectItem>
                    </SelectContent>
                  </Select>
                )}
              </Field>
              <div className="sm:col-span-2">
                <Field label={t("credentials.key")}>
                  {(field) => (
                    <Input
                      {...field}
                      type="password"
                      value={key}
                      disabled={adding}
                      autoComplete="new-password"
                      spellCheck={false}
                      onChange={(event) => setKey(event.target.value)}
                    />
                  )}
                </Field>
              </div>
            </div>
            <DialogFooter className="mt-4">
              <Button type="button" variant="outline" size="sm" disabled={adding} onClick={closeAdd}>{t("common.cancel")}</Button>
              <Button type="submit" size="sm" disabled={adding || !label.trim() || !key}>
                {adding && <Loader2 className="animate-spin" aria-hidden="true" />}
                {adding ? t("settings.saving") : t("credentials.add")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(renameTarget)}
        onOpenChange={(open) => {
          if (!open) closeRename();
        }}
      >
        <DialogContent
          className="max-w-md p-5"
          onEscapeKeyDown={(event) => {
            if (busyId) event.preventDefault();
          }}
          onInteractOutside={(event) => {
            if (busyId) event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle className="text-base">{t("credentials.rename")}</DialogTitle>
            <DialogDescription>{renameTarget ? t("credentials.renamePrompt", { label: renameTarget.label }) : ""}</DialogDescription>
          </DialogHeader>
          {errorMessage && <InlineError message={errorMessage} />}
          <form onSubmit={(event) => void renameCredential(event)}>
            <Field label={t("credentials.label")}>
              {(field) => <Input {...field} value={renameLabel} disabled={Boolean(busyId)} autoFocus autoComplete="off" onChange={(event) => setRenameLabel(event.target.value)} />}
            </Field>
            <DialogFooter className="mt-4">
              <Button type="button" variant="outline" size="sm" disabled={Boolean(busyId)} onClick={closeRename}>{t("common.cancel")}</Button>
              <Button type="submit" size="sm" disabled={Boolean(busyId) || !renameLabel.trim() || renameLabel.trim() === renameTarget?.label}>
                {busyId && <Loader2 className="animate-spin" aria-hidden="true" />}
                {busyId ? t("settings.saving") : t("common.rename")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(replacingCredential)}
        onOpenChange={(open) => {
          if (!open && !busyId) closeReplace();
        }}
      >
        <DialogContent
          className="max-h-[calc(100vh-2rem)] max-w-lg overflow-y-auto p-5"
          onEscapeKeyDown={(event) => {
            if (busyId) event.preventDefault();
          }}
          onInteractOutside={(event) => {
            if (busyId) event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle className="text-base">{t("credentials.replace")}</DialogTitle>
            <DialogDescription>{t("credentials.replaceHint")}</DialogDescription>
          </DialogHeader>
          {errorMessage && <InlineError message={errorMessage} />}
          {replacingCredential && (
            <form onSubmit={(event) => void replaceCredential(event)}>
              <div className="grid gap-3 sm:grid-cols-2">
                <Field label={t("credentials.replacementKey")}>
                  {(field) => (
                    <Input
                      {...field}
                      type="password"
                      value={replacementKey}
                      disabled={Boolean(busyId)}
                      autoComplete="new-password"
                      spellCheck={false}
                      onChange={(event) => setReplacementKey(event.target.value)}
                    />
                  )}
                </Field>
                <Field label={t("credentials.replaceProfile")}>
                  {(field) => (
                    <Select
                      value={replaceProfileId}
                      disabled={Boolean(busyId) || replaceScope === "all"}
                      onValueChange={setReplaceProfileId}
                    >
                      <SelectTrigger {...field}><SelectValue /></SelectTrigger>
                      <SelectContent>
                        {replacingReferences.map((reference) => (
                          <SelectItem key={reference.id} value={reference.id}>
                            {reference.label} ({t(roleLabels[reference.role])})
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  )}
                </Field>
              </div>
              <fieldset className="mt-3">
                <legend className="text-xs font-medium text-foreground">{t("credentials.replace")}</legend>
                <RadioGroup
                  value={replaceScope}
                  disabled={Boolean(busyId)}
                  onValueChange={(value) => setReplaceScope(value as "all" | "one")}
                  className="mt-2 space-y-2"
                >
                  <div className="flex items-center gap-2">
                    <RadioGroupItem value="all" id={`${replaceGroupId}-all`} />
                    <Label htmlFor={`${replaceGroupId}-all`} className="text-xs font-normal text-muted-foreground">{t("credentials.replaceAll")}</Label>
                  </div>
                  <div className="flex items-center gap-2">
                    <RadioGroupItem value="one" id={`${replaceGroupId}-one`} />
                    <Label htmlFor={`${replaceGroupId}-one`} className="text-xs font-normal text-muted-foreground">{t("credentials.replaceOne")}</Label>
                  </div>
                </RadioGroup>
              </fieldset>
              <DialogFooter className="mt-4">
                <Button type="button" variant="outline" size="sm" disabled={Boolean(busyId)} onClick={closeReplace}>{t("common.cancel")}</Button>
                <Button type="submit" size="sm" disabled={Boolean(busyId) || !replacementKey || (replaceScope === "one" && !replaceProfileId)}>
                  {busyId && <Loader2 className="animate-spin" aria-hidden="true" />}
                  {busyId ? t("settings.saving") : t("common.replace")}
                </Button>
              </DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>

      <AlertDialog open={Boolean(deleteTarget)} onOpenChange={(open) => {
        if (!open && !busyId) setDeleteTarget(null);
      }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("credentials.delete")}</AlertDialogTitle>
            <AlertDialogDescription>{deleteQuestion}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={Boolean(busyId)}>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={Boolean(busyId)}
              className={buttonVariants({ variant: "destructive" })}
              onClick={() => {
                if (deleteTarget) void deleteCredential(deleteTarget.credential);
              }}
            >
              {busyId && <Loader2 className="animate-spin" aria-hidden="true" />}
              {t("common.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={cleanupOpen} onOpenChange={(open) => {
        if (!open && !cleanupBusy) setCleanupOpen(false);
      }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("credentials.cleanup")}</AlertDialogTitle>
            <AlertDialogDescription>{t("credentials.cleanupConfirm")}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={cleanupBusy}>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={cleanupBusy}
              className={buttonVariants({ variant: "destructive" })}
              onClick={() => void cleanupLegacyKeys()}
            >
              {cleanupBusy && <Loader2 className="animate-spin" aria-hidden="true" />}
              {t("credentials.cleanup")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}

function referencesFor(settings: Settings, credentialId: string): ProfileReference[] {
  return [
    ...settings.asr_profiles
      .filter((profile) => profile.credential_id === credentialId)
      .map((profile) => ({ id: profile.id, label: profile.label, role: "asr" as const })),
    ...settings.llm_profiles
      .filter((profile) => profile.credential_id === credentialId)
      .map((profile) => ({ id: profile.id, label: profile.label, role: "llm" as const })),
  ];
}

type FieldControlProps = { id: string; "aria-describedby"?: string };

function Field({ label, children }: { label: string; children: (props: FieldControlProps) => ReactNode }) {
  const id = useId();
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children({ id })}
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

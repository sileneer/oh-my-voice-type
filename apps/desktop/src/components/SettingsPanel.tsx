import { useCallback, useEffect, useState } from "react";
import {
  AudioLines, BookOpen, Check, ExternalLink, Info, Languages, LoaderCircle,
  Palette, Plus, ShieldCheck, Trash2, X,
} from "lucide-react";
import packageJson from "../../package.json";
import {
  api, type ApiStatus, type DictionaryEntry, type Mode, type Settings,
} from "../lib/api";
import { toast } from "sonner";
import { localeOptions, localizeHotkey, translationTargets, type Locale, type TFunc } from "../i18n";
import { normalizeThemePreference, themeOptions, useTheme } from "../lib/theme";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Switch } from "./ui/switch";
import { Badge } from "./ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./ui/tabs";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./ui/dialog";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from "./ui/alert-dialog";
import { PageHeader } from "./app/PageHeader";
import ProviderProfiles from "./ProviderProfiles";
import { UpdatePanel } from "./UpdatePanel";

interface Props {
  settings: Settings;
  apiStatus: ApiStatus;
  t: TFunc;
  locale: Locale;
  onSave: (settings: Settings) => Promise<void>;
  onError: (message: string) => void;
  onChanged: () => Promise<void>;
  onApiStatusChanged: () => Promise<void>;
  /** Section to open, e.g. "about" when the tray asks to show an update. */
  section?: { id: string; nonce: number } | null;
}

const HOTKEYS = ["fn", "right-opt", "right-cmd", "right-alt", "right-ctrl", "left-alt", "left-ctrl", "f6", "f7", "f8", "f9", "f10"];
const NONE = "__none__";
const KEEP_ALIVE = "mt-0 data-[state=inactive]:hidden";

export default function SettingsPanel({ settings, apiStatus, t, locale, onSave, onError, onChanged, onApiStatusChanged, section }: Props) {
  const [tab, setTab] = useState(section?.id ?? "general");
  useEffect(() => { if (section) setTab(section.id); }, [section]);
  const configurationChanged = async () => { await onChanged(); await onApiStatusChanged(); };
  const save = async (next: Settings) => {
    await onSave(next);
    toast.success(t("settings.savedToast"));
  };
  const sections = [
    { id: "general", label: t("settings.general"), icon: Palette },
    { id: "modes", label: t("settings.modes"), icon: Languages },
    { id: "providers", label: t("settings.providers"), icon: AudioLines },
    { id: "personalization", label: t("settings.personalization"), icon: BookOpen },
    { id: "privacy", label: t("settings.privacy"), icon: ShieldCheck },
    { id: "about", label: t("settings.about"), icon: Info },
  ];

  return (
    <>
      <PageHeader title={t("tab.settings")} />
      <Tabs value={tab} onValueChange={setTab} orientation="vertical" className="flex flex-col gap-6 md:flex-row">
        <TabsList className="flex h-auto w-full flex-row flex-wrap justify-start gap-1 bg-transparent p-0 md:w-44 md:flex-col md:items-stretch" aria-label={t("tab.settings")}>
          {sections.map(({ id, label, icon: Icon }) => (
            <TabsTrigger key={id} value={id} className="justify-start gap-2 text-muted-foreground hover:text-foreground data-[state=active]:bg-card data-[state=active]:text-foreground data-[state=active]:shadow-sm data-[state=active]:ring-1 data-[state=active]:ring-border">
              <Icon className="size-4" aria-hidden="true" />{label}
            </TabsTrigger>
          ))}
        </TabsList>
        <div className="min-w-0 flex-1">
          {/* Panels stay mounted so unsaved edits survive switching tabs. */}
          <TabsContent forceMount className={KEEP_ALIVE} value="general"><GeneralSettings settings={settings} t={t} onSave={save} onError={onError} /></TabsContent>
          <TabsContent forceMount className={KEEP_ALIVE} value="modes"><ModeSettings settings={settings} locale={locale} t={t} onSave={save} onError={onError} /></TabsContent>
          <TabsContent forceMount className={`${KEEP_ALIVE} space-y-5`} value="providers">
            <ProviderProfiles settings={settings} apiStatus={apiStatus} t={t} onChanged={configurationChanged} onError={onError} />
          </TabsContent>
          <TabsContent forceMount className={KEEP_ALIVE} value="personalization"><PersonalizationSettings settings={settings} t={t} onSave={save} onError={onError} /></TabsContent>
          <TabsContent forceMount className={KEEP_ALIVE} value="privacy"><PrivacySettings settings={settings} t={t} onSave={save} onError={onError} /></TabsContent>
          <TabsContent forceMount className={KEEP_ALIVE} value="about"><AboutSettings settings={settings} locale={locale} t={t} onSave={onSave} onError={onError} /></TabsContent>
        </div>
      </Tabs>
    </>
  );
}

function Section({ title, description, children, footer }: { title: string; description?: string; children: React.ReactNode; footer?: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-border bg-card">
      <div className="border-b border-border p-5"><h2 className="text-base font-semibold">{title}</h2>{description && <p className="mt-1 text-sm leading-relaxed text-muted-foreground">{description}</p>}</div>
      <div className="space-y-5 p-5">{children}</div>
      {footer && <div className="flex justify-end border-t border-border p-4">{footer}</div>}
    </section>
  );
}

function GeneralSettings({ settings, t, onSave, onError }: Pick<Props, "settings" | "t" | "onSave" | "onError">) {
  const [ui, setUi] = useState(settings.ui);
  const [audio, setAudio] = useState(settings.audio);
  const [mics, setMics] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  useTheme(ui.theme);
  useEffect(() => { setUi(settings.ui); setAudio(settings.audio); }, [settings.ui, settings.audio]);
  const dirty = changed({ ui, audio }, { ui: settings.ui, audio: settings.audio });
  useEffect(() => { void api.listMics().then(setMics).catch(() => setMics([])); }, []);
  const save = async () => {
    setSaving(true);
    try { await onSave({ ...settings, ui, audio }); } catch (cause) { onError(String(cause)); } finally { setSaving(false); }
  };
  return (
    <Section title={t("settings.general")} footer={<SaveBar saving={saving} dirty={dirty} t={t} onClick={() => void save()} />}>
      <Group title={t("settings.group.appearance")}>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t("settings.language.interface")}>
            <Select value={ui.locale} onValueChange={(locale) => setUi({ ...ui, locale })}>
              <SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{localeOptions.map((option) => <SelectItem key={option.value} value={option.value}>{t(option.label)}</SelectItem>)}</SelectContent>
            </Select>
          </Field>
          <Field label={t("settings.appearance.theme")} hint={normalizeThemePreference(ui.theme) === "system" ? t("settings.appearance.themeHint") : undefined}>
            <Select value={normalizeThemePreference(ui.theme)} onValueChange={(theme) => setUi({ ...ui, theme })}>
              <SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{themeOptions.map((option) => <SelectItem key={option.value} value={option.value}>{t(option.label)}</SelectItem>)}</SelectContent>
            </Select>
          </Field>
        </div>
      </Group>
      <Group title={t("settings.group.audio")}>
        <Field label={t("settings.audio.mic")}>
          <Select value={audio.mic_device ?? "__default__"} onValueChange={(value) => setAudio({ ...audio, mic_device: value === "__default__" ? null : value })}>
            <SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="__default__">{t("settings.audio.systemDefault")}</SelectItem>{mics.map((mic) => <SelectItem key={mic} value={mic}>{mic}</SelectItem>)}</SelectContent>
          </Select>
        </Field>
        <ToggleRow label={t("settings.prewarm")} description={t("settings.prewarmHint")} checked={audio.prewarm ?? false} onChange={(checked) => setAudio({ ...audio, prewarm: checked })} />
        <ToggleRow label={t("settings.muteOutput")} description={t("settings.muteOutputHint")} checked={audio.mute_output_while_dictating} onChange={(checked) => setAudio({ ...audio, mute_output_while_dictating: checked })} />
      </Group>
      <Group title={t("settings.group.window")}>
        <ToggleRow label={t("settings.closeToTray")} description={t("settings.closeToTrayHint")} checked={ui.close_to_tray ?? true} onChange={(checked) => setUi({ ...ui, close_to_tray: checked })} />
      </Group>
    </Section>
  );
}

function ModeSettings({ settings, locale, t, onSave, onError }: Pick<Props, "settings" | "locale" | "t" | "onSave" | "onError">) {
  const [hotkey, setHotkey] = useState(settings.hotkey);
  const [dictation, setDictation] = useState(settings.dictation);
  const [audio, setAudio] = useState(settings.audio);
  const [saving, setSaving] = useState(false);
  useEffect(() => { setHotkey(settings.hotkey); setDictation(settings.dictation); setAudio(settings.audio); }, [settings.hotkey, settings.dictation, settings.audio]);
  const dirty = changed({ hotkey, dictation, audio }, { hotkey: settings.hotkey, dictation: settings.dictation, audio: settings.audio });
  const save = async () => {
    setSaving(true);
    try { await onSave({ ...settings, hotkey, dictation, audio: { ...audio, max_recording_seconds: clampNumber(audio.max_recording_seconds ?? 540, 60, 900) } }); } catch (cause) { onError(String(cause)); } finally { setSaving(false); }
  };
  return (
    <Section title={t("settings.modes")} footer={<SaveBar saving={saving} dirty={dirty} t={t} onClick={() => void save()} />}>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={t("settings.defaultMode")}>
          <Select value={dictation.default_mode ?? "dictate"} onValueChange={(value) => setDictation({ ...dictation, default_mode: value as Mode })}>
            <SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="dictate">{t("home.mode.dictate")}</SelectItem><SelectItem value="translate">{t("home.mode.translate")}</SelectItem><SelectItem value="edit_selection">{t("home.mode.edit_selection")}</SelectItem></SelectContent>
          </Select>
        </Field>
        <Field label={t("settings.translationTarget")}>
          <Select value={dictation.translate_target} onValueChange={(value) => setDictation({ ...dictation, translate_target: value })}>
            <SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{translationTargets(dictation.translate_target).map((language) => <SelectItem key={language} value={language}>{language}</SelectItem>)}</SelectContent>
          </Select>
        </Field>
      </div>
      <div className="grid gap-4 sm:grid-cols-3">
        <HotkeyField label={t("hotkey.title")} value={hotkey.dictate} locale={locale} t={t} onChange={(value) => setHotkey({ ...hotkey, dictate: value })} />
        <HotkeyField label={t("settings.translateHotkey")} value={hotkey.translate || NONE} locale={locale} t={t} onChange={(value) => setHotkey({ ...hotkey, translate: value === NONE ? "" : value })} optional />
        <HotkeyField label={t("settings.editHotkey")} value={hotkey.edit_selection || NONE} locale={locale} t={t} onChange={(value) => setHotkey({ ...hotkey, edit_selection: value === NONE ? "" : value })} optional />
      </div>
      <ToggleRow label={t("hotkey.smart")} description={t("hotkey.hint.smart")} checked={hotkey.smart_gestures} onChange={(checked) => setHotkey({ ...hotkey, smart_gestures: checked })} />
      <Field label={t("settings.maxRecording")} hint={t("settings.maxRecordingHint")}>
        <div className="relative max-w-48">
          <Input type="number" min={60} max={900} step={30} className="pr-12" value={audio.max_recording_seconds ?? 540} onChange={(event) => setAudio({ ...audio, max_recording_seconds: Number(event.target.value) })} />
          <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-xs text-muted-foreground">{t("settings.secondsSuffix")}</span>
        </div>
      </Field>
      <ToggleRow label={t("settings.behavior.cleanup")} description={t("settings.behavior.cleanupOn")} checked={dictation.cleanup_enabled} onChange={(checked) => setDictation({ ...dictation, cleanup_enabled: checked })} />
      <ToggleRow label={t("settings.behavior.preserve")} checked={dictation.preserve_wording} disabled={!dictation.cleanup_enabled} onChange={(checked) => setDictation({ ...dictation, preserve_wording: checked })} />
      <ToggleRow label={t("settings.behavior.noLists")} checked={dictation.no_list_formatting} disabled={!dictation.cleanup_enabled} onChange={(checked) => setDictation({ ...dictation, no_list_formatting: checked })} />
    </Section>
  );
}

function PersonalizationSettings({ settings, t, onSave, onError }: Pick<Props, "settings" | "t" | "onSave" | "onError">) {
  const [dictation, setDictation] = useState(settings.dictation);
  const [entries, setEntries] = useState<DictionaryEntry[]>([]);
  const [term, setTerm] = useState("");
  const [renameEntry, setRenameEntry] = useState<DictionaryEntry | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [saving, setSaving] = useState(false);
  useEffect(() => setDictation(settings.dictation), [settings.dictation]);
  const dirty = changed(dictation, settings.dictation);
  const loadDictionary = useCallback(async () => { try { setEntries(await api.dictionaryList()); } catch (cause) { onError(String(cause)); } }, [onError]);
  useEffect(() => { void loadDictionary(); }, [loadDictionary]);
  const save = async () => { setSaving(true); try { await onSave({ ...settings, dictation }); } catch (cause) { onError(String(cause)); } finally { setSaving(false); } };
  const add = async () => { if (!term.trim()) return; try { setEntries(await api.dictionaryAdd(term.trim())); setTerm(""); } catch (cause) { onError(String(cause)); } };
  const rename = async () => { if (!renameEntry || !renameValue.trim()) return; try { setEntries(await api.dictionaryRename(renameEntry.term, renameValue.trim())); setRenameEntry(null); } catch (cause) { onError(String(cause)); } };
  const remove = async (entry: DictionaryEntry) => { try { setEntries(await api.dictionaryRemove(entry.term)); } catch (cause) { onError(String(cause)); } };
  const addTone = () => setDictation({ ...dictation, app_tones: [...dictation.app_tones, { app: "", style: "" }] });
  const updateTone = (index: number, patch: Partial<(typeof dictation.app_tones)[number]>) => setDictation({ ...dictation, app_tones: dictation.app_tones.map((tone, i) => i === index ? { ...tone, ...patch } : tone) });
  return (
    <div className="space-y-5">
      <Section title={t("settings.tone")} footer={<SaveBar saving={saving} dirty={dirty} t={t} onClick={() => void save()} />}>
        <Field label={t("settings.defaultTone")}><Input value={dictation.default_tone ?? ""} onChange={(event) => setDictation({ ...dictation, default_tone: event.target.value })} placeholder={t("settings.stylePlaceholder")} /></Field>
        <div className="space-y-3">
          <div className="flex items-center justify-between gap-3"><h3 className="text-sm font-medium">{t("settings.appTone")}</h3><Button type="button" variant="outline" size="sm" onClick={addTone}><Plus className="size-4" aria-hidden="true" />{t("common.add")}</Button></div>
          {dictation.app_tones.map((tone, index) => <div key={index} className="flex flex-wrap items-end gap-2"><Field label={t("settings.appTonePlaceholder")}><Input value={tone.app} onChange={(event) => updateTone(index, { app: event.target.value })} /></Field><Field label={t("settings.stylePlaceholder")}><Input value={tone.style} onChange={(event) => updateTone(index, { style: event.target.value })} /></Field><Button type="button" variant="ghost" size="icon" aria-label={t("common.delete")} onClick={() => setDictation({ ...dictation, app_tones: dictation.app_tones.filter((_, i) => i !== index) })}><X className="size-4" /></Button></div>)}
        </div>
      </Section>
      <Section title={t("settings.dictionary")} description={t("settings.dictionaryHint")}>
        <form className="flex gap-2" onSubmit={(event) => { event.preventDefault(); void add(); }}><Input value={term} onChange={(event) => setTerm(event.target.value)} placeholder={t("settings.dictionaryPlaceholder")} /><Button type="submit" disabled={!term.trim()}><Plus className="size-4" aria-hidden="true" />{t("common.add")}</Button></form>
        {entries.length === 0 ? <p className="text-sm text-muted-foreground">{t("profiles.empty")}</p> : <ul className="divide-y divide-border rounded-md border border-border">{entries.map((entry) => <li key={entry.term} className="flex items-center justify-between gap-3 px-3 py-2.5 text-sm"><span className="truncate">{entry.term}</span><div className="flex gap-1"><Button type="button" variant="ghost" size="sm" onClick={() => { setRenameEntry(entry); setRenameValue(entry.term); }}>{t("common.rename")}</Button><Button type="button" variant="ghost" size="icon" aria-label={`${t("common.delete")} ${entry.term}`} onClick={() => void remove(entry)}><Trash2 className="size-4" /></Button></div></li>)}</ul>}
      </Section>
      <Dialog open={renameEntry !== null} onOpenChange={(open) => !open && setRenameEntry(null)}><DialogContent><DialogHeader><DialogTitle>{t("common.rename")}</DialogTitle><DialogDescription>{renameEntry?.term}</DialogDescription></DialogHeader><Input value={renameValue} onChange={(event) => setRenameValue(event.target.value)} /><DialogFooter><Button type="button" variant="outline" onClick={() => setRenameEntry(null)}>{t("common.cancel")}</Button><Button type="button" onClick={() => void rename()} disabled={!renameValue.trim()}>{t("common.save")}</Button></DialogFooter></DialogContent></Dialog>
    </div>
  );
}

function PrivacySettings({ settings, t, onSave, onError }: Pick<Props, "settings" | "t" | "onSave" | "onError">) {
  const [history, setHistory] = useState(settings.history);
  const [context, setContext] = useState(settings.context);
  const [pendingCount, setPendingCount] = useState(0);
  const [confirmCleanup, setConfirmCleanup] = useState(false);
  const [saving, setSaving] = useState(false);
  useEffect(() => { setHistory(settings.history); setContext(settings.context); }, [settings.history, settings.context]);
  const dirty = changed({ history, context }, { history: settings.history, context: settings.context });
  const loadPending = useCallback(async () => { try { setPendingCount((await api.pendingList()).length); } catch { setPendingCount(0); } }, []);
  useEffect(() => { void loadPending(); }, [loadPending]);
  const save = async () => { setSaving(true); try { await onSave({ ...settings, history, context }); } catch (cause) { onError(String(cause)); } finally { setSaving(false); } };
  const cleanup = async () => { try { await api.pendingCleanup(); setConfirmCleanup(false); await loadPending(); } catch (cause) { onError(String(cause)); } };
  const retention = history.retention_days === null ? "forever" : history.retention_days === 0 ? "never" : String(history.retention_days);
  return (
    <Section title={t("settings.privacy")} footer={<SaveBar saving={saving} dirty={dirty} t={t} onClick={() => void save()} />}>
      <p className="rounded-md border border-warning/25 bg-warning/10 p-3 text-sm leading-relaxed text-warning">{t("settings.pendingDisclosure")}</p>
      <Field label={t("settings.retention")}><Select value={retention} onValueChange={(value) => setHistory({ ...history, retention_days: value === "forever" ? null : Number(value) })}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="forever">{t("settings.forever")}</SelectItem><SelectItem value="30">{t("settings.days", { n: 30 })}</SelectItem><SelectItem value="7">{t("settings.days", { n: 7 })}</SelectItem><SelectItem value="never">{t("settings.never")}</SelectItem></SelectContent></Select></Field>
      <ToggleRow label={t("settings.keepAudio")} description={t("settings.keepAudioHint")} checked={history.keep_audio} onChange={(checked) => setHistory({ ...history, keep_audio: checked })} />
      <Field label={t("settings.pendingRetention")}><Select value={String(history.pending_retention_days ?? 7)} onValueChange={(value) => setHistory({ ...history, pending_retention_days: Number(value) })}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{[1, 3, 7, 14, 30].map((days) => <SelectItem key={days} value={String(days)}>{t("settings.days", { n: days })}</SelectItem>)}</SelectContent></Select></Field>
      <ToggleRow label={t("settings.appContext")} description={t("settings.appContextHint")} checked={context.include_app_context} onChange={(checked) => setContext({ ...context, include_app_context: checked })} />
      <ToggleRow label={t("settings.caretContext")} description={t("settings.caretContextHint")} checked={context.include_caret_context} onChange={(checked) => setContext({ ...context, include_caret_context: checked })} />
      <div className="flex items-center justify-between gap-3 border-t border-border pt-4"><span className="text-sm text-muted-foreground">{t("history.pendingTitle")}: {pendingCount}</span><Button type="button" variant="outline" size="sm" disabled={pendingCount === 0} onClick={() => setConfirmCleanup(true)}><Trash2 className="size-4" aria-hidden="true" />{t("settings.pendingCleanup")}</Button></div>
      <AlertDialog open={confirmCleanup} onOpenChange={setConfirmCleanup}><AlertDialogContent><AlertDialogHeader><AlertDialogTitle>{t("settings.pendingCleanup")}</AlertDialogTitle><AlertDialogDescription>{t("settings.pendingDisclosure")}</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel><AlertDialogAction onClick={() => void cleanup()}>{t("common.discard")}</AlertDialogAction></AlertDialogFooter></AlertDialogContent></AlertDialog>
    </Section>
  );
}

function AboutSettings({ settings, locale, t, onSave, onError }: Pick<Props, "settings" | "locale" | "t" | "onSave" | "onError">) {
  return (
    <Section title={t("settings.about")}>
      <div className="flex items-center justify-between gap-3"><div><p className="text-sm font-medium">oh-my-voice-type</p><p className="text-xs text-muted-foreground">{t("settings.version")} {packageJson.version}</p></div><Badge variant="outline">Tauri 2</Badge></div>
      <UpdatePanel settings={settings} locale={locale} t={t} onSave={onSave} onError={onError} />
      <Button type="button" variant="outline" onClick={() => void api.openSettingsFolder().catch((cause) => onError(String(cause)))}><ExternalLink className="size-4" aria-hidden="true" />{t("settings.dataFolder")}</Button>
    </Section>
  );
}

function HotkeyField({ label, value, locale, t, onChange, optional = false }: { label: string; value: string; locale: Locale; t: TFunc; onChange: (value: string) => void; optional?: boolean }) {
  return <Field label={label}><Select value={value} onValueChange={onChange}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{optional && <SelectItem value={NONE}>{t("common.none")}</SelectItem>}{HOTKEYS.map((key) => <SelectItem key={key} value={key}>{localizeHotkey(locale, key)}</SelectItem>)}</SelectContent></Select></Field>;
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return <label className="block min-w-0 flex-1"><span className="mb-1.5 block text-xs font-medium text-secondary-foreground">{label}</span>{children}{hint && <span className="mt-1.5 block text-xs leading-relaxed text-muted-foreground">{hint}</span>}</label>;
}

function ToggleRow({ label, description, checked, onChange, disabled = false }: { label: string; description?: string; checked: boolean; onChange: (checked: boolean) => void; disabled?: boolean }) {
  return <div className="flex items-center justify-between gap-4"><div><p className="text-sm font-medium text-secondary-foreground">{label}</p>{description && <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{description}</p>}</div><Switch checked={checked} onCheckedChange={onChange} disabled={disabled} aria-label={label} /></div>;
}

function SaveBar({ saving, dirty, t, onClick }: { saving: boolean; dirty: boolean; t: TFunc; onClick: () => void }) {
  return (
    <div className="flex items-center gap-3">
      {dirty && !saving && <span className="flex items-center gap-1.5 text-xs text-warning" role="status"><span className="size-1.5 rounded-full bg-warning" aria-hidden="true" />{t("settings.unsaved")}</span>}
      <Button type="button" onClick={onClick} disabled={saving || !dirty}>{saving ? <LoaderCircle className="size-4 animate-spin" aria-hidden="true" /> : <Check className="size-4" aria-hidden="true" />}{saving ? t("settings.saving") : t("settings.save")}</Button>
    </div>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="space-y-4 border-t border-border pt-5 first:border-t-0 first:pt-0">
      <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{title}</h3>
      {children}
    </div>
  );
}

/** Whether a section's local draft differs from the saved settings. */
function changed(draft: unknown, saved: unknown) {
  return JSON.stringify(draft) !== JSON.stringify(saved);
}

function clampNumber(value: number, min: number, max: number) { return Math.min(max, Math.max(min, Number.isFinite(value) ? value : min)); }

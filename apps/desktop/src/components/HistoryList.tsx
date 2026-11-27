import { useCallback, useEffect, useRef, useState } from "react";
import {
  BookPlus, Check, ChevronRight, Copy, Download, History as HistoryIcon, LoaderCircle,
  Pin, RotateCcw, Search, Trash2, TriangleAlert, X,
} from "lucide-react";
import {
  api, events, type HistoryEntry, type HistoryQuery, type Mode, type PendingRecord,
} from "../lib/api";
import { toast } from "sonner";
import { localizeHotkey, type Locale, type MessageKey, type TFunc } from "../i18n";
import { formatWhen } from "../lib/format";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Badge } from "./ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "./ui/sheet";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "./ui/alert-dialog";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "./ui/dialog";
import { PageHeader } from "./app/PageHeader";
import { EmptyState } from "./app/EmptyState";

const PAGE_SIZE = 50;

export default function HistoryList({ locale, t, hotkey = "right-alt" }: { locale: Locale; t: TFunc; hotkey?: string }) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [pending, setPending] = useState<PendingRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"all" | Mode>("all");
  const [delivery, setDelivery] = useState<"all" | NonNullable<HistoryEntry["delivery_status"]>>("all");
  const [pinnedOnly, setPinnedOnly] = useState(false);
  const [dateRange, setDateRange] = useState<"all" | "today" | "week">("all");
  const [loading, setLoading] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<HistoryEntry | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<HistoryEntry | "all" | null>(null);
  const [discardTarget, setDiscardTarget] = useState<PendingRecord | null>(null);
  const [dictionaryOpen, setDictionaryOpen] = useState(false);
  const [dictionaryTerm, setDictionaryTerm] = useState("");
  const [retrying, setRetrying] = useState<Set<string>>(() => new Set());
  const requestId = useRef(0);
  const entriesRef = useRef(0);
  useEffect(() => { entriesRef.current = entries.length; }, [entries.length]);

  const loadPending = useCallback(async () => {
    try { setPending(await api.pendingList()); } catch { setPending([]); }
  }, []);

  const load = useCallback(async (append = false) => {
    const id = ++requestId.current;
    append ? setLoadingMore(true) : setLoading(true);
    setError(null);
    const now = new Date();
    const from = dateRange === "today"
      ? new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
      : dateRange === "week" ? now.getTime() - 7 * 24 * 60 * 60 * 1000 : null;
    const request: HistoryQuery = {
      limit: PAGE_SIZE,
      offset: append ? entriesRef.current : 0,
      query: query.trim() || undefined,
      mode: mode === "all" ? null : mode,
      delivery: delivery === "all" ? null : delivery,
      pinned: pinnedOnly ? true : null,
      from_ms: from,
      to_ms: dateRange === "all" ? null : now.getTime(),
    };
    try {
      const page = await api.historyList(request);
      if (id !== requestId.current) return;
      setEntries((current) => append ? [...current, ...page.entries] : page.entries);
      setTotal(page.total);
      setLoaded(true);
    } catch (cause) {
      if (id === requestId.current) setError(String(cause));
    } finally {
      if (id === requestId.current) { setLoading(false); setLoadingMore(false); }
    }
  }, [dateRange, delivery, mode, pinnedOnly, query]);

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 250);
    return () => window.clearTimeout(timer);
  }, [load]);
  useEffect(() => { void loadPending(); }, [loadPending]);
  useEffect(() => {
    let active = true;
    let unsubscribe = () => {};
    void events.onPendingChanged(() => { if (active) void loadPending(); }).then((fn) => {
      if (active) unsubscribe = fn; else fn();
    });
    return () => { active = false; unsubscribe(); };
  }, [loadPending]);
  // A dictation finished while this page is open: show it without a reload.
  useEffect(() => {
    let active = true;
    let unsubscribe = () => {};
    void events.onSessionOutcome(() => { if (active) void load(); }).then((fn) => {
      if (active) unsubscribe = fn; else fn();
    });
    return () => { active = false; unsubscribe(); };
  }, [load]);

  // Action failures go to a toast; the inline banner is kept for load failures.
  const fail = (cause: unknown) => { toast.error(String(cause)); };
  const copy = async (text: string) => {
    try { await api.copyText(text); toast.success(t("common.copied")); } catch (cause) { fail(cause); }
  };
  const pin = async (entry: HistoryEntry) => {
    try {
      await api.historyPin(entry.id, !entry.pinned);
      setEntries((current) => current.map((item) => item.id === entry.id ? { ...item, pinned: !entry.pinned } : item));
      if (pinnedOnly) await load();
    } catch (cause) { fail(cause); }
  };
  const confirmDelete = async () => {
    if (!deleteTarget) return;
    try {
      if (deleteTarget === "all") await api.historyDeleteAll();
      else await api.historyDelete(deleteTarget.id);
      setDeleteTarget(null);
      setSelected(null);
      await load();
    } catch (cause) { fail(cause); }
  };
  const reprocess = async (entry: HistoryEntry) => {
    const entryMode: Mode = entry.mode === "translate" || entry.mode === "edit_selection" ? entry.mode : "dictate";
    try {
      await api.historyReprocess(entry.id, entryMode, entry.target_language);
      setSelected(null);
      await load();
    } catch (cause) { fail(cause); }
  };
  const addDictionary = async () => {
    if (!dictionaryTerm.trim()) return;
    try { await api.dictionaryAdd(dictionaryTerm.trim()); setDictionaryOpen(false); setDictionaryTerm(""); } catch (cause) { fail(cause); }
  };
  const retryPending = async (record: PendingRecord) => {
    if (retrying.has(record.id)) return;
    setRetrying((current) => new Set(current).add(record.id));
    try {
      await api.pendingRetry(record.id);
      toast.success(t("history.pendingRetried"));
      // The recovered text is a new history entry, not just a removed row.
      await load();
    } catch (cause) {
      fail(cause);
    } finally {
      setRetrying((current) => { const next = new Set(current); next.delete(record.id); return next; });
      await loadPending();
    }
  };
  // Dictionary entries are words, not whole sentences: offer the text the user
  // highlighted in the details sheet, otherwise start empty.
  const openDictionary = () => {
    setDictionaryTerm(window.getSelection()?.toString().trim().slice(0, 80) ?? "");
    setDictionaryOpen(true);
  };
  const exportAudio = async (id: string) => {
    try { if (await api.historyExportAudio(id)) toast.success(t("history.audioExported")); } catch (cause) { fail(cause); }
  };
  const discardPending = async () => {
    if (!discardTarget) return;
    try { await api.pendingDiscard(discardTarget.id); setDiscardTarget(null); await loadPending(); } catch (cause) { fail(cause); }
  };
  const clearFilters = () => { setQuery(""); setMode("all"); setDelivery("all"); setPinnedOnly(false); setDateRange("all"); };
  const filtered = query.trim() !== "" || mode !== "all" || delivery !== "all" || pinnedOnly || dateRange !== "all";

  return (
    <>
      <PageHeader
        title={t("history.title")}
        description={t("history.subtitle")}
        actions={entries.length > 0 ? <Button type="button" variant="outline" size="sm" onClick={() => setDeleteTarget("all")}><Trash2 className="size-4" aria-hidden="true" />{t("history.deleteAll")}</Button> : undefined}
      />

      <div className="rounded-lg border border-border bg-card p-4">
        <div className="flex flex-col gap-3 xl:flex-row xl:items-center">
          <div className="relative min-w-0 flex-1">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
            <Input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("history.searchPlaceholder")} className="pl-9" aria-label={t("history.searchPlaceholder")} />
          </div>
          <div className="grid grid-cols-2 gap-2 sm:flex">
            <FilterSelect value={mode} onChange={(value) => setMode(value as typeof mode)} label={t("history.filter.all")} options={[
              ["all", t("history.filter.all")], ["dictate", t("home.mode.dictate")], ["translate", t("home.mode.translate")], ["edit_selection", t("home.mode.edit_selection")],
            ]} />
            <FilterSelect value={delivery} onChange={(value) => setDelivery(value as typeof delivery)} label={t("history.filter.delivery")} options={[
              ["all", t("history.filter.delivery")], ["inserted", t("history.delivery.inserted")], ["copied", t("history.delivery.copied")], ["needs_review", t("history.delivery.needs_review")], ["failed", t("history.delivery.failed")],
            ]} />
            <FilterSelect value={dateRange} onChange={(value) => setDateRange(value as typeof dateRange)} label={t("history.filter.date")} options={[
              ["all", t("history.filter.date")], ["today", t("history.filter.today")], ["week", t("history.filter.week")],
            ]} />
            <Button type="button" variant={pinnedOnly ? "secondary" : "outline"} size="sm" className="col-span-2 sm:col-span-1" onClick={() => setPinnedOnly((value) => !value)} aria-pressed={pinnedOnly}>
              <Pin className="size-4" aria-hidden="true" />{t("history.filter.pinned")}
            </Button>
          </div>
        </div>
        <div className="mt-3 flex min-h-8 items-center justify-between gap-3 text-xs text-muted-foreground">
          <span aria-live="polite">{loaded ? (total === 1 ? t("history.count.one") : t("history.count.other", { n: total })) : ""}</span>
          {filtered && <Button type="button" variant="link" size="sm" onClick={clearFilters}><X className="size-3.5" aria-hidden="true" />{t("common.clearFilters")}</Button>}
        </div>
      </div>

      {error && <p className="mt-4 rounded-md border border-destructive/25 bg-destructive/10 p-3 text-sm text-destructive" role="alert">{error}</p>}

      {pending.length > 0 && (
        <section className="mt-6">
          <h2 className="mb-3 text-sm font-semibold">{t("history.pendingTitle")}</h2>
          <div className="flex flex-col gap-2">
            {pending.map((record) => {
              const text = record.final_text || record.raw_text;
              // Without audio or a transcript there is nothing to retry.
              const retryable = record.has_audio || text !== "";
              const busy = retrying.has(record.id);
              return (
                <div key={record.id} className="flex flex-wrap items-center gap-3 rounded-lg border border-warning/25 bg-warning/10 p-3">
                  <TriangleAlert className="size-4 shrink-0 text-warning" aria-hidden="true" />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium">{text || t("history.notInserted")}</p>
                    <p className="mt-0.5 text-xs text-muted-foreground">{formatWhen(record.created_at_ms, locale)} · {t("history.pendingStage", { stage: stageLabel(record.stage, t) })}</p>
                    {!retryable
                      ? <p className="mt-1 text-xs text-warning">{t("history.pendingNoAudio")}</p>
                      : record.error && <p className="mt-1 line-clamp-2 break-words text-xs text-warning">{record.error}</p>}
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    <Button type="button" variant="outline" size="sm" disabled={!retryable || busy} onClick={() => void retryPending(record)}>{busy ? <LoaderCircle className="size-4 animate-spin" aria-hidden="true" /> : <RotateCcw className="size-4" aria-hidden="true" />}{t("history.pendingRetry")}</Button>
                    <Button type="button" variant="ghost" size="sm" disabled={text === ""} onClick={() => void copy(text)}><Copy className="size-4" aria-hidden="true" />{t("history.pendingCopy")}</Button>
                    <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => setDiscardTarget(record)}><Trash2 className="size-4" aria-hidden="true" />{t("history.pendingDiscard")}</Button>
                  </div>
                </div>
              );
            })}
          </div>
        </section>
      )}

      <div className="mt-6">
        {loading ? (
          <div className="flex items-center justify-center gap-2 py-12 text-sm text-muted-foreground" role="status"><LoaderCircle className="size-4 animate-spin" aria-hidden="true" />{t("history.loading")}</div>
        ) : entries.length === 0 ? (
          filtered
            ? <EmptyState icon={Search} title={t("history.emptyFiltered")} description={t("history.emptyFilteredHint")} action={<Button type="button" variant="outline" size="sm" onClick={clearFilters}>{t("common.clearFilters")}</Button>} />
            : <EmptyState icon={HistoryIcon} title={t("history.emptyTitle")} description={t("history.empty", { hotkey: localizeHotkey(locale, hotkey) })} />
        ) : (
          <ul className="flex flex-col gap-2">
            {entries.map((entry) => (
              <li key={entry.id} className={cn("rounded-lg border bg-card p-4 transition-colors hover:border-input", entry.pinned ? "border-primary/30" : "border-border")}>
                <Button type="button" variant="ghost" onClick={() => setSelected(entry)} className="block h-auto w-full whitespace-normal rounded-none p-0 text-left font-normal hover:bg-transparent">
                  <p className="line-clamp-3 whitespace-pre-wrap text-sm leading-relaxed text-secondary-foreground">{entry.final_text || entry.raw_text}</p>
                </Button>
                <div className="mt-3 flex flex-wrap items-center gap-2">
                  <Badge variant="secondary">{modeLabel(entry.mode, t)}</Badge>
                  <span className="text-xs text-muted-foreground">{formatWhen(entry.created_at_ms, locale)}{entry.app_name ? ` · ${entry.app_name}` : ""}</span>
                  <DeliveryBadge entry={entry} t={t} />
                  <div className="ml-auto flex items-center gap-0.5">
                    <Button type="button" variant="ghost" size="sm" onClick={() => void copy(entry.final_text || entry.raw_text)}><Copy className="size-4" aria-hidden="true" />{t("history.copy")}</Button>
                    <Button type="button" variant="ghost" size="icon" className={cn("size-8", entry.pinned && "text-primary")} onClick={() => void pin(entry)} aria-pressed={entry.pinned} aria-label={t(entry.pinned ? "history.unpin" : "history.pin")} title={t(entry.pinned ? "history.unpin" : "history.pin")}><Pin className={cn("size-4", entry.pinned && "fill-current")} aria-hidden="true" /></Button>
                    <Button type="button" variant="ghost" size="icon" className="size-8" onClick={() => setSelected(entry)} aria-label={t("common.details")} title={t("common.details")}><ChevronRight className="size-4" aria-hidden="true" /></Button>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        )}
        {!loading && entries.length < total && <div className="mt-4 flex justify-center"><Button type="button" variant="outline" onClick={() => void load(true)} disabled={loadingMore}>{loadingMore && <LoaderCircle className="size-4 animate-spin" aria-hidden="true" />}{t("history.loadMore")}</Button></div>}
      </div>

      <HistoryDetailSheet
        entry={selected}
        locale={locale}
        t={t}
        onClose={() => setSelected(null)}
        onCopy={(text) => void copy(text)}
        onDelete={(entry) => setDeleteTarget(entry)}
        onReprocess={(entry) => void reprocess(entry)}
        onExportAudio={(id) => void exportAudio(id)}
        onAddDictionary={openDictionary}
      />

      <AlertDialog open={deleteTarget !== null} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>{t("common.delete")}</AlertDialogTitle><AlertDialogDescription>{t("history.deleteConfirm")}</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel><AlertDialogAction onClick={() => void confirmDelete()} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">{t("common.delete")}</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <AlertDialog open={discardTarget !== null} onOpenChange={(open) => !open && setDiscardTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>{t("history.pendingDiscard")}</AlertDialogTitle><AlertDialogDescription>{t("history.deleteConfirm")}</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel><AlertDialogAction onClick={() => void discardPending()} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">{t("common.discard")}</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <Dialog open={dictionaryOpen} onOpenChange={setDictionaryOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("history.addDictionary")}</DialogTitle><DialogDescription>{t("settings.dictionaryHint")}</DialogDescription></DialogHeader>
          <Input value={dictionaryTerm} onChange={(event) => setDictionaryTerm(event.target.value)} placeholder={t("settings.dictionaryPlaceholder")} />
          <DialogFooter><Button type="button" variant="outline" onClick={() => setDictionaryOpen(false)}>{t("common.cancel")}</Button><Button type="button" onClick={() => void addDictionary()} disabled={!dictionaryTerm.trim()}><BookPlus className="size-4" aria-hidden="true" />{t("common.add")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

function HistoryDetailSheet({ entry, locale, t, onClose, onCopy, onDelete, onReprocess, onExportAudio, onAddDictionary }: {
  entry: HistoryEntry | null;
  locale: Locale;
  t: TFunc;
  onClose: () => void;
  onCopy: (text: string) => void;
  onDelete: (entry: HistoryEntry) => void;
  onReprocess: (entry: HistoryEntry) => void;
  onExportAudio: (id: string) => void;
  onAddDictionary: () => void;
}) {
  return (
    <Sheet open={entry !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent className="w-full overflow-y-auto sm:max-w-lg">
        {entry && <SheetHeader><SheetTitle>{t("common.details")}</SheetTitle><SheetDescription>{new Date(entry.created_at_ms).toLocaleString(locale)} · {modeLabel(entry.mode, t)}{entry.app_name ? ` · ${entry.app_name}` : ""}</SheetDescription></SheetHeader>}
        {entry && <div className="mt-6 space-y-5">
          <DetailBlock title={t("history.final")} text={entry.final_text || entry.raw_text} />
          {entry.raw_text && entry.raw_text !== entry.final_text && <DetailBlock title={t("history.raw")} text={entry.raw_text} />}
          <dl className="grid grid-cols-2 gap-3 text-sm">
            <div><dt className="text-xs text-muted-foreground">ASR</dt><dd>{entry.asr_ms == null ? "—" : `${entry.asr_ms} ms`}</dd></div>
            <div><dt className="text-xs text-muted-foreground">LLM</dt><dd>{entry.cleanup_ms == null ? "—" : `${entry.cleanup_ms} ms`}</dd></div>
            <div><dt className="text-xs text-muted-foreground">{t("history.filter.delivery")}</dt><dd><DeliveryBadge entry={entry} t={t} /></dd></div>
            <div><dt className="text-xs text-muted-foreground">{t("home.translateTarget")}</dt><dd>{entry.target_language || "—"}</dd></div>
          </dl>
          {entry.warnings && entry.warnings.length > 0 && <div className="rounded-md border border-warning/25 bg-warning/10 p-3 text-sm text-warning">{entry.warnings.join("\n")}</div>}
          <div className="flex flex-wrap gap-2 border-t border-border pt-4">
            <Button type="button" variant="outline" size="sm" onClick={() => onCopy(entry.final_text || entry.raw_text)}><Copy className="size-4" aria-hidden="true" />{t("history.copy")}</Button>
            <Button type="button" variant="outline" size="sm" onClick={() => onReprocess(entry)} disabled={entry.mode === "edit_selection"}><RotateCcw className="size-4" aria-hidden="true" />{t("history.reprocess")}</Button>
            <Button type="button" variant="outline" size="sm" onClick={onAddDictionary}><BookPlus className="size-4" aria-hidden="true" />{t("history.addDictionary")}</Button>
            {entry.audio_path && <Button type="button" variant="outline" size="sm" onClick={() => onExportAudio(entry.id)}><Download className="size-4" aria-hidden="true" />{t("history.exportAudio")}</Button>}
            <Button type="button" variant="ghost" size="sm" className="text-destructive" onClick={() => onDelete(entry)}><Trash2 className="size-4" aria-hidden="true" />{t("common.delete")}</Button>
          </div>
        </div>}
      </SheetContent>
    </Sheet>
  );
}

function DetailBlock({ title, text }: { title: string; text: string }) {
  return <section><h3 className="mb-2 text-xs font-medium text-muted-foreground">{title}</h3><p className="whitespace-pre-wrap rounded-md border border-border bg-muted/40 p-3 text-sm leading-relaxed text-secondary-foreground">{text}</p></section>;
}

function DeliveryBadge({ entry, t }: { entry: HistoryEntry; t: TFunc }) {
  const status = entry.delivery_status ?? (entry.insert_ok ? "inserted" : "failed");
  const labels = { inserted: "history.delivery.inserted", copied: "history.delivery.copied", needs_review: "history.delivery.needs_review", failed: "history.delivery.failed" } as const;
  return <Badge variant={status === "inserted" ? "outline" : "secondary"} className={cn(status === "inserted" && "border-success/25 bg-success/10 text-success", status === "failed" && "border-destructive/25 bg-destructive/10 text-destructive", status === "needs_review" && "border-warning/25 bg-warning/10 text-warning")}>{status === "inserted" && <Check className="size-3" aria-hidden="true" />}{t(labels[status])}</Badge>;
}

function FilterSelect({ value, onChange, label, options }: { value: string; onChange: (value: string) => void; label: string; options: Array<[string, string]> }) {
  return <Select value={value} onValueChange={onChange}><SelectTrigger className="h-9 min-w-36" aria-label={label}><SelectValue /></SelectTrigger><SelectContent>{options.map(([id, text]) => <SelectItem key={id} value={id}>{text}</SelectItem>)}</SelectContent></Select>;
}

const stageKeys: Record<string, MessageKey> = {
  capturing: "pending.stage.capturing",
  queued: "pending.stage.queued",
  transcribing: "pending.stage.transcribing",
  rewriting: "pending.stage.rewriting",
  needs_delivery: "pending.stage.needs_delivery",
  delivering: "pending.stage.delivering",
  completed: "pending.stage.completed",
  failed: "pending.stage.failed",
  interrupted: "pending.stage.interrupted",
  needs_review: "pending.stage.needs_review",
};

function stageLabel(stage: string, t: TFunc) {
  const key = stageKeys[stage];
  return key ? t(key) : stage.replace(/_/g, " ");
}

function modeLabel(mode: string, t: TFunc) {
  if (mode === "translate") return t("home.mode.translate");
  if (mode === "edit_selection") return t("home.mode.edit_selection");
  return t("home.mode.dictate");
}

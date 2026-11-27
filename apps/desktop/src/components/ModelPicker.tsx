import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Check, ChevronsUpDown, Loader2, RefreshCw } from "lucide-react";
import { api, type ModelListOutcome, type ProfileRole, type ProviderModel } from "../lib/api";
import type { TFunc } from "../i18n";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "./ui/command";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";

interface ModelPickerProps {
  role: ProfileRole;
  protocol: "open_ai" | "anthropic";
  baseUrl: string | null;
  allowInsecureHttp: boolean;
  /** True for custom / self-hosted servers whose model names may not match standard keywords. */
  allowUnfilteredFallback?: boolean;
  apiKey: string;
  profileId?: string | null;
  hasSavedKey: boolean;
  value: string;
  fallbackModels: string[];
  preferredDefault?: string;
  /** True when the endpoint supports live `GET /models` discovery. */
  supportsDiscovery?: boolean;
  disabled: boolean;
  t: TFunc;
  onChange: (model: string) => void;
}

/**
 * Editable model selector with live `GET /models` discovery.
 *
 * Automatically queries the provider's available models once a valid API key
 * is entered (or when editing a profile that already has a key in the vault),
 * while gracefully falling back to preset models if the endpoint does not list
 * models for this role.
 */
export function ModelPicker({
  role,
  protocol,
  baseUrl,
  allowInsecureHttp,
  allowUnfilteredFallback = false,
  apiKey,
  profileId,
  hasSavedKey,
  value,
  fallbackModels,
  preferredDefault,
  supportsDiscovery = true,
  disabled,
  t,
  onChange,
}: ModelPickerProps) {
  const inputId = useId();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [liveModels, setLiveModels] = useState<ProviderModel[] | null>(null);
  const [notice, setNotice] = useState<Exclude<ModelListOutcome, "ok"> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestSeq = useRef(0);
  const userTouchedModel = useRef(false);
  const valueRef = useRef(value);
  valueRef.current = value;

  const trimmedKey = apiKey.trim();
  const trimmedUrl = (baseUrl ?? "").trim();
  const hasUsableEndpoint =
    Boolean(trimmedUrl) &&
    (trimmedUrl.startsWith("https://") || trimmedUrl.startsWith("http://"));
  const canQueryWithNewKey = trimmedKey.length >= 8;
  const canQueryWithSavedKey = !trimmedKey && hasSavedKey && Boolean(profileId);
  const canFetch =
    supportsDiscovery &&
    hasUsableEndpoint &&
    (canQueryWithNewKey || canQueryWithSavedKey);

  // Reset live cache when switching endpoint/protocol/role
  useEffect(() => {
    requestSeq.current += 1;
    setLiveModels(null);
    setNotice(null);
    setError(null);
    setLoading(false);
    userTouchedModel.current = false;
  }, [role, protocol, trimmedUrl]);

  // Clear stale error/cache whenever the key changes or becomes unable to query
  useEffect(() => {
    setError(null);
    setNotice(null);
    if (!canFetch) {
      requestSeq.current += 1;
      setLiveModels(null);
      setLoading(false);
    }
  }, [trimmedKey, canFetch]);

  const fetchLiveModels = useCallback(
    async (_explicit: boolean) => {
      if (!supportsDiscovery || !hasUsableEndpoint) return;
      if (!trimmedKey && !(hasSavedKey && profileId)) return;

      const seq = ++requestSeq.current;
      setLoading(true);
      setError(null);
      setNotice(null);
      try {
        const result = await api.listModels({
          role,
          protocol,
          base_url: trimmedUrl,
          allow_insecure_http: allowInsecureHttp,
          allow_unfiltered_fallback: allowUnfilteredFallback,
          api_key: trimmedKey || null,
          profile_id: trimmedKey ? null : (profileId ?? null),
        });
        if (requestSeq.current !== seq) return;
        if (result.outcome === "ok" && result.models.length > 0) {
          setLiveModels(result.models);
          setNotice(null);
          if (valueRef.current.trim() === "" && !userTouchedModel.current) {
            const preferred =
              preferredDefault && result.models.some((item) => item.id === preferredDefault)
                ? preferredDefault
                : result.models[0].id;
            onChange(preferred);
          }
        } else if (result.outcome === "unsupported") {
          setLiveModels(null);
          setNotice("unsupported");
        } else {
          setLiveModels(null);
          setNotice("no_matching_models");
        }
      } catch (cause) {
        if (requestSeq.current !== seq) return;
        setLiveModels(null);
        setNotice(null);
        setError(String(cause));
      } finally {
        if (requestSeq.current === seq) {
          setLoading(false);
        }
      }
    },
    [
      allowInsecureHttp,
      allowUnfilteredFallback,
      hasSavedKey,
      hasUsableEndpoint,
      onChange,
      preferredDefault,
      profileId,
      protocol,
      role,
      supportsDiscovery,
      trimmedKey,
      trimmedUrl,
    ],
  );

  // Auto-fetch debounced when a valid key (or saved key) + endpoint is ready
  useEffect(() => {
    if (!canFetch) return;
    const delay = canQueryWithSavedKey ? 80 : 650;
    const timer = window.setTimeout(() => {
      void fetchLiveModels(false);
    }, delay);
    return () => window.clearTimeout(timer);
  }, [canFetch, canQueryWithSavedKey, fetchLiveModels]);

  const hasLiveModels = liveModels !== null && liveModels.length > 0;
  const options: ProviderModel[] = hasLiveModels
    ? liveModels
    : fallbackModels.map((id) => ({ id }));
  const isLive = hasLiveModels;
  const trimmedSearch = search.trim();
  const searchMatchesExact = options.some(
    (item) => item.id.toLowerCase() === trimmedSearch.toLowerCase(),
  );

  const selectModel = (modelId: string) => {
    userTouchedModel.current = true;
    onChange(modelId);
    setOpen(false);
    setSearch("");
  };

  return (
    <div className="space-y-1.5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Label htmlFor={inputId}>{t("settings.asr.model")}</Label>
        {supportsDiscovery && (
          <div className="flex items-center gap-2">
            {isLive && !loading && (
              <span className="text-[11px] font-medium text-success" role="status">
                {t("profiles.models.loadedCount", { n: liveModels.length })}
              </span>
            )}
            <button
              type="button"
              disabled={disabled || loading || (!trimmedKey && !(hasSavedKey && profileId)) || !hasUsableEndpoint}
              onClick={() => void fetchLiveModels(true)}
              className="inline-flex items-center gap-1 text-xs font-medium text-primary transition-colors hover:underline disabled:cursor-not-allowed disabled:text-muted-foreground disabled:no-underline"
            >
              {loading ? (
                <Loader2 className="size-3 animate-spin" aria-hidden="true" />
              ) : (
                <RefreshCw className="size-3" aria-hidden="true" />
              )}
              {loading ? t("profiles.models.fetching") : t("profiles.models.fetch")}
            </button>
          </div>
        )}
      </div>

      <div className="flex gap-1.5">
        <Input
          id={inputId}
          value={value}
          disabled={disabled}
          spellCheck={false}
          placeholder={t("profiles.modelPlaceholder")}
          onChange={(event) => {
            userTouchedModel.current = true;
            onChange(event.target.value);
          }}
          className="min-w-0 flex-1 font-mono text-xs"
        />

        {(options.length > 0 || supportsDiscovery) && (
          <Popover open={open} onOpenChange={setOpen} modal>
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="outline"
                size="icon"
                role="combobox"
                aria-expanded={open}
                aria-label={t("profiles.models.choose")}
                title={t("profiles.models.choose")}
                disabled={disabled || (options.length === 0 && !loading)}
                className="shrink-0"
              >
                {loading ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                ) : (
                  <ChevronsUpDown className="size-4 opacity-70" aria-hidden="true" />
                )}
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-80 p-0" align="end">
              <Command>
                <CommandInput
                  value={search}
                  onValueChange={setSearch}
                  placeholder={t("profiles.models.searchPlaceholder")}
                />
                <CommandList>
                  <CommandEmpty>
                    {trimmedSearch ? (
                      <button
                        type="button"
                        onClick={() => selectModel(trimmedSearch)}
                        className="text-xs font-medium text-primary hover:underline"
                      >
                        {t("profiles.models.useCustom", { id: trimmedSearch })}
                      </button>
                    ) : (
                      t("profiles.models.empty")
                    )}
                  </CommandEmpty>
                  {options.length > 0 && (
                    <CommandGroup
                      heading={t(isLive ? "profiles.models.liveGroup" : "profiles.models.fallbackGroup")}
                    >
                      {options.map((item) => {
                        const selected = value.trim() === item.id;
                        return (
                          <CommandItem
                            key={item.id}
                            value={`${item.id} ${item.label ?? ""}`}
                            onSelect={() => selectModel(item.id)}
                          >
                            <Check
                              className={cn("size-3.5 shrink-0", selected ? "opacity-100" : "opacity-0")}
                              aria-hidden="true"
                            />
                            <div className="min-w-0 flex-1">
                              <div className="truncate font-mono text-xs">{item.id}</div>
                              {item.label && (
                                <div className="truncate text-[11px] text-muted-foreground">
                                  {item.label}
                                </div>
                              )}
                            </div>
                          </CommandItem>
                        );
                      })}
                    </CommandGroup>
                  )}
                  {trimmedSearch && !searchMatchesExact && options.length > 0 && (
                    <CommandGroup>
                      <CommandItem
                        value={`__custom__ ${trimmedSearch}`}
                        onSelect={() => selectModel(trimmedSearch)}
                      >
                        <span className="truncate text-xs text-primary">
                          {t("profiles.models.useCustom", { id: trimmedSearch })}
                        </span>
                      </CommandItem>
                    </CommandGroup>
                  )}
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
        )}
      </div>

      {error ? (
        <p className="text-xs leading-relaxed text-destructive" role="alert">
          {t("profiles.models.fetchError", { message: error })}
        </p>
      ) : notice === "unsupported" ? (
        <p className="text-xs leading-relaxed text-muted-foreground" role="status">
          {t("profiles.models.unsupported")}
        </p>
      ) : notice === "no_matching_models" ? (
        <p className="text-xs leading-relaxed text-muted-foreground" role="status">
          {t(role === "asr" ? "profiles.models.noAsrModels" : "profiles.models.noLlmModels")}
        </p>
      ) : supportsDiscovery && !isLive && !loading && !trimmedKey && !hasSavedKey ? (
        <p className="text-xs leading-relaxed text-muted-foreground">
          {t("profiles.models.enterKeyToLoad")}
        </p>
      ) : null}
    </div>
  );
}

import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MessageKey } from "../i18n";

export type ThemePreference = "system" | "light" | "dark";
type ResolvedTheme = "light" | "dark";

export interface ThemeOption {
  value: ThemePreference;
  label: MessageKey;
}

export const themeOptions: ThemeOption[] = [
  { value: "system", label: "settings.theme.system" },
  { value: "light", label: "settings.theme.light" },
  { value: "dark", label: "settings.theme.dark" },
];

export function normalizeThemePreference(
  value: string | null | undefined,
): ThemePreference {
  return value === "light" || value === "dark" ? value : "system";
}

function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference !== "system") return preference;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function useTheme(preference: string | null | undefined) {
  const normalized = normalizeThemePreference(preference);

  useEffect(() => {
    const apply = () => {
      const resolved = resolveTheme(normalized);
      document.documentElement.dataset.theme = resolved;
      document.documentElement.style.colorScheme = resolved;
    };

    apply();
    if (isTauri()) {
      void getCurrentWindow().setTheme(normalized === "system" ? null : normalized);
    }

    if (normalized !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [normalized]);
}

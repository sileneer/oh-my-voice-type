import { TriangleAlert } from "lucide-react";
import type { ApiStatus } from "@/lib/api";
import type { TFunc } from "@/i18n";
import { Button } from "@/components/ui/button";

export default function SetupBanner({
  status,
  t,
  onOpenSettings,
}: {
  status: ApiStatus;
  t: TFunc;
  onOpenSettings: () => void;
}) {
  const state = status.asr.state;
  const messageKey = state === "ready"
    ? "setup.state.ready"
    : state === "missing_profile"
      ? "setup.state.missingProfile"
      : state === "missing_key"
        ? "setup.state.missingKey"
        : state === "vault_error"
          ? "setup.state.vaultError"
          : "setup.state.invalidConfig";
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-warning/25 bg-warning/10 p-4" role="status">
      <div className="flex min-w-0 items-start gap-3">
        <TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden="true" />
        <div>
          <p className="text-sm font-medium text-foreground">{t("setup.title")}</p>
          <p className="mt-1 text-sm leading-relaxed text-muted-foreground">{t(messageKey)}</p>
        </div>
      </div>
      <Button type="button" variant="outline" size="sm" onClick={onOpenSettings}>{t("setup.openSettings")}</Button>
    </div>
  );
}

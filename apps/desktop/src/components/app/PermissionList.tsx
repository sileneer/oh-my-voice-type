import { Check, RefreshCw, TriangleAlert } from "lucide-react";
import { localizePermissionName, localizePermissionValue, type Locale, type TFunc } from "@/i18n";
import { Button } from "@/components/ui/button";

export function PermissionList({
  permissions,
  locale,
  t,
  onRefresh,
  refreshing = false,
}: {
  permissions: Record<string, string>;
  locale: Locale;
  t: TFunc;
  onRefresh: () => void;
  refreshing?: boolean;
}) {
  const entries = Object.entries(permissions).filter(([key]) => key !== "platform");
  return (
    <section className="rounded-lg border border-border bg-card p-4">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-semibold">{t("permissions.title")}</h2>
        <Button type="button" variant="ghost" size="sm" onClick={onRefresh} disabled={refreshing}>
          <RefreshCw className={refreshing ? "size-4 animate-spin" : "size-4"} aria-hidden="true" />
          {t("common.retry")}
        </Button>
      </div>
      <ul className="mt-3 divide-y divide-border">
        {entries.map(([key, value]) => {
          const ok = value === "granted" || value === "not_required" || value.startsWith("granted_") || value.startsWith("av_");
          const Icon = ok ? Check : TriangleAlert;
          return (
            <li key={key} className="flex items-center justify-between gap-3 py-2.5 text-sm">
              <span className="flex items-center gap-2 text-secondary-foreground">
                <Icon className={ok ? "size-4 text-success" : "size-4 text-warning"} aria-hidden="true" />
                {localizePermissionName(locale, key)}
              </span>
              <span className={ok ? "text-success" : "text-warning"}>{localizePermissionValue(t, value)}</span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

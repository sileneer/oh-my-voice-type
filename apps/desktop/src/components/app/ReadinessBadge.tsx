import { CheckCircle2, CircleAlert, KeyRound, PlugZap } from "lucide-react";
import type { ReadinessState } from "@/lib/api";
import type { MessageKey, TFunc } from "@/i18n";
import { cn } from "@/lib/utils";

const labels: Record<ReadinessState, MessageKey> = {
  ready: "profiles.state.ready",
  missing_profile: "profiles.state.missing_profile",
  missing_key: "profiles.state.missing_key",
  vault_error: "profiles.state.vault_error",
  invalid_config: "profiles.state.invalid_config",
};

export function ReadinessBadge({
  state,
  t,
  compact = false,
}: {
  state: ReadinessState;
  t: TFunc;
  compact?: boolean;
}) {
  const ready = state === "ready";
  const Icon = ready
    ? CheckCircle2
    : state === "missing_key"
      ? KeyRound
      : state === "missing_profile"
        ? PlugZap
        : CircleAlert;
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full border px-2 py-1 text-xs font-medium",
        ready
          ? "border-success/25 bg-success/10 text-success"
          : state === "missing_profile"
            ? "border-border bg-muted text-muted-foreground"
            : "border-warning/25 bg-warning/10 text-warning",
        compact && "px-1.5 py-0.5 text-[11px]",
      )}
    >
      <Icon className="size-3.5" aria-hidden="true" />
      {t(labels[state])}
    </span>
  );
}

import { AudioLines, History, Home, Settings, type LucideIcon } from "lucide-react";
import packageJson from "../../../package.json";
import type { ApiStatus } from "@/lib/api";
import type { MessageKey, TFunc } from "@/i18n";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ReadinessBadge } from "./ReadinessBadge";

type Tab = "home" | "history" | "settings";

const items: Array<{ id: Tab; label: MessageKey; icon: LucideIcon }> = [
  { id: "home", label: "tab.home", icon: Home },
  { id: "history", label: "tab.history", icon: History },
  { id: "settings", label: "tab.settings", icon: Settings },
];

export function Sidebar({
  tab,
  apiStatus,
  pendingCount,
  t,
  onNavigate,
}: {
  tab: Tab;
  apiStatus: ApiStatus;
  pendingCount: number;
  t: TFunc;
  onNavigate: (tab: Tab) => void;
}) {
  return (
    <aside className="flex w-14 shrink-0 flex-col border-r border-border bg-card md:w-44 xl:w-52">
      <div className="flex h-16 items-center gap-2.5 px-3 md:px-4">
        <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
          <AudioLines className="size-5" aria-hidden="true" />
        </div>
        <div className="hidden min-w-0 md:block">
          <p className="truncate text-sm font-semibold tracking-tight">oh-my-voice-type</p>
          <p className="text-[11px] text-muted-foreground">v{packageJson.version}</p>
        </div>
      </div>

      <nav className="flex flex-col gap-1 px-2 py-3" aria-label={t("nav.primary")}>
        {items.map(({ id, label, icon: Icon }) => (
          <Button
            key={id}
            type="button"
            variant="ghost"
            onClick={() => onNavigate(id)}
            aria-current={tab === id ? "page" : undefined}
            title={t(label)}
            className={cn(
              "flex h-10 items-center gap-3 rounded-md px-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              tab === id
                ? "bg-secondary text-secondary-foreground"
                : "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
            )}
          >
            <Icon className="size-4 shrink-0" aria-hidden="true" />
            <span className="hidden md:inline">{t(label)}</span>
            {id === "history" && pendingCount > 0 && (
              <span className="ml-auto hidden rounded-full bg-warning/15 px-2 py-0.5 text-[11px] font-semibold text-warning md:inline">
                {pendingCount}
              </span>
            )}
          </Button>
        ))}
      </nav>

      <div className="mt-auto hidden border-t border-border p-3 md:block">
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between gap-2 text-xs">
            <span className="text-muted-foreground">ASR</span>
            <ReadinessBadge state={apiStatus.asr.state} t={t} compact />
          </div>
          <div className="flex items-center justify-between gap-2 text-xs">
            <span className="text-muted-foreground">LLM</span>
            <ReadinessBadge state={apiStatus.llm.state} t={t} compact />
          </div>
        </div>
      </div>
    </aside>
  );
}

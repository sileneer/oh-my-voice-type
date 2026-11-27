import type { ReactNode } from "react";
import type { ApiStatus } from "@/lib/api";
import type { TFunc } from "@/i18n";
import { Sidebar } from "./Sidebar";

export function AppShell({
  tab,
  apiStatus,
  activeAsrLabel,
  activeLlmLabel,
  pendingCount,
  t,
  onNavigate,
  onOpenProviders,
  children,
}: {
  tab: "home" | "history" | "settings";
  apiStatus: ApiStatus;
  activeAsrLabel?: string;
  activeLlmLabel?: string;
  pendingCount: number;
  t: TFunc;
  onNavigate: (tab: "home" | "history" | "settings") => void;
  onOpenProviders: () => void;
  children: ReactNode;
}) {
  return (
    <div className="flex h-full min-h-0 bg-background text-foreground">
      <Sidebar
        tab={tab}
        apiStatus={apiStatus}
        activeAsrLabel={activeAsrLabel}
        activeLlmLabel={activeLlmLabel}
        pendingCount={pendingCount}
        t={t}
        onNavigate={onNavigate}
        onOpenProviders={onOpenProviders}
      />
      <main className="min-w-0 flex-1 overflow-auto" id="main-content">
        <div className="mx-auto w-full max-w-[860px] px-5 py-7 md:px-8 md:py-9">{children}</div>
      </main>
    </div>
  );
}

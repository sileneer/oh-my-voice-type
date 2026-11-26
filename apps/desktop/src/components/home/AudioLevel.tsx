import { cn } from "@/lib/utils";

export function AudioLevel({ level, active }: { level: number; active: boolean }) {
  const bars = 12;
  return (
    <div className="flex h-8 items-center gap-1" aria-hidden="true">
      {Array.from({ length: bars }).map((_, index) => {
        const threshold = (index + 1) / (bars + 1);
        const on = active && level > threshold;
        return (
          <span
            key={index}
            className={cn(
              "w-1 rounded-full transition-[height,background-color] duration-100 motion-reduce:transition-none",
              on ? "bg-primary" : "bg-border",
            )}
            style={{ height: `${6 + index * 1.4}px` }}
          />
        );
      })}
    </div>
  );
}

import { toast } from "sonner";

// While the main window is occluded or hidden, WebView2 throttles timers, so
// sonner's auto-dismiss never fires and repeated warnings pile up into a wall
// of stale toasts (every silent take added one during testing). Give each
// message a stable id so repeats update one toast in place, and sweep toasts
// that went stale while the window could not run timers.
const DEFAULT_DURATION_MS = 4_000;
const GRACE_MS = 1_000;

const emitted = new Map<string, { at: number; duration: number }>();

type Kind = "info" | "success" | "warning" | "error";

export type NotifyOptions = {
  duration?: number;
  action?: { label: string; onClick: () => void };
};

function prune(now: number) {
  for (const [id, entry] of emitted) {
    if (now - entry.at > entry.duration + GRACE_MS) {
      toast.dismiss(id);
      emitted.delete(id);
    }
  }
}

function emit(kind: Kind, message: string, options?: NotifyOptions) {
  const now = Date.now();
  prune(now);
  // Keyed by kind too, so a success and an error with the same text stay
  // separate toasts.
  const id = `${kind}:${message}`;
  // Toasts with an action are left to sonner's own timer, which pauses while
  // hovered: a wall-clock sweep could close one under the cursor.
  if (options?.action) emitted.delete(id);
  else emitted.set(id, { at: now, duration: options?.duration ?? DEFAULT_DURATION_MS });
  toast[kind](message, { id, ...options });
}

function onVisibilityChange() {
  if (document.visibilityState === "visible") prune(Date.now());
}

let sweeperInstalled = false;

/** Call once from the app root; safe to call again. */
export function installToastSweeper() {
  if (sweeperInstalled) return;
  sweeperInstalled = true;
  document.addEventListener("visibilitychange", onVisibilityChange);
}

export const notify = {
  info: (message: string, options?: NotifyOptions) => emit("info", message, options),
  success: (message: string) => emit("success", message),
  warning: (message: string) => emit("warning", message),
  error: (message: string) => emit("error", message),
};

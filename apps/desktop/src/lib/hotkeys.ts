/**
 * Activation keys the native hook can actually bind on this platform.
 * Offering the other platform's keys (e.g. "fn" on Windows) let users save a
 * key the backend then failed to install, leaving no working hotkey at all.
 * Must stay in sync with `parse_hotkey_key` (windows.rs) and
 * `flags_mask_for` (macos.rs).
 */
const WINDOWS_HOTKEYS = ["right-alt", "right-ctrl", "left-alt", "left-ctrl", "f6", "f7", "f8", "f9", "f10"];
const MAC_HOTKEYS = ["right-opt", "right-cmd", "right-ctrl", "fn"];

export function isMacPlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /mac/i.test(navigator.platform || navigator.userAgent);
}

/** Keys to offer, keeping any already-saved value visible so it is not silently replaced. */
export function hotkeyOptions(...current: Array<string | null | undefined>): string[] {
  const base = isMacPlatform() ? MAC_HOTKEYS : WINDOWS_HOTKEYS;
  const extra = current.filter((key): key is string => !!key && key !== "__none__" && !base.includes(key));
  return [...base, ...new Set(extra)];
}

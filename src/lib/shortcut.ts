import { useQuery } from "@tanstack/react-query";
import { api, queryKeys } from "./ipc";

/** Matches `VaultOptions`/`Settings` defaults on the Rust side. */
export const DEFAULT_ACCELERATOR = "CmdOrCtrl+Shift+Space";

const MODIFIER_SYMBOLS: Record<string, string> = {
  cmdorctrl: "⌘",
  commandorcontrol: "⌘",
  cmd: "⌘",
  command: "⌘",
  super: "⌘",
  ctrl: "⌃",
  control: "⌃",
  alt: "⌥",
  option: "⌥",
  shift: "⇧",
};

/** `CmdOrCtrl+Shift+Space` → `⌘ ⇧ Space` for display. */
export function formatAccelerator(accelerator: string): string {
  const parts = accelerator.split("+").map((part) => part.trim()).filter(Boolean);
  if (parts.length === 0) return "—";
  return parts
    .map((part) => MODIFIER_SYMBOLS[part.toLowerCase()] ?? part.replace(/^Key/, "").replace(/^Arrow/, "→ "))
    .join(" ");
}

/**
 * The configured global capture shortcut, formatted for hints and buttons.
 * Follows settings, so changing it in 设置 updates every label that mentions it.
 */
export function useShortcutLabel(): string {
  const settings = useQuery({
    queryKey: queryKeys.settings,
    queryFn: () => api.settings(),
    retry: 0,
  });
  return formatAccelerator(settings.data?.shortcut?.trim() || DEFAULT_ACCELERATOR);
}

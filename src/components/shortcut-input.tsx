import * as React from "react";
import { cn } from "../lib/format";
import { DEFAULT_ACCELERATOR, formatAccelerator } from "../lib/shortcut";

/**
 * Records a key chord and serialises it as a Tauri accelerator string
 * (`CmdOrCtrl+Shift+Space`), which is what `global-hotkey` parses on the Rust
 * side: modifiers first, exactly one key last.
 */

const KEY_NAMES: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  NumpadEnter: "NumpadEnter",
  Tab: "Tab",
  Backquote: "Backquote",
  Backslash: "Backslash",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight",
  Comma: "Comma",
  Equal: "Equal",
  Minus: "Minus",
  Period: "Period",
  Quote: "Quote",
  Semicolon: "Semicolon",
  Slash: "Slash",
};

const IGNORED_KEYS = new Set(["Meta", "Control", "Alt", "Shift", "AltGraph", "CapsLock"]);

export function primaryIsMeta(): boolean {
  const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
  return /Mac|iPhone|iPad/.test(ua);
}

export function keyToAccelerator(
  event: React.KeyboardEvent | KeyboardEvent,
  options: { primaryIsMeta?: boolean } = {},
): string | null {
  const code = event.code;
  if (!code || IGNORED_KEYS.has(code)) return null;

  // On macOS the "command" key is the natural primary modifier; elsewhere it is
  // control. `CmdOrCtrl` maps to the platform default in both cases.
  const meta = options.primaryIsMeta ?? primaryIsMeta();
  const mods: string[] = [];
  if (meta ? event.metaKey : event.ctrlKey) mods.push("CmdOrCtrl");
  if (meta && event.ctrlKey) mods.push("Ctrl");
  if (!meta && event.metaKey) mods.push("Super");
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");

  let key: string | null = null;
  if (code.startsWith("Key")) key = code.slice(3).toUpperCase();
  else if (code.startsWith("Digit")) key = code.slice(5);
  else if (code.startsWith("Numpad") && /^Numpad\d$/.test(code)) key = `Numpad${code.slice(-1)}`;
  else if (/^F\d{1,2}$/.test(code)) key = code;
  else if (code.startsWith("Arrow")) key = `Arrow${code.slice(5)}`;
  else if (KEY_NAMES[code]) key = KEY_NAMES[code];

  if (!key) return null;
  if (mods.length === 0) return null; // a bare letter would hijack the whole system
  return [...mods, key].join("+");
}

export function ShortcutInput({
  value,
  onChange,
  invalid,
}: {
  value: string;
  onChange: (next: string) => void;
  invalid?: boolean;
}) {
  const [recording, setRecording] = React.useState(false);
  const [warning, setWarning] = React.useState<string | null>(null);

  const onKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      setRecording(false);
      setWarning(null);
      return;
    }
    if (event.key === "Backspace") {
      setRecording(false);
      setWarning(null);
      return;
    }
    const next = keyToAccelerator(event);
    if (!next) {
      setWarning("请同时按下修饰键（⌘ ⌃ ⌥ ⇧）和一个普通键");
      return;
    }
    setWarning(null);
    setRecording(false);
    onChange(next);
  };

  return (
    <div className="space-y-1">
      <div className="flex gap-2">
        <button
          type="button"
          onClick={() => {
            setRecording(true);
            setWarning(null);
          }}
          onKeyDown={onKeyDown}
          onBlur={() => setRecording(false)}
          aria-label="录制全局快捷键"
          className={cn(
            "flex-1 rounded-lg border px-3 py-1.5 text-left text-sm transition",
            invalid
              ? "border-[var(--color-danger)]"
              : "border-[var(--color-line)] hover:border-[var(--color-accent)]",
            recording
              ? "border-[var(--color-accent)] bg-[var(--color-surface-2)] text-[var(--color-accent)]"
              : "bg-[var(--color-surface)]",
          )}
        >
          {recording ? "请按下新的快捷键…（Esc 取消）" : `当前：${formatAccelerator(value)}`}
        </button>
        <button
          type="button"
          onClick={() => {
            onChange(DEFAULT_ACCELERATOR);
            setWarning(null);
            setRecording(false);
          }}
          className="rounded-lg border border-[var(--color-line)] px-3 py-1.5 text-xs text-[var(--color-muted)] transition hover:text-[var(--color-ink)]"
        >
          重置
        </button>
      </div>
      <div className="font-mono text-[11px] text-[var(--color-muted)]">{value || "—"}</div>
      {warning ? <div className="text-[11px] text-[var(--color-danger)]">{warning}</div> : null}
    </div>
  );
}

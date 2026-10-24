import * as React from "react";
import { cn } from "../lib/format";
import { DEFAULT_ACCELERATOR, formatAccelerator } from "../lib/shortcut";

/**
 * Records a key chord and serialises it as a Tauri accelerator string
 * (`CmdOrCtrl+Shift+Space`), which is what `global-hotkey` parses on the Rust
 * side: modifiers first, exactly one key last.
 *
 * The listener lives on `window` in the capture phase for two reasons: WebKit
 * does not focus buttons on click, and the recorder must keep working while the
 * global shortcut is released (see `shortcut_recording`).
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

const MODIFIER_NAMES = new Set(["Meta", "Control", "Alt", "Shift", "AltGraph", "CapsLock"]);

/** A key that only modifies the chord: pressing it alone is not a chord. */
function isModifier(event: Pick<KeyboardEvent, "key" | "code">): boolean {
  return (
    MODIFIER_NAMES.has(event.key) ||
    /^(Meta|Control|Alt|Shift|OS)(Left|Right)$/.test(event.code)
  );
}

/** Chords the macOS app menu already owns; they never reach the webview. */
const MENU_KEYS = new Set(["A", "C", "V", "X", "Z", "Q", "W", "M", "H", "N", "O", "S", "P", "T"]);

export function primaryIsMeta(): boolean {
  const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
  return /Mac|iPhone|iPad/.test(ua);
}

type ChordEvent = Pick<
  KeyboardEvent,
  "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"
>;

function modifiersOf(event: ChordEvent, meta: boolean): string[] {
  const mods: string[] = [];
  if (meta ? event.metaKey : event.ctrlKey) mods.push("CmdOrCtrl");
  if (meta && event.ctrlKey) mods.push("Ctrl");
  if (!meta && event.metaKey) mods.push("Super");
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");
  return mods;
}

/** The chord's meaning: `⌘`/`⌃` (primary) or `⌥` must be involved. */
export function hasUsableModifier(mods: string[]): boolean {
  return mods.some((mod) => mod === "CmdOrCtrl" || mod === "Ctrl" || mod === "Alt");
}

export function keyToAccelerator(
  event: KeyboardEvent | React.KeyboardEvent,
  options: { primaryIsMeta?: boolean } = {},
): string | null {
  const code = event.code;
  if (!code || isModifier(event)) return null;

  // On macOS the "command" key is the natural primary modifier; elsewhere it is
  // control. `CmdOrCtrl` maps to the platform default in both cases.
  const meta = options.primaryIsMeta ?? primaryIsMeta();
  const mods = modifiersOf(event, meta);

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

/** Human readable reason why a chord cannot be recorded, or `null` when it can. */
export function chordProblem(
  event: KeyboardEvent,
  meta: boolean,
): string | null {
  if (isModifier(event)) return null; // still typing modifiers
  const mods = modifiersOf(event, meta);
  if (mods.length === 0) {
    return "请按住修饰键（⌘ ⌃ ⌥ ⇧）再按一个普通键";
  }
  if (!hasUsableModifier(mods)) {
    return "只有 ⇧ 不够：请加上 ⌘、⌃ 或 ⌥";
  }
  const accelerator = keyToAccelerator(event, { primaryIsMeta: meta });
  if (!accelerator) {
    return "这个键不能用作全局快捷键，请换一个";
  }
  // `accelerator` is `MOD+MOD+KEY`; only the trailing key matters here.
  const key = accelerator.slice(accelerator.lastIndexOf("+") + 1);
  const bare = mods.length === 1 && mods[0] === (meta ? "CmdOrCtrl" : "Ctrl");
  if (meta && bare && MENU_KEYS.has(key)) {
    return `⌘${key} 被 macOS 菜单占用，请再加 ⇧ 或 ⌥`;
  }
  return null;
}

function previewOf(event: KeyboardEvent, meta: boolean): string {
  const mods = modifiersOf(event, meta);
  return mods.map((mod) => formatAccelerator(mod)).join(" ");
}

export function ShortcutInput({
  value,
  onChange,
  invalid,
  onRecordingChange,
}: {
  value: string;
  onChange: (next: string) => void;
  invalid?: boolean;
  /** Lets the caller release/restore the live global hotkey while recording. */
  onRecordingChange?: (recording: boolean) => void;
}) {
  const [recording, setRecording] = React.useState(false);
  const [preview, setPreview] = React.useState("");
  const [warning, setWarning] = React.useState<string | null>(null);
  const rootRef = React.useRef<HTMLDivElement | null>(null);
  const recordingRef = React.useRef(false);
  const changeRef = React.useRef(onChange);
  changeRef.current = onChange;
  const recordingChangeRef = React.useRef(onRecordingChange);
  recordingChangeRef.current = onRecordingChange;

  const stop = React.useCallback((commit?: string) => {
    if (!recordingRef.current) return;
    recordingRef.current = false;
    setRecording(false);
    setPreview("");
    // Restore the global hotkey that `begin` released.
    recordingChangeRef.current?.(false);
    if (commit !== undefined) changeRef.current(commit);
  }, []);

  const start = React.useCallback(() => {
    recordingRef.current = true;
    setRecording(true);
    setPreview("");
    setWarning(null);
  }, []);

  // A released global hotkey must be restored no matter how the component goes
  // away (route change, unmount, failed save).
  React.useEffect(() => {
    return () => {
      if (recordingRef.current) recordingChangeRef.current?.(false);
    };
  }, []);

  React.useEffect(() => {
    if (!recording) return;
    const meta = primaryIsMeta();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.repeat) return;
      // Keep the chord away from the app level shortcuts (⌘K, ⌘N, …).
      event.preventDefault();
      event.stopPropagation();

      if (event.key === "Escape" || event.key === "Backspace") {
        setWarning(null);
        stop();
        return;
      }

      setPreview(previewOf(event, meta));

      const problem = chordProblem(event, meta);
      if (problem) {
        // A lone modifier is not a problem, it is simply not the chord yet.
        if (!isModifier(event)) setWarning(problem);
        return;
      }
      const next = keyToAccelerator(event, { primaryIsMeta: meta });
      if (!next) return;
      setWarning(null);
      stop(next);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      if (isModifier(event)) setPreview(previewOf(event, meta));
    };

    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) stop();
    };
    const onWindowBlur = () => stop();

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", onWindowBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", onWindowBlur);
    };
  }, [recording, stop]);

  const begin = () => {
    start();
    recordingChangeRef.current?.(true);
  };

  return (
    <div className="space-y-1" ref={rootRef}>
      <div className="flex gap-2">
        <button
          type="button"
          onClick={() => (recording ? stop() : begin())}
          aria-label="录制全局快捷键"
          aria-pressed={recording}
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
          {recording
            ? preview
              ? `新快捷键：${preview} + …`
              : "请按下新的快捷键…（Esc 取消）"
            : `当前：${formatAccelerator(value)}`}
        </button>
        <button
          type="button"
          onClick={() => {
            stop();
            setWarning(null);
            onChange(DEFAULT_ACCELERATOR);
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

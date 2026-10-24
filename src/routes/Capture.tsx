import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import type { CaptureParseDto } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn } from "../lib/format";
import { hideCaptureWindow, focusMain } from "../lib/window";

const DEBOUNCE_MS = 60;

/**
 * The capture palette: one flat line in, one canonical hledger block out.
 *
 * The Rust side owns the grammar (`capture_parse`), so this component only
 * renders the preview and decides what Enter means for each kind.
 */
export function CaptureWindow() {
  const [text, setText] = React.useState("");
  const [preview, setPreview] = React.useState<CaptureParseDto | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [status, setStatus] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const completion = React.useRef({ index: 0 });
  const inputRef = React.useRef<HTMLInputElement>(null);

  const snapshot = useQuery({
    queryKey: queryKeys.snapshot,
    queryFn: () => api.snapshot(),
    retry: 0,
  });

  const accounts = React.useMemo(
    () => (snapshot.data?.accounts ?? []).map((a) => a.full).filter(Boolean),
    [snapshot.data],
  );

  React.useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Live preview, debounced so typing stays smooth.
  React.useEffect(() => {
    const value = text.trim();
    if (!value) {
      setPreview(null);
      setError(null);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api
        .captureParse(value)
        .then((next) => {
          if (cancelled) return;
          setPreview(next);
          setError(next.errors.length > 0 ? next.errors[0].message : null);
        })
        .catch((err: unknown) => {
          if (cancelled) return;
          setPreview(null);
          setError(err instanceof Error ? err.message : String(err));
        });
    }, DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [text]);

  const reset = React.useCallback(() => {
    setText("");
    setPreview(null);
    setError(null);
    completion.current.index = 0;
    inputRef.current?.focus();
  }, []);

  const commit = React.useCallback(
    async (keepOpen: boolean) => {
      const value = text.trim();
      if (!value || busy) return;
      setBusy(true);
      setError(null);
      try {
        if (preview?.kind === "search") {
          await focusMain(value.replace(/^\//, "").trim());
          reset();
          return;
        }
        if (preview?.kind === "rule") {
          const rule = await api.ruleUpsert(value);
          setStatus(`规则 ${rule.id} 已保存`);
        } else if (preview?.kind === "command") {
          setStatus("命令面板：输入 / 搜索，或直接记一笔");
        } else {
          const saved = await api.captureCommit(value);
          setStatus(`已记入 ${saved.file}`);
        }
        reset();
        if (!keepOpen) await hideCaptureWindow();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [busy, preview?.kind, reset, text],
  );

  const complete = React.useCallback(() => {
    if (accounts.length === 0) return;
    const parts = text.split(/\s+/);
    const last = parts[parts.length - 1] ?? "";
    const pool = accounts.filter((a) =>
      last.startsWith("@") ? a.includes(last.slice(1).toLowerCase()) : true,
    );
    if (pool.length === 0) return;
    const pick = pool[completion.current.index % pool.length];
    completion.current.index += 1;
    parts[parts.length - 1] = `@${pick}`;
    setText(`${parts.join(" ")} `);
  }, [accounts, text]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void commit(!event.metaKey && !event.ctrlKey);
    } else if (event.key === "Escape") {
      event.preventDefault();
      if (text) reset();
      else void hideCaptureWindow();
    } else if (event.key === "Tab") {
      event.preventDefault();
      complete();
    }
  };

  const warnings = preview?.warnings ?? [];
  const postings = preview?.postings ?? [];

  return (
    <div className="flex h-full flex-col justify-center p-3">
      <div
        className={cn(
          "rounded-[var(--radius-card)] border bg-[var(--color-surface)]/95 p-3 shadow-xl backdrop-blur",
          error
            ? "border-[var(--color-danger)]"
            : "border-[var(--color-line)]",
        )}
      >
        <div className="flex items-center gap-2">
          <span className="text-sm text-[var(--color-accent)]">›</span>
          <input
            ref={inputRef}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setStatus(null);
              completion.current.index = 0;
            }}
            onKeyDown={onKeyDown}
            placeholder="咖啡 4.5 @微信 +餐饮   ·   = 规则   ·   / 搜索"
            className="w-full bg-transparent text-[15px] outline-none placeholder:text-[var(--color-muted)]"
            aria-label="快录输入"
            spellCheck={false}
          />
          {busy ? <span className="text-xs text-[var(--color-muted)]">保存中…</span> : null}
        </div>

        <div className="mt-2 min-h-[46px] border-t border-[var(--color-line)] pt-2 text-xs">
          {error ? (
            <div className="text-[var(--color-danger)]">{error}</div>
          ) : postings.length > 0 ? (
            <div className="space-y-0.5">
              {postings.map((posting, index) => (
                <div key={index} className="flex justify-between gap-3 font-mono">
                  <span className="truncate text-[var(--color-muted)]">{posting.account}</span>
                  <span>{posting.display}</span>
                </div>
              ))}
            </div>
          ) : status ? (
            <div className="text-[var(--color-accent)]">{status}</div>
          ) : (
            <div className="text-[var(--color-muted)]">
              Enter 记一笔并继续 · ⌘Enter 记完隐藏 · Tab 补全账户 · Esc 关闭
            </div>
          )}
          {warnings.length > 0 && !error ? (
            <div className="mt-1 text-amber-600 dark:text-amber-400">{warnings[0]}</div>
          ) : null}
          {preview?.rule_id ? (
            <div className="mt-1 text-[var(--color-muted)]">规则 {preview.rule_id}</div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

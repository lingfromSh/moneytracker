import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import type { CaptureParseDto, LlmDraftDto } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn } from "../lib/format";
import { hideCaptureWindow, focusMain } from "../lib/window";

const DEBOUNCE_MS = 60;
/// The model needs a round trip, so typing waits a little longer for it.
const LLM_DEBOUNCE_MS = 600;
const MODE_KEY = "moneytracker.captureMode";

type Mode = "ledger" | "llm";

const Code = ({ children }: { children: React.ReactNode }) => (
  <code className="rounded bg-[var(--color-surface-2)] px-1 font-mono text-[10.5px] text-[var(--color-ink)]">
    {children}
  </code>
);

const PLACEHOLDER: Record<Mode, string> = {
  ledger: "咖啡 4.5 @assets:cash +餐饮 #昨天",
  llm: "昨天和同事在星巴克花了 45，支付宝付的",
};

/** Cheat sheet shown while the field is empty, so the grammar is discoverable. */
function SyntaxHint() {
  return (
    <div className="space-y-1 text-[11px] leading-4 text-[var(--color-muted)]">
      <div className="flex gap-2">
        <span className="w-7 shrink-0 opacity-60">账户</span>
        <span className="min-w-0">
          <Code>assets:</Code> <Code>liabilities:</Code> <Code>income:</Code>{" "}
          <Code>expenses:</Code> <Code>equity:</Code> — 写全名如 <Code>@assets:cash</Code>
          ，也可只写 <Code>@现金</Code> 由程序补全
        </span>
      </div>
      <div className="flex gap-2">
        <span className="w-7 shrink-0 opacity-60">日期</span>
        <span className="min-w-0">
          <Code>#今天</Code> <Code>#昨天</Code> <Code>#前天</Code> <Code>#-1d</Code>{" "}
          <Code>#+3d</Code> <Code>#2026-10-05</Code> — 不写就是今天
        </span>
      </div>
      <div className="flex gap-2">
        <span className="w-7 shrink-0 opacity-60">分录</span>
        <span className="min-w-0">
          <Code>火锅 168 @微信 100 @现金 68</Code> 一笔支出多个来源 ·{" "}
          <Code>@餐饮 70%</Code> 按比例 · 不写金额的账户由程序补平
        </span>
      </div>
      <div className="flex gap-2">
        <span className="w-7 shrink-0 opacity-60">其他</span>
        <span className="min-w-0">
          <Code>+餐饮</Code> 加标签 · <Code>&gt; @a &gt; @b</Code> 转账 · <Code>=</Code>{" "}
          新规则 · <Code>/</Code> 搜索
        </span>
      </div>
      <div className="opacity-70">
        Enter 记一笔并继续 · ⌘Enter 记完隐藏 · ⌥Tab 补全账户 · Tab 切换自然语言 · Esc 关闭
      </div>
    </div>
  );
}

/** Empty-state help for natural-language mode. */
function LlmHint({
  ready,
  reason,
  enabled,
}: {
  ready: boolean;
  reason: string | null;
  enabled: boolean;
}) {
  if (!enabled) {
    return (
      <div className="space-y-1 text-[11px] leading-4 text-[var(--color-muted)]">
        <div className="text-[var(--color-ink)]">自然语言模式还没启用。</div>
        <div>
          到 <span className="font-medium">设置 → AI 记一笔</span>{" "}
          打开开关、选择服务商并填写 API Key，就能用一句话记账。
        </div>
        <div className="opacity-70">Tab 切回 Hledger 语法 · Esc 关闭</div>
      </div>
    );
  }
  if (!ready) {
    return (
      <div className="space-y-1 text-[11px] leading-4 text-[var(--color-muted)]">
        <div className="text-amber-600 dark:text-amber-400">{reason ?? "配置还没填完。"}</div>
        <div>到 设置 → AI 记一笔 补齐；也可以直接按 Tab 切回 Hledger 语法自己写。</div>
      </div>
    );
  }
  return (
    <div className="space-y-1 text-[11px] leading-4 text-[var(--color-muted)]">
      <div>
        用一句话描述这笔账，例如
        <span className="text-[var(--color-ink)]">
          「昨天和同事在星巴克花了 45，支付宝付的」
        </span>
        、「这个月工资 12000 到招行」、或者「从现金转 500 到银行卡」。
      </div>
      <div>
        模型只会挑出日期、金额和账户，最终仍由本地记账引擎校验并写成 Hledger
        文本；回车前你可以看到它打算写什么。
      </div>
      <div className="opacity-70">
        Enter 记一笔 · ⌘Enter 记完隐藏 · Tab 切回 Hledger 语法 · Esc 关闭
      </div>
    </div>
  );
}

/**
 * The capture palette: one flat line in, one canonical hledger block out.
 *
 * The Rust side owns both grammars (`capture_parse` for hledger syntax,
 * `llm_draft` for natural language), so this component only renders the preview,
 * switches modes and decides what Enter means for each kind.
 */
export function CaptureWindow() {
  const [text, setText] = React.useState("");
  const [mode, setMode] = React.useState<Mode>(
    () => (localStorage.getItem(MODE_KEY) === "llm" ? "llm" : "ledger"),
  );
  const [preview, setPreview] = React.useState<CaptureParseDto | null>(null);
  const [draft, setDraft] = React.useState<LlmDraftDto | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [status, setStatus] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [thinking, setThinking] = React.useState(false);
  const completion = React.useRef({ index: 0 });
  const inputRef = React.useRef<HTMLInputElement>(null);

  const snapshot = useQuery({
    queryKey: queryKeys.snapshot,
    queryFn: () => api.snapshot(),
    retry: 0,
  });

  const llmStatus = useQuery({
    queryKey: queryKeys.llmStatus,
    queryFn: () => api.llmStatus(),
    retry: 0,
  });

  const accounts = React.useMemo(
    () => (snapshot.data?.accounts ?? []).map((a) => a.full).filter(Boolean),
    [snapshot.data],
  );

  React.useEffect(() => {
    inputRef.current?.focus();
  }, []);

  React.useEffect(() => {
    localStorage.setItem(MODE_KEY, mode);
  }, [mode]);

  // Live preview. In hledger mode this is the parser; in natural-language mode
  // the model drafts and the parser previews the line it produced.
  React.useEffect(() => {
    const value = text.trim();
    setDraft(null);
    if (!value) {
      setPreview(null);
      setError(null);
      setThinking(false);
      return;
    }
    let cancelled = false;
    setThinking(mode === "llm");
    const timer = window.setTimeout(
      () => {
        const request =
          mode === "llm"
            ? api.llmDraft(value).then((next) => {
                if (!cancelled) setDraft(next);
                return next.preview;
              })
            : api.captureParse(value);
        request
          .then((next) => {
            if (cancelled) return;
            setPreview(next ?? null);
            setError(next && next.errors.length > 0 ? next.errors[0].message : null);
          })
          .catch((err: unknown) => {
            if (cancelled) return;
            setPreview(null);
            setError(err instanceof Error ? err.message : String(err));
          })
          .finally(() => {
            if (!cancelled) setThinking(false);
          });
      },
      mode === "llm" ? LLM_DEBOUNCE_MS : DEBOUNCE_MS,
    );
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [text, mode]);

  const reset = React.useCallback(() => {
    setText("");
    setPreview(null);
    setDraft(null);
    setError(null);
    completion.current.index = 0;
    inputRef.current?.focus();
  }, []);

  /** Natural-language mode commits the line the model drafted, never the prose. */
  const pendingLine = mode === "llm" ? (draft?.line ?? "") : text.trim();

  const commit = React.useCallback(
    async (keepOpen: boolean) => {
      const value = pendingLine.trim();
      if (!value || busy) return;
      setBusy(true);
      setError(null);
      try {
        if (mode === "ledger" && preview?.kind === "search") {
          await focusMain(value.replace(/^\//, "").trim());
          reset();
          return;
        }
        if (mode === "ledger" && preview?.kind === "rule") {
          const rule = await api.ruleUpsert(value);
          setStatus(`规则 ${rule.id} 已保存`);
        } else if (mode === "ledger" && preview?.kind === "command") {
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
    [busy, mode, pendingLine, preview?.kind, reset],
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
      if (event.altKey) {
        complete();
        return;
      }
      // Tab flips between the two input grammars.
      setStatus(null);
      setMode((current) => (current === "ledger" ? "llm" : "ledger"));
    }
  };

  const warnings = preview?.warnings ?? [];
  const postings = preview?.postings ?? [];
  const llm = llmStatus.data;

  return (
    <div className="flex h-full flex-col p-3">
      <div
        className={cn(
          "flex h-full min-h-0 flex-col rounded-[var(--radius-card)] border bg-[var(--color-surface)]/95 p-3 shadow-xl backdrop-blur",
          error ? "border-[var(--color-danger)]" : "border-[var(--color-line)]",
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
            placeholder={PLACEHOLDER[mode]}
            className="w-full bg-transparent text-[15px] outline-none placeholder:text-[var(--color-muted)]"
            aria-label="快录输入"
            spellCheck={false}
          />
          <button
            type="button"
            onClick={() => setMode((current) => (current === "ledger" ? "llm" : "ledger"))}
            className={cn(
              "shrink-0 rounded-full border px-2 py-0.5 text-[10px] tracking-wide",
              mode === "llm"
                ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                : "border-[var(--color-line)] text-[var(--color-muted)]",
            )}
            title="Tab 切换 Hledger 语法 / 自然语言"
            aria-label="输入模式"
            aria-pressed={mode === "llm"}
          >
            {mode === "llm" ? "自然语言" : "Hledger"}
          </button>
          {busy || thinking ? (
            <span className="shrink-0 text-xs text-[var(--color-muted)]">
              {busy ? "保存中…" : "解析中…"}
            </span>
          ) : null}
        </div>

        <div className="mt-2 min-h-0 flex-1 overflow-y-auto border-t border-[var(--color-line)] pt-2 text-xs">
          {error ? (
            <div className="text-[var(--color-danger)]">{error}</div>
          ) : mode === "llm" && draft ? (
            <div className="space-y-1">
              <div className="flex items-baseline gap-2 font-mono text-[11px]">
                <span className="shrink-0 text-[var(--color-muted)]">将写入</span>
                <span className="min-w-0 truncate text-[var(--color-ink)]">{draft.line}</span>
              </div>
              <div className="space-y-0.5">
                {postings.map((posting, index) => (
                  <div key={index} className="flex justify-between gap-3 font-mono">
                    <span className="truncate text-[var(--color-muted)]">{posting.account}</span>
                    <span>{posting.display}</span>
                  </div>
                ))}
              </div>
              {draft.notes ? (
                <div className="text-[11px] text-[var(--color-muted)]">{draft.notes}</div>
              ) : null}
            </div>
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
          ) : mode === "llm" ? (
            <LlmHint
              ready={llm?.ready ?? false}
              reason={llm?.reason ?? null}
              enabled={llm?.enabled ?? false}
            />
          ) : (
            <SyntaxHint />
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

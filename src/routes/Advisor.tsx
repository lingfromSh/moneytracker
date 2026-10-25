import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { Sparkles, Trash2 } from "lucide-react";
import type { AdvisorToolDto, AdvisorTurnDto } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { ADVISOR_EXAMPLES, takeAdvisorSeed } from "../lib/advisor";
import { Badge, Button, Card, EmptyState, Textarea } from "../components/ui";

/**
 * 顾问：只读的智能体对话。
 *
 * 和「问一句」的差别在于上下文 —— 后端会把净资产、现金流、FIRE 目标、债务、预算、
 * 日程压成一段「账本全景」交给模型，模型还可以再调工具要数字。前端只负责展示，
 * 所有数字都是引擎算出来的。
 */
export function Advisor() {
  const history = useQuery({
    queryKey: queryKeys.advisor,
    queryFn: () => api.advisorLoad(),
    retry: 0,
  });
  const [turns, setTurns] = React.useState<AdvisorTurnDto[]>([]);
  const [draft, setDraft] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [tools, setTools] = React.useState<AdvisorToolDto[]>([]);
  const [seeded, setSeeded] = React.useState(false);

  // 从看板「接着聊」过来时，把刚才那轮问答带进来。
  React.useEffect(() => {
    const seed = takeAdvisorSeed();
    if (seed.length) {
      setTurns(seed);
      setSeeded(true);
    }
  }, []);

  // 否则用存下来的会话（只在第一次挂载时读一次，避免覆盖正在进行的对话）。
  React.useEffect(() => {
    if (seeded || !history.data) return;
    setTurns((current) => (current.length ? current : history.data.turns));
  }, [history.data, seeded]);

  const send = React.useCallback(
    async (text: string) => {
      const question = text.trim();
      if (!question || busy) return;
      const before = turns;
      const next: AdvisorTurnDto[] = [
        ...before,
        { role: "user", content: question, at: new Date().toISOString() },
      ];
      setTurns(next);
      setDraft("");
      setBusy(true);
      setError(null);
      setTools([]);
      try {
        const reply = await api.advisorAsk(question, before);
        setTurns([
          ...next,
          { role: "assistant", content: reply.content, at: new Date().toISOString() },
        ]);
        setTools(reply.tools ?? []);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        setBusy(false);
      }
    },
    [busy, turns],
  );

  const clear = async () => {
    try {
      await api.advisorClear();
      setTurns([]);
      setTools([]);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };

  const empty = turns.length === 0;

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex flex-col gap-2 px-4 pb-3 pt-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Sparkles className="h-4 w-4 text-[var(--color-accent)]" />
            <span className="text-lg font-semibold">顾问</span>
            <Badge tone="neutral">只读</Badge>
          </div>
          <div className="mt-0.5 text-xs text-[var(--color-muted)]">
            带着你的净值、现金流和 FIRE 目标聊，数字都由本地账本算出
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="subtle" onClick={() => void clear()} disabled={busy || empty}>
            <Trash2 className="mr-1 h-3.5 w-3.5" />
            清空对话
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto overflow-x-hidden px-4 pb-4">
        {empty ? (
          <Card className="p-4">
            <EmptyState
              title="问一个需要判断的问题"
              hint="顾问会先看你的现金、储备月数、本月现金流和 FIRE 进度，再把数字讲清楚。它不会改账本。"
              action={
                <div className="flex flex-wrap justify-center gap-2">
                  {ADVISOR_EXAMPLES.map((example) => (
                    <button
                      key={example}
                      className="rounded-full border border-[var(--color-line)] px-3 py-1 text-[11px] hover:bg-[var(--color-surface-2)]"
                      onClick={() => void send(example)}
                    >
                      {example}
                    </button>
                  ))}
                </div>
              }
            />
          </Card>
        ) : null}

        {turns.map((turn, index) => (
          <Bubble key={`${turn.role}-${index}`} turn={turn} />
        ))}

        {busy ? (
          <div className="text-xs text-[var(--color-muted)]">顾问正在查账本…</div>
        ) : null}

        {error ? (
          <div className="rounded-lg border border-[var(--color-danger)] px-3 py-2 text-[11px] text-[var(--color-danger)]">
            {error}
          </div>
        ) : null}

        {tools.length ? (
          <details className="rounded-lg border border-[var(--color-line)] px-3 py-2 text-[11px]">
            <summary className="cursor-pointer text-[var(--color-muted)]">
              这一轮查了 {tools.length} 次账本
            </summary>
            <div className="mt-2 space-y-2">
              {tools.map((tool, index) => (
                <div key={`${tool.tool}-${index}`} className="space-y-1">
                  <div className="font-mono text-[var(--color-fg)]">
                    {tool.tool} {tool.input}
                  </div>
                  <pre className="overflow-x-auto whitespace-pre-wrap break-words font-mono text-[var(--color-muted)]">
                    {tool.output}
                  </pre>
                </div>
              ))}
            </div>
          </details>
        ) : null}
      </div>

      <div className="border-t border-[var(--color-line)] px-4 py-3">
        <div className="flex items-end gap-2">
          <Textarea
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="问问账本：这个月能不能多花 3000？"
            className="min-h-10 flex-1"
            rows={1}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                void send(draft);
              }
            }}
          />
          <Button variant="primary" onClick={() => void send(draft)} disabled={busy || !draft.trim()}>
            发送
          </Button>
        </div>
        <div className="mt-1 text-[10px] text-[var(--color-muted)]">
          Enter 发送 · Shift+Enter 换行 · 会话只存在本机
        </div>
      </div>
    </div>
  );
}

function Bubble({ turn }: { turn: AdvisorTurnDto }) {
  const mine = turn.role !== "assistant";
  return (
    <div className={mine ? "flex justify-end" : "flex justify-start"}>
      <div
        className={
          mine
            ? "max-w-[85%] whitespace-pre-wrap rounded-2xl rounded-br-md bg-[var(--color-accent)]/12 px-3 py-2 text-sm"
            : "max-w-[92%] whitespace-pre-wrap rounded-2xl rounded-bl-md border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2 text-sm"
        }
      >
        {turn.content}
      </div>
    </div>
  );
}

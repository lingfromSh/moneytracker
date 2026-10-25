import * as React from "react";
import { Sparkles } from "lucide-react";
import type { AskResultDto_Serialize } from "../lib/bindings";
import { api } from "../lib/ipc";
import { Badge, Button, Card, Input } from "./ui";

const EXAMPLES = [
  "今年餐饮花了多少",
  "哪个月支出最高",
  "按账户看这个月的支出",
  "最近 12 个月收入趋势",
];

/**
 * Read-only question answering. The model only picks query fields; the numbers
 * come from the engine, so an answer cannot contain a figure the ledger does
 * not have.
 */
export function AskCard() {
  const [question, setQuestion] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [result, setResult] = React.useState<AskResultDto_Serialize | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  const ask = async (text: string) => {
    const trimmed = text.trim();
    if (trimmed.length === 0) return;
    setQuestion(trimmed);
    setBusy(true);
    setError(null);
    try {
      setResult(await api.llmAsk(trimmed));
    } catch (cause) {
      setResult(null);
      setError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="p-4">
      <div className="mb-2 flex items-center gap-2">
        <Sparkles className="h-4 w-4 text-[var(--color-accent)]" />
        <span className="text-sm font-medium">问一句</span>
        <span className="text-[11px] text-[var(--color-muted)]">
          只读：模型把问题翻译成查询，数字由本地账本算出
        </span>
      </div>
      <form
        className="flex items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          void ask(question);
        }}
      >
        <Input
          value={question}
          onChange={(event) => setQuestion(event.target.value)}
          placeholder="比如：今年餐饮花了多少？"
          aria-label="问题"
        />
        <Button type="submit" variant="primary" disabled={busy}>
          {busy ? "思考中…" : "提问"}
        </Button>
      </form>
      <div className="mt-1.5 flex flex-wrap gap-1.5">
        {EXAMPLES.map((example) => (
          <button
            key={example}
            type="button"
            className="rounded-full border border-[var(--color-line)] px-2 py-0.5 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-ink)]"
            onClick={() => void ask(example)}
            disabled={busy}
          >
            {example}
          </button>
        ))}
      </div>

      {error ? (
        <div className="mt-3 rounded-lg border border-[var(--color-danger)]/40 px-3 py-2 text-xs text-[var(--color-danger)]">
          {error}
        </div>
      ) : null}

      {result ? (
        <div className="mt-3 space-y-2">
          <div className="text-xs leading-relaxed">{result.answer ?? result.result.summary}</div>
          <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--color-muted)]">
            <Badge>{result.result.metric}</Badge>
            {result.plan.group_by ? <Badge>按{label(result.plan.group_by)}</Badge> : null}
            {result.plan.date_from || result.plan.date_to ? (
              <Badge>{`${result.plan.date_from ?? "…"} → ${result.plan.date_to ?? "…"}`}</Badge>
            ) : null}
            {result.plan.accounts.map((account) => (
              <Badge key={account}>{account}</Badge>
            ))}
            <span>共 {result.result.matched} 笔，合计 {result.result.total.display}</span>
          </div>
          {result.result.facts.length > 0 ? (
            <table className="w-full text-xs">
              <thead>
                <tr className="text-left text-[var(--color-muted)]">
                  <th className="py-1 font-normal">分组</th>
                  <th className="py-1 text-right font-normal">金额</th>
                </tr>
              </thead>
              <tbody>
                {result.result.facts.map((fact) => (
                  <tr key={fact.label} className="border-t border-[var(--color-line)]/60">
                    <td className="py-1">{fact.label}</td>
                    <td className="py-1 text-right font-mono tabular-nums">{fact.display}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : null}
        </div>
      ) : null}
    </Card>
  );
}

function label(groupBy: string): string {
  if (groupBy === "account") return "账户";
  if (groupBy === "tag") return "标签";
  return "月份";
}

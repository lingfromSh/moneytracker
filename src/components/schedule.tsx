import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, CalendarClock } from "lucide-react";
import type {
  AmountDto,
  ScheduleOccurrenceDto_Serialize,
  ScheduleTemplateDto_Serialize,
} from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { dayLabel, money, monthLabel } from "../lib/format";
import { Badge, Button, EmptyState } from "./ui";

/** The cash leg of a template / occurrence: what actually leaves the account. */
export function cashOf(entry: {
  settle: AmountDto;
  income: AmountDto;
  expense: AmountDto;
}): string {
  const sign = (entry.income.value ?? 0) > 0 ? "+" : "";
  return `${sign}${money(entry.settle.value, entry.settle.display)}`;
}

export function kindTone(kind: string): "neutral" | "accent" | "warn" | "danger" {
  switch (kind) {
    case "salary":
    case "income":
      return "accent";
    case "loan":
      return "danger";
    case "expense":
      return "warn";
    default:
      return "neutral";
  }
}

export function kindText(kind: string): string {
  switch (kind) {
    case "salary":
      return "工资";
    case "income":
      return "收入";
    case "loan":
      return "还款";
    case "expense":
      return "支出";
    default:
      return "其他";
  }
}

/** Status chip for one occurrence inside a month. */
export function OccurrenceStatus({ occurrence }: { occurrence: ScheduleOccurrenceDto_Serialize }) {
  if (occurrence.recorded) return <Badge tone="accent">已记</Badge>;
  if (occurrence.skipped) return <Badge>已跳过</Badge>;
  if (occurrence.due) return <Badge tone="warn">待记</Badge>;
  return <Badge>待到期</Badge>;
}

export function cadenceText(template: ScheduleTemplateDto_Serialize): string {
  const parts: string[] = [];
  switch (template.cadence) {
    case "monthly":
      parts.push("每月");
      break;
    case "quarterly":
      parts.push("每季");
      break;
    case "yearly":
      parts.push("每年");
      break;
    case "weekly":
      parts.push("每周");
      break;
    case "daily":
      parts.push("每天");
      break;
    default:
      parts.push(template.period || "每月");
  }
  if (template.cadence === "monthly" || template.cadence === "quarterly" || template.cadence === "yearly") {
    parts.push(`${template.day} 号`);
  }
  return parts.join(" · ");
}

/**
 * The dashboard card: how much fixed money comes in and goes out each month, and
 * what is waiting to be booked. Nothing is written unless the user clicks.
 */
export function ScheduleCard({ onOpen }: { onOpen: () => void }) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = React.useState(false);
  const [message, setMessage] = React.useState<string | null>(null);
  const listed = useQuery({
    queryKey: queryKeys.schedule(12),
    queryFn: () => api.scheduleOverview(12),
    retry: 0,
  });

  const commit = async (id: string) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await api.scheduleCommit(id);
      setMessage(Number(result.written ?? 0) > 0 ? "已记入账本" : "这个月已经记过了");
      await queryClient.invalidateQueries();
    } catch (cause) {
      setMessage((cause as Error).message);
    } finally {
      setBusy(false);
    }
  };

  if (listed.isLoading) {
    return <div className="p-3 text-xs text-[var(--color-muted)]">正在读取 recurring.ledger…</div>;
  }

  const data = listed.data;
  if (!data || !data.has_schedule) {
    return (
      <EmptyState
        title="还没有固定收支"
        hint="在 recurring.ledger 里写一条 `~ monthly …`，工资、房贷、订阅就会出现在这里和日程页。"
        action={
          <Button variant="subtle" onClick={onOpen}>
            打开日程
            <ArrowRight className="ml-1 h-3.5 w-3.5" />
          </Button>
        }
      />
    );
  }

  const totals = data.month_totals[0];
  const due = data.occurrences.filter((occurrence) => occurrence.due);
  const net = (totals?.income.value ?? 0) - (totals?.expense.value ?? 0);

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 overflow-auto p-3 text-xs">
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <Cell label={totals ? monthLabel(totals.month) : "本月"} tone="income">
          <span className="font-mono">{money(totals?.income.value, totals?.income.display)}</span>
        </Cell>
        <Cell label="固定支出">
          <span className="font-mono">{money(totals?.expense.value, totals?.expense.display)}</span>
        </Cell>
        <Cell label="浮动结余">
          <span className="font-mono">{money(net)}</span>
        </Cell>
      </div>

      {due.length > 0 ? (
        <ul className="space-y-1">
          {due.slice(0, 4).map((occurrence) => (
            <li key={`${occurrence.id}@${occurrence.month}`} className="flex items-center gap-2">
              <CalendarClock className="h-3.5 w-3.5 shrink-0 text-[var(--color-muted)]" />
              <span className="min-w-0 flex-1 truncate">
                {dayLabel(occurrence.date)} · {occurrence.label}
              </span>
              <span className="font-mono tabular-nums">{cashOf(occurrence)}</span>
              <Button
                variant="subtle"
                className="px-2 py-0.5 text-[11px]"
                disabled={busy}
                onClick={() => void commit(occurrence.id)}
              >
                记入
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <div className="text-[11px] text-[var(--color-muted)]">本月没有待记的固定收支。</div>
      )}

      <div className="mt-auto flex items-center gap-2 text-[11px] text-[var(--color-muted)]">
        <span>{data.auto_commit ? "自动记入已开启（设置 → 自动化）" : "只提醒，不会写入账本"}</span>
        <Button variant="ghost" className="ml-auto px-2 py-0.5 text-[11px]" onClick={onOpen}>
          日程
          <ArrowRight className="ml-1 h-3 w-3" />
        </Button>
      </div>
      {message ? <div className="text-[11px] text-[var(--color-accent)]">{message}</div> : null}
    </div>
  );
}

function Cell({
  label,
  children,
  tone,
}: {
  label: string;
  children: React.ReactNode;
  tone?: "income";
}) {
  return (
    <div className="rounded-lg border border-[var(--color-line)] px-2 py-1.5">
      <div className="text-[11px] text-[var(--color-muted)]">{label}</div>
      <div className={tone === "income" ? "text-[var(--color-accent)]" : undefined}>{children}</div>
    </div>
  );
}

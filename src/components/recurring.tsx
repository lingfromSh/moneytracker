import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Repeat } from "lucide-react";
import { api, queryKeys } from "../lib/ipc";
import { money } from "../lib/format";
import { Badge, Button } from "./ui";

/**
 * "These look like they repeat" — one click writes next month's occurrence.
 * Nothing is written automatically unless 设置 → 自动化 turns it on.
 */
export function RecurringBanner() {
  const queryClient = useQueryClient();
  const [busy, setBusy] = React.useState(false);
  const [message, setMessage] = React.useState<string | null>(null);
  const [dismissed, setDismissed] = React.useState<Set<string>>(new Set());

  const listed = useQuery({
    queryKey: queryKeys.recur(12),
    queryFn: () => api.recurList(12),
    retry: 0,
  });

  const due = (listed.data?.groups ?? []).filter(
    (group) => group.due && !dismissed.has(group.id),
  );
  if (due.length === 0) return null;

  const commit = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await api.recurCommit(due.map((group) => group.id));
      setMessage(
        `已记录 ${result.written} 笔${(result.skipped ?? 0) > 0 ? `，跳过 ${result.skipped} 笔` : ""}`,
      );
      await queryClient.invalidateQueries();
    } catch (cause) {
      setMessage((cause as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const skip = async (id: string) => {
    setBusy(true);
    try {
      await api.recurSkip(id);
      setDismissed((current) => new Set(current).add(id));
      await listed.refetch();
    } catch (cause) {
      setMessage((cause as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mx-4 mt-3 rounded-[var(--radius-card)] border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/8 px-3 py-2">
      <div className="flex items-center gap-2 text-xs">
        <Repeat className="h-3.5 w-3.5 text-[var(--color-accent)]" />
        <span className="font-medium">发现 {due.length} 笔定期交易</span>
        <span className="text-[var(--color-muted)]">按月重复，可一键记入下个月</span>
        <Button
          variant="primary"
          className="ml-auto"
          onClick={() => void commit()}
          disabled={busy}
        >
          全部记入
        </Button>
      </div>
      <ul className="mt-1.5 space-y-1">
        {due.map((group) => (
          <li key={group.id} className="flex items-center gap-2 text-xs">
            <span className="min-w-0 flex-1 truncate">{group.payee}</span>
            <span className="font-mono tabular-nums">
              {money(group.amount.value, group.amount.display)}
            </span>
            <Badge>{`${group.occurrences} 次 · ${group.next_date}`}</Badge>
            <button
              className="text-[11px] text-[var(--color-muted)] underline decoration-dotted"
              onClick={() => void skip(group.id)}
              disabled={busy}
            >
              本月不算
            </button>
          </li>
        ))}
      </ul>
      {message ? (
        <div className="mt-1.5 text-[11px] text-[var(--color-muted)]">{message}</div>
      ) : null}
    </div>
  );
}

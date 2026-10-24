import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { api, queryKeys } from "../lib/ipc";
import { cn, money, percent, shortAccount } from "../lib/format";
import { Button, Card, EmptyState } from "../components/ui";

type Mode = "account" | "tag";

export function Categories() {
  const [mode, setMode] = React.useState<Mode>("account");
  const dashboard = useQuery({
    queryKey: queryKeys.dashboard(12),
    queryFn: () => api.dashboard(12),
    retry: 0,
  });
  const tagged = useQuery({
    queryKey: queryKeys.transactions({ query: "date:>=2026-01-01", offset: 0, limit: 1000, order: "date_desc" }),
    queryFn: () =>
      api.transactions({
        query: null,
        offset: 0,
        limit: 1000,
        order: "date_desc",
      }),
    retry: 0,
    enabled: mode === "tag",
  });

  const tagRows = React.useMemo(() => {
    const totals = new Map<string, { total: number; count: number; display: string }>();
    for (const txn of tagged.data?.items ?? []) {
      const amount = Math.abs(txn.base_value ?? 0);
      if (amount === 0) continue;
      for (const tag of txn.tags) {
        const entry = totals.get(tag.key) ?? { total: 0, count: 0, display: "" };
        entry.total += amount;
        entry.count += 1;
        entry.display = "";
        totals.set(tag.key, entry);
      }
    }
    return Array.from(totals.entries())
      .map(([key, value]) => ({ key, ...value }))
      .sort((a, b) => b.total - a.total);
  }, [tagged.data]);

  const categories = dashboard.data?.categories ?? [];
  const totalSpend = categories.reduce((acc, category) => acc + Math.abs(category.value ?? 0), 0);
  const tagTotal = tagRows.reduce((acc, row) => acc + row.total, 0);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 border-b border-[var(--color-line)] px-4 py-3">
        <div>
          <div className="text-lg font-semibold">分类</div>
          <div className="text-xs text-[var(--color-muted)]">
            {mode === "account" ? "按账户子树汇总本月支出" : "按标签汇总全部交易"}
          </div>
        </div>
        <div className="ml-auto flex items-center gap-1 rounded-lg bg-[var(--color-surface-2)] p-0.5">
          {(
            [
              ["account", "按账户"],
              ["tag", "按标签"],
            ] as [Mode, string][]
          ).map(([value, label]) => (
            <button
              key={value}
              onClick={() => setMode(value)}
              className={cn(
                "rounded-md px-2.5 py-1 text-xs transition",
                mode === value
                  ? "bg-[var(--color-surface)] font-medium shadow-sm"
                  : "text-[var(--color-muted)]",
              )}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-4">
        {mode === "account" ? (
          categories.length === 0 ? (
            <EmptyState title="本月还没有支出" hint="记一笔试试" />
          ) : (
            <Card className="divide-y divide-[var(--color-line)]">
              {categories.map((category) => (
                <div key={category.account} className="flex items-center gap-3 px-3 py-2 text-sm">
                  <div className="min-w-0 flex-1">
                    <div className="truncate">{category.label}</div>
                    <div className="truncate text-[11px] text-[var(--color-muted)]">
                      {shortAccount(category.account)} · {category.txn_count} 笔
                    </div>
                  </div>
                  <div className="w-24">
                    <div className="h-1.5 overflow-hidden rounded-full bg-[var(--color-line)]">
                      <div
                        className="h-full rounded-full bg-[var(--color-accent)]"
                        style={{
                          width: `${totalSpend > 0 ? (Math.abs(category.value ?? 0) / totalSpend) * 100 : 0}%`,
                        }}
                      />
                    </div>
                  </div>
                  <span className="w-14 text-right text-xs text-[var(--color-muted)]">
                    {percent(category.share ?? 0)}
                  </span>
                  <span className="w-28 text-right font-mono text-xs">
                    {money(category.value, category.display)}
                  </span>
                </div>
              ))}
            </Card>
          )
        ) : tagged.isLoading ? (
          <EmptyState title="正在汇总标签…" />
        ) : tagRows.length === 0 ? (
          <EmptyState
            title="还没有带标签的交易"
            hint="快录时用 +餐饮 这样的标签即可"
            action={
              <Button variant="primary" className="mt-2" onClick={() => void api.showCapture()}>
                记一笔
              </Button>
            }
          />
        ) : (
          <Card className="divide-y divide-[var(--color-line)]">
            {tagRows.map((row) => (
              <div key={row.key} className="flex items-center gap-3 px-3 py-2 text-sm">
                <span className="min-w-0 flex-1 truncate">#{row.key}</span>
                <span className="text-[11px] text-[var(--color-muted)]">{row.count} 笔</span>
                <div className="w-24">
                  <div className="h-1.5 overflow-hidden rounded-full bg-[var(--color-line)]">
                    <div
                      className="h-full rounded-full bg-[var(--color-accent)]"
                      style={{ width: `${tagTotal > 0 ? (row.total / tagTotal) * 100 : 0}%` }}
                    />
                  </div>
                </div>
                <span className="w-28 text-right font-mono text-xs">{money(row.total)}</span>
              </div>
            ))}
          </Card>
        )}
      </div>
    </div>
  );
}

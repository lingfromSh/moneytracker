import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import type { AccountNodeDto } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn, kindLabel, money } from "../lib/format";
import { Badge, Card, EmptyState, Input } from "../components/ui";

const KIND_TONE: Record<string, "accent" | "neutral" | "warn" | "danger"> = {
  A: "accent",
  L: "danger",
  R: "accent",
  X: "warn",
};

export function Accounts() {
  const [filter, setFilter] = React.useState("");
  const [expanded, setExpanded] = React.useState<Set<string>>(new Set(["assets", "expenses"]));
  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: () => api.accounts(),
    retry: 0,
  });

  const snapshot = useQuery({
    queryKey: queryKeys.snapshot,
    queryFn: () => api.snapshot(),
    retry: 0,
  });
  const foreign = (snapshot.data?.balances ?? []).filter(
    (row) => row.commodity !== snapshot.data?.vault.base_commodity,
  );

  const tree = data ?? [];
  const totals = React.useMemo(() => {
    const sum = (nodes: AccountNodeDto[], kind: string) =>
      flatten(nodes)
        .filter((node) => node.kind === kind)
        .reduce((acc, node) => acc + (node.own[0]?.value ?? node.total[0]?.value ?? 0), 0);
    return {
      assets: sum(tree, "A"),
      liabilities: sum(tree, "L"),
      income: sum(tree, "R"),
      expenses: sum(tree, "X"),
    };
  }, [tree]);

  if (error) return <EmptyState title="账户读取失败" hint={String((error as Error).message)} />;
  if (isLoading) return <EmptyState title="正在读取账户…" />;

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 border-b border-[var(--color-line)] px-4 py-3">
        <div>
          <div className="text-lg font-semibold">账户</div>
          <div className="text-xs text-[var(--color-muted)]">点击账户下钻到相关流水</div>
        </div>
        <div className="ml-auto w-64">
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="筛选账户，例如 餐饮"
            aria-label="筛选账户"
          />
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2 px-4 py-3 lg:grid-cols-4">
        <Stat label="资产" value={totals.assets} />
        <Stat label="负债" value={totals.liabilities} />
        <Stat label="收入" value={totals.income} />
        <Stat label="支出" value={totals.expenses} />
      </div>

      {foreign.length > 0 ? (
        <div className="px-4 pb-2">
          <Card className="divide-y divide-[var(--color-line)]">
            <div className="px-3 py-2 text-[11px] text-[var(--color-muted)]">
              原币余额（未折算，仅作参考）
            </div>
            {foreign.map((row) => (
              <div
                key={`${row.account}-${row.commodity}`}
                className="flex items-center gap-2 px-3 py-1.5 text-xs"
              >
                <span className="min-w-0 flex-1 truncate">{row.account}</span>
                <span className="font-mono">{row.display}</span>
              </div>
            ))}
          </Card>
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-auto px-4 pb-4">
        {tree.length === 0 ? (
          <EmptyState title="还没有账户" hint="记一笔，账户会自动出现" />
        ) : (
          <Card className="divide-y divide-[var(--color-line)]">
            {tree.map((node) => (
              <AccountRow
                key={node.full}
                node={node}
                depth={0}
                filter={filter.trim().toLowerCase()}
                expanded={expanded}
                onToggle={(full) =>
                  setExpanded((current) => {
                    const next = new Set(current);
                    if (next.has(full)) next.delete(full);
                    else next.add(full);
                    return next;
                  })
                }
              />
            ))}
          </Card>
        )}
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <Card className="p-3">
      <div className="text-[11px] text-[var(--color-muted)]">{label}</div>
      <div className="mt-1 font-mono text-sm">{money(value)}</div>
    </Card>
  );
}

function flatten(nodes: AccountNodeDto[]): AccountNodeDto[] {
  return nodes.flatMap((node) => [node, ...flatten(node.children)]);
}

function AccountRow({
  node,
  depth,
  filter,
  expanded,
  onToggle,
}: {
  node: AccountNodeDto;
  depth: number;
  filter: string;
  expanded: Set<string>;
  onToggle: (full: string) => void;
}) {
  const matches = !filter || node.full.toLowerCase().includes(filter);
  const childMatches = React.useMemo(
    () => flatten(node.children).some((child) => child.full.toLowerCase().includes(filter)),
    [filter, node.children],
  );
  if (!matches && !childMatches) return null;

  const open = expanded.has(node.full) || (filter.length > 0 && childMatches);
  const hasChildren = node.children.length > 0;
  const amount = node.total[0];

  const drill = () => {
    window.localStorage.setItem("moneytracker.pendingSearch", `acct:${node.full}`);
    window.location.hash = "#/transactions";
  };

  return (
    <>
      <div
        className={cn(
          "flex items-center gap-2 px-3 py-1.5 text-sm hover:bg-[var(--color-surface-2)]/50",
          depth === 0 && "font-medium",
        )}
        style={{ paddingLeft: 12 + depth * 16 }}
      >
        <button
          onClick={() => onToggle(node.full)}
          className={cn("w-4 text-xs text-[var(--color-muted)]", !hasChildren && "opacity-0")}
          aria-label={open ? "折叠" : "展开"}
        >
          {open ? "▾" : "▸"}
        </button>
        <button onClick={drill} className="min-w-0 flex-1 truncate text-left">
          {node.name}
          {node.declared ? (
            <span className="ml-1 text-[10px] text-[var(--color-muted)]">声明</span>
          ) : null}
        </button>
        <Badge tone={KIND_TONE[node.kind] ?? "neutral"}>{kindLabel(node.kind)}</Badge>
        <span className="w-32 text-right font-mono text-xs">
          {money(amount?.value, amount?.display)}
        </span>
      </div>
      {open
        ? node.children.map((child) => (
            <AccountRow
              key={child.full}
              node={child}
              depth={depth + 1}
              filter={filter}
              expanded={expanded}
              onToggle={onToggle}
            />
          ))
        : null}
    </>
  );
}

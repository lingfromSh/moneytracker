import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { Copy } from "lucide-react";
import type {
  AmountDto,
  BalanceSheetDto_Serialize,
  BridgeDto,
  IncomeStatementDto_Serialize,
  SectionDto_Serialize,
} from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn, money } from "../lib/format";
import { Badge, Button, Card, CardHeader } from "../components/ui";

type Tab = "balance" | "income" | "bridge";

export function Reports() {
  const [tab, setTab] = React.useState<Tab>("income");
  const [periodKey, setPeriodKey] = React.useState<string | null>(null);
  const [copied, setCopied] = React.useState(false);

  const periods = useQuery({ queryKey: queryKeys.periods, queryFn: () => api.reportPeriods(), retry: 0 });
  const period = React.useMemo(() => {
    const list = periods.data ?? [];
    return list.find((p) => p.key === periodKey) ?? list[0] ?? null;
  }, [periods.data, periodKey]);

  const income = useQuery({
    queryKey: queryKeys.incomeStatement(period?.from ?? "", period?.to ?? ""),
    queryFn: () => api.statementIncome(period!.from, period!.to),
    enabled: Boolean(period),
    retry: 0,
  });
  const balance = useQuery({
    queryKey: queryKeys.balanceSheet(period?.to ?? null),
    queryFn: () => api.statementBalance(period!.to),
    enabled: Boolean(period),
    retry: 0,
  });
  const bridge = useQuery({
    queryKey: queryKeys.bridge(period?.from ?? "", period?.to ?? ""),
    queryFn: () => api.statementBridge(period!.from, period!.to),
    enabled: Boolean(period),
    retry: 0,
  });

  const copy = React.useCallback(async () => {
    if (!period) return;
    try {
      const markdown = await api.reportMarkdown(period.to.slice(0, 7));
      await navigator.clipboard.writeText(markdown);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard is best effort (it can be blocked outside a user gesture) */
    }
  }, [period]);

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex flex-wrap items-center justify-between gap-2 px-4 pb-3 pt-4">
        <div>
          <div className="text-lg font-semibold">报表</div>
          <div className="text-xs text-[var(--color-muted)]">
            {period ? `${period.label} · ${period.from} → ${period.to}` : "尚无交易"}
          </div>
        </div>
        <div className="flex items-center gap-1">
          {copied ? <span className="text-xs text-[var(--color-muted)]">已复制</span> : null}
          <Button variant="ghost" onClick={() => void copy()} disabled={!period}>
            <Copy className="mr-1 h-3.5 w-3.5" />
            复制 Markdown
          </Button>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-1 px-4 pb-2">
        {(periods.data ?? []).map((item) => (
          <button
            key={item.key}
            onClick={() => setPeriodKey(item.key)}
            className={cn(
              "rounded-lg px-2.5 py-1 text-xs transition",
              item.key === period?.key
                ? "bg-[var(--color-surface)] font-medium shadow-sm"
                : "hover:bg-[var(--color-surface)]/70",
            )}
          >
            {item.label}
          </button>
        ))}
      </div>

      <div className="flex items-center gap-1 px-4 pb-3">
        {(
          [
            ["balance", "资产负债表"],
            ["income", "损益表"],
            ["bridge", "净值变动"],
          ] as [Tab, string][]
        ).map(([key, label]) => (
          <button
            key={key}
            onClick={() => setTab(key)}
            className={cn(
              "rounded-lg border px-2.5 py-1 text-xs",
              tab === key
                ? "border-[var(--color-accent)]/60 bg-[var(--color-accent)]/10 font-medium"
                : "border-[var(--color-line)]",
            )}
          >
            {label}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto overflow-x-hidden px-4 pb-6">
        {tab === "balance" ? <BalanceView sheet={balance.data} loading={balance.isLoading} /> : null}
        {tab === "income" ? <IncomeView stmt={income.data} loading={income.isLoading} /> : null}
        {tab === "bridge" ? <BridgeView bridge={bridge.data} loading={bridge.isLoading} /> : null}
      </div>
    </div>
  );
}

function Section({ section }: { section: SectionDto_Serialize }) {
  return (
    <Card className="p-4">
      <CardHeader title={section.title} hint={`${section.rows.length} 个账户`} />
      <div className="-mx-1 mt-2 overflow-x-auto px-1">
      <table className="w-full min-w-[520px] text-xs">
        <tbody>
          {section.rows.map((row) => (
            <tr key={row.account} className="border-t border-[var(--color-line)]/60">
              <td className="py-1">
                <span>{row.label}</span>
                <span className="ml-1 text-[10px] text-[var(--color-muted)]">{row.account}</span>
              </td>
              <td className="py-1 text-right tabular-nums">{cell(row.unit)}</td>
            </tr>
          ))}
          <tr className="border-t border-[var(--color-line)] font-medium">
            <td className="py-1.5">合计</td>
            <td className="py-1.5 text-right tabular-nums">{cell(section.total)}</td>
          </tr>
        </tbody>
      </table>
      </div>
    </Card>
  );
}

function cell(amount: AmountDto): string {
  return money(amount?.value, amount?.display);
}

function BalanceView({ sheet, loading }: { sheet?: BalanceSheetDto_Serialize; loading: boolean }) {
  if (loading || !sheet) return <div className="text-xs text-[var(--color-muted)]">正在计算…</div>;
  return (
    <>
      {sheet.note ? (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 text-xs text-amber-700 dark:text-amber-300">
          {sheet.note}
        </div>
      ) : null}
      <Card className="p-4">
        <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
          <span>截至 {sheet.as_of}</span>
          <span className="flex items-center gap-2">
            <Badge tone="accent">净资产 {cell(sheet.net_worth)}</Badge>
            <span className="text-[11px] text-[var(--color-muted)]">
              权益 {cell(sheet.equity.total)} + 累计结余 {cell(sheet.cumulative_net)}
            </span>
          </span>
        </div>
        <div className="mt-1 text-[11px] text-[var(--color-muted)]">
          资产 − 负债 = 权益 + 累计结余，双式记账恒等式每期成立
        </div>
      </Card>
      <Section section={sheet.assets} />
      <Section section={sheet.liabilities} />
      <Section section={sheet.equity} />
    </>
  );
}

function IncomeView({ stmt, loading }: { stmt?: IncomeStatementDto_Serialize; loading: boolean }) {
  if (loading || !stmt) return <div className="text-xs text-[var(--color-muted)]">正在计算…</div>;
  return (
    <>
      <Card className="p-4">
        <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
          <span>
            {stmt.from} → {stmt.to}
          </span>
          <Badge tone={(stmt.net.value ?? 0) >= 0 ? "accent" : "danger"}>
            本期结余 {cell(stmt.net)}
          </Badge>
        </div>
      </Card>
      <Section section={stmt.income} />
      <Section section={stmt.expenses} />
    </>
  );
}

function BridgeView({ bridge, loading }: { bridge?: BridgeDto; loading: boolean }) {
  if (loading || !bridge) return <div className="text-xs text-[var(--color-muted)]">正在计算…</div>;
  const rows: [string, string, string][] = [
    ["期初净资产", cell(bridge.opening), "区间开始前的余额"],
    ["+ 收入", cell(bridge.income), "区间内 R 类过账"],
    ["− 支出", cell(bridge.expense), "区间内 X 类过账"],
    ["± 汇率/估值差", cell(bridge.valuation), "多币种按交易日汇率重估的残差"],
    ["= 期末净资产", cell(bridge.closing), "与资产负债表一致"],
  ];
  return (
    <Card className="p-4">
      <CardHeader
        title="净值变动"
        hint={bridge.reconciled ? "已对齐" : "存在未解释差额，检查缺失汇率"}
      />
      <div className="-mx-1 mt-2 overflow-x-auto px-1">
      <table className="w-full min-w-[520px] text-xs">
        <tbody>
          {rows.map(([label, value, hint], index) => (
            <tr
              key={label}
              className={cn(
                "border-t border-[var(--color-line)]/60",
                index === rows.length - 1 && "font-medium",
              )}
            >
              <td className="py-1.5">
                {label}
                <div className="text-[10px] text-[var(--color-muted)]">{hint}</div>
              </td>
              <td className="py-1.5 text-right tabular-nums">{value}</td>
            </tr>
          ))}
        </tbody>
      </table>
      </div>
    </Card>
  );
}

import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Target, TrendingUp, Wallet } from "lucide-react";
import type { PlanOverviewDto_Serialize } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn, money, pct } from "../lib/format";
import { AskCard } from "../components/ask-card";
import { Badge, Button, Card, CardHeader, EmptyState, Field, Modal, Textarea } from "../components/ui";

const num = (value: number | null | undefined): number => Number(value ?? 0);

const SAMPLE_PLAN = `; MoneyTracker plan — read by the app, never included by index.ledger.
assume  ; return:6% inflation:2.5% reserve:6
pool    ; accounts:assets:invest,assets:cash,assets:bank
alloc   ; equity:60% bond:20% cash:20%
goal fire 财富自由 ; swr:4%
goal account 退休金 ; account:assets:invest:retirement target:1000000.00 CNY by:2040-01-01
debt 房贷 ; account:liabilities:mortgage rate:3.15% payment:8200.00 CNY
`;

export function Plan() {
  const queryClient = useQueryClient();
  const [editing, setEditing] = React.useState(false);
  const [draft, setDraft] = React.useState("");
  const [saving, setSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const plan = useQuery({ queryKey: queryKeys.plan, queryFn: () => api.planOverview(), retry: 0 });

  const startEditing = React.useCallback(() => {
    setDraft(plan.data?.goals_text?.trim() ? plan.data.goals_text : SAMPLE_PLAN);
    setError(null);
    setEditing(true);
  }, [plan.data?.goals_text]);

  const save = React.useCallback(async () => {
    setSaving(true);
    setError(null);
    try {
      await api.goalsSave(draft);
      await queryClient.invalidateQueries();
      setEditing(false);
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setSaving(false);
    }
  }, [draft, queryClient]);

  if (plan.isLoading) {
    return <div className="p-6 text-sm text-[var(--color-muted)]">正在读取规划…</div>;
  }
  if (plan.error) {
    return (
      <div className="p-6 text-sm text-[var(--color-danger)]">
        读取规划失败：{(plan.error as Error).message}
      </div>
    );
  }
  const data = plan.data as PlanOverviewDto_Serialize;
  if (!data) return null;

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex items-center justify-between px-4 pb-3 pt-4">
        <div>
          <div className="text-lg font-semibold">规划</div>
          <div className="text-xs text-[var(--color-muted)]">
            来自 goals.ledger · 名义 {pct(num(data.assumptions.nominal_return))} · 通胀{" "}
            {pct(num(data.assumptions.inflation))} · 实际{" "}
            {pct(num(data.fire.real_return))}
          </div>
        </div>
        <Button variant="subtle" onClick={startEditing}>
          <Pencil className="mr-1 h-3.5 w-3.5" />
          {data.has_goals ? "编辑计划" : "设置目标"}
        </Button>
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto overflow-x-hidden px-4 pb-6">
        {!data.has_goals ? (
          <Card className="p-4">
            <EmptyState
              title="还没有规划文件"
              hint="写一份 goals.ledger，就能看到财富自由进度、储备月数、负债还清时间和资产配置漂移。"
              action={
                <Button variant="primary" onClick={startEditing}>
                  创建规划
                </Button>
              }
            />
          </Card>
        ) : null}

        <FireCard data={data} />
        <div className="grid gap-3 lg:grid-cols-2">
          <ReserveCard data={data} />
          <PoolCard data={data} />
        </div>
        {data.goals.length > 0 ? <GoalsCard data={data} /> : null}
        <DebtCard data={data} />
        <AskCard />
        <AllocationCard data={data} />

        {data.diagnostics.length > 0 ? (
          <Card className="p-3">
            <div className="mb-1 text-xs font-medium text-amber-600 dark:text-amber-400">
              goals.ledger 有 {data.diagnostics.length} 处问题
            </div>
            <ul className="space-y-1 text-xs text-[var(--color-muted)]">
              {data.diagnostics.map((d, i) => (
                <li key={`${d.code}-${i}`}>
                  {d.line ? `第 ${d.line} 行：` : ""}
                  {d.message}
                </li>
              ))}
            </ul>
          </Card>
        ) : null}
      </div>

      <Modal open={editing} onClose={() => setEditing(false)} title="规划文件 goals.ledger" width="720px">
        <div className="space-y-3">
          <Field
            label="计划内容"
            hint="一行一条指令：assume / pool / alloc / goal fire / goal account / debt"
          >
            <Textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              className="h-72 font-mono text-xs"
              aria-label="计划内容"
            />
          </Field>
          {error ? <div className="text-xs text-[var(--color-danger)]">{error}</div> : null}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setEditing(false)}>
              取消
            </Button>
            <Button variant="primary" onClick={() => void save()} disabled={saving}>
              {saving ? "保存中…" : "保存"}
            </Button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

function Bar({
  value,
  tone = "accent",
}: {
  value: number | null | undefined;
  tone?: "accent" | "warn";
}) {
  const numeric = num(value);
  const clamped = Math.max(0, Math.min(100, Number.isFinite(numeric) ? numeric : 0));
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-line)]">
      <div
        className={cn(
          "h-full rounded-full",
          tone === "warn" ? "bg-[var(--color-danger)]" : "bg-[var(--color-accent)]",
        )}
        style={{ width: `${clamped}%` }}
      />
    </div>
  );
}

function FireCard({ data }: { data: PlanOverviewDto_Serialize }) {
  const fire = data.fire;
  return (
    <Card className="p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2 text-sm font-medium">
            <Target className="h-4 w-4 text-[var(--color-accent)]" />
            财富自由进度
          </div>
          <div className="mt-1 text-xs text-[var(--color-muted)]">
            可投资资产 {money(fire.balance.value, fire.balance.display)} · 年支出{" "}
            {money(fire.annual_expense.value, fire.annual_expense.display)} · 月均储蓄{" "}
            {money(fire.contribution.value, fire.contribution.display)}
          </div>
        </div>
        <div className="text-right">
          <div className="text-2xl font-semibold tabular-nums">{pct(num(fire.progress))}</div>
          <div className="text-[11px] text-[var(--color-muted)]">
            按 {pct(num(fire.primary_swr))} 提取率
          </div>
        </div>
      </div>
      <div className="mt-3">
        <Bar value={fire.progress} />
      </div>
      <div className="mt-3 grid gap-2 sm:grid-cols-3">
        {fire.targets.map((target) => (
          <div
            key={target.swr}
            className={cn(
              "rounded-lg border border-[var(--color-line)] p-2.5",
              Math.abs(num(target.swr) - num(fire.primary_swr)) < 0.001 && "border-[var(--color-accent)]/60",
            )}
          >
            <div className="flex items-center justify-between text-[11px] text-[var(--color-muted)]">
              <span>SWR {pct(num(target.swr))}</span>
              {target.achieved ? <Badge tone="accent">已达成</Badge> : null}
            </div>
            <div className="mt-1 text-sm font-medium tabular-nums">
              {money(target.target.value, target.target.display)}
            </div>
            <div className="text-[11px] text-[var(--color-muted)]">{target.eta_label}</div>
          </div>
        ))}
      </div>
      {fire.note ? (
        <div className="mt-2 text-xs text-amber-600 dark:text-amber-400">{fire.note}</div>
      ) : null}
    </Card>
  );
}

function ReserveCard({ data }: { data: PlanOverviewDto_Serialize }) {
  const reserve = data.reserve;
  const progress = num(reserve.target_months) > 0 ? (num(reserve.months) / num(reserve.target_months)) * 100 : 0;
  return (
    <Card className="p-4">
      <CardHeader
        title="储备月数"
        hint={`目标 ${reserve.target_months} 个月 · 现金 ${money(reserve.cash.value, reserve.cash.display)}`}
      />
      <div className="mt-2 flex items-end justify-between">
        <div className="text-2xl font-semibold tabular-nums">{num(reserve.months).toFixed(1)} 个月</div>
        <div className="text-[11px] text-[var(--color-muted)]">
          月支出 {money(reserve.monthly_expense.value, reserve.monthly_expense.display)}
        </div>
      </div>
      <div className="mt-3">
        <Bar value={progress} tone={progress < 100 ? "warn" : "accent"} />
      </div>
    </Card>
  );
}

function PoolCard({ data }: { data: PlanOverviewDto_Serialize }) {
  return (
    <Card className="p-4">
      <CardHeader title="可投资资产池" hint={`${data.pool.length} 个账户`} />
      <div className="mt-2 max-h-40 space-y-1 overflow-auto text-xs">
        {data.pool.length === 0 ? (
          <div className="text-[var(--color-muted)]">还没有池子账户，可在 pool 行指定</div>
        ) : (
          data.pool.map((member) => (
            <div key={member.account} className="flex items-center justify-between gap-2">
              <span className="truncate">
                {member.account}
                {member.class ? (
                  <span className="ml-1 text-[10px] text-[var(--color-muted)]">· {member.class}</span>
                ) : null}
              </span>
              <span className="tabular-nums">{money(member.amount.value, member.amount.display)}</span>
            </div>
          ))
        )}
      </div>
    </Card>
  );
}

function GoalsCard({ data }: { data: PlanOverviewDto_Serialize }) {
  return (
    <Card className="p-4">
      <CardHeader title="目标" hint="账户目标" />
      <div className="mt-2 space-y-2">
        {data.goals.map((goal) => (
          <div key={`${goal.label}-${goal.account}`}>
            <div className="flex items-center justify-between text-xs">
              <span className="flex items-center gap-1.5">
                <Wallet className="h-3.5 w-3.5 text-[var(--color-muted)]" />
                {goal.label}
                {goal.account ? (
                  <span className="text-[10px] text-[var(--color-muted)]">{goal.account}</span>
                ) : null}
              </span>
              <span className="tabular-nums">
                {money(goal.current.value, goal.current.display)} /{" "}
                {money(goal.target.value, goal.target.display)}
              </span>
            </div>
            <div className="mt-1">
              <Bar value={goal.progress} />
            </div>
            {goal.by ? (
              <div className="mt-0.5 text-[11px] text-[var(--color-muted)]">
                目标日期 {goal.by}
                {goal.months_left !== null && goal.months_left !== undefined
                  ? goal.months_left >= 0
                    ? ` · 还有 ${goal.months_left} 个月`
                    : ` · 已过期 ${Math.abs(goal.months_left)} 个月`
                  : ""}
              </div>
            ) : null}
          </div>
        ))}
      </div>
    </Card>
  );
}

function DebtCard({ data }: { data: PlanOverviewDto_Serialize }) {
  if (data.debts.length === 0) {
    return (
      <Card className="p-4">
        <CardHeader title="负债进度" hint="没有未结清的负债" />
      </Card>
    );
  }
  return (
    <Card className="p-4">
      <CardHeader title="负债进度" hint={`${data.debts.length} 笔`} />
      <div className="mt-2 overflow-x-auto">
        <table className="w-full min-w-[560px] text-xs">
          <thead className="text-[var(--color-muted)]">
            <tr className="text-left">
              <th className="py-1 font-normal">负债</th>
              <th className="py-1 text-right font-normal">余额</th>
              <th className="py-1 text-right font-normal">利率</th>
              <th className="py-1 text-right font-normal">月供</th>
              <th className="py-1 text-right font-normal">已付利息</th>
              <th className="py-1 text-right font-normal">还清</th>
            </tr>
          </thead>
          <tbody>
            {data.debts.map((debt) => (
              <tr key={debt.account} className="border-t border-[var(--color-line)]">
                <td className="py-1.5">
                  <div className="flex items-center gap-1.5">
                    <TrendingUp className="h-3 w-3 text-[var(--color-danger)]" />
                    {debt.label}
                  </div>
                  {debt.note ? (
                    <div className="text-[10px] text-[var(--color-muted)]">{debt.note}</div>
                  ) : null}
                  {debt.extra.length > 0 ? (
                    <div className="text-[10px] text-[var(--color-muted)]">
                      多还 {debt.extra.map((e) => e.extra.display).join(" / ")}：最早可省{" "}
                      {debt.extra[0].months_saved} 个月、{money(debt.extra[0].interest_saved.value, debt.extra[0].interest_saved.display)}
                    </div>
                  ) : null}
                </td>
                <td className="py-1.5 text-right tabular-nums">{money(debt.balance.value, debt.balance.display)}</td>
                <td className="py-1.5 text-right tabular-nums">
                  {debt.rate === null || debt.rate === undefined ? "—" : pct(num(debt.rate), 2)}
                </td>
                <td className="py-1.5 text-right tabular-nums">
                  {debt.payment ? money(debt.payment.value, debt.payment.display) : "—"}
                </td>
                <td className="py-1.5 text-right tabular-nums">
                  {money(debt.interest_paid.value, debt.interest_paid.display)}
                </td>
                <td className="py-1.5 text-right tabular-nums">
                  {debt.payoff_date ?? "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Card>
  );
}

function AllocationCard({ data }: { data: PlanOverviewDto_Serialize }) {
  const rows = data.allocation.rows;
  if (rows.length === 0) return null;
  return (
    <Card className="p-4">
      <CardHeader title="资产配置" hint="按 accounts.ledger 的 class: 标签" />
      <div className="mt-2 space-y-2">
        {rows.map((row) => {
          const drift = row.drift;
          const off = drift !== null && drift !== undefined && Math.abs(drift) > 5;
          return (
            <div key={row.class}>
              <div className="flex items-center justify-between text-xs">
                <span className="flex items-center gap-1.5">
                  {row.class}
                  {off ? <Badge tone="warn">偏离 {drift.toFixed(1)}pp</Badge> : null}
                </span>
                <span className="tabular-nums">
                  {money(row.current.value, row.current.display)} · {num(row.current_share).toFixed(1)}%
                  {row.target_share !== null && row.target_share !== undefined
                    ? ` / 目标 ${num(row.target_share).toFixed(0)}%`
                    : ""}
                </span>
              </div>
              <div className="mt-1">
                <Bar value={row.current_share} />
              </div>
            </div>
          );
        })}
      </div>
      {data.allocation.untagged.length > 0 ? (
        <div className="mt-2 text-[11px] text-[var(--color-muted)]">
          未标注类别：{data.allocation.untagged.slice(0, 6).join("、")}
          {data.allocation.untagged.length > 6 ? " …" : ""}
        </div>
      ) : null}
    </Card>
  );
}

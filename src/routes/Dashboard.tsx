import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { GridLayout } from "react-grid-layout";
import type { Layout } from "react-grid-layout";
import type { DashboardDto_Serialize, PlanOverviewDto_Serialize } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { useElementWidth } from "../lib/use-element-width";
import { CARD_KEYS, KPI_KEYS } from "../lib/views";
import { cn, money, monthLabel, pct, percent } from "../lib/format";
import { Badge, Button, Card, CardHeader, EmptyState, Modal } from "../components/ui";
import { ScheduleCard } from "../components/schedule";
import {
  BudgetBars,
  CategoryDonut,
  CategoryList,
  ForecastLine,
  Heatmap,
  InsightList,
  MonthlyBars,
  NetWorthArea,
  SankeyDiagram,
  StackedArea,
} from "../components/charts";

const GRID = { cols: 12, rowHeight: 46, margin: [12, 12] as [number, number] };

const DEFAULT_LAYOUT: Layout = [
  { i: "months", x: 0, y: 0, w: 8, h: 4, minW: 4, minH: 3 },
  { i: "categories", x: 8, y: 0, w: 4, h: 4, minW: 3, minH: 3 },
  { i: "networth", x: 0, y: 4, w: 8, h: 3, minW: 4, minH: 2 },
  { i: "budget", x: 8, y: 4, w: 4, h: 3, minW: 3, minH: 2 },
  { i: "stacked", x: 0, y: 7, w: 6, h: 3, minW: 4, minH: 2 },
  { i: "forecast", x: 6, y: 7, w: 6, h: 3, minW: 4, minH: 2 },
  { i: "heatmap", x: 8, y: 10, w: 4, h: 5, minW: 3, minH: 3 },
  { i: "sankey", x: 0, y: 10, w: 8, h: 5, minW: 4, minH: 3 },
  { i: "insights", x: 8, y: 15, w: 4, h: 4, minW: 3, minH: 3 },
  { i: "schedule", x: 0, y: 19, w: 8, h: 4, minW: 4, minH: 3 },
];

function Panel({
  title,
  hint,
  children,
  className,
}: {
  title: React.ReactNode;
  hint?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <Card className={cn("flex h-full min-h-0 flex-col overflow-hidden", className)}>
      <div className="drag-handle shrink-0 cursor-grab active:cursor-grabbing">
        <CardHeader title={title} hint={hint} />
      </div>
      <div className="min-h-0 min-w-0 flex-1 overflow-hidden">{children}</div>
    </Card>
  );
}

export function Dashboard() {
  const [months, setMonths] = React.useState(12);
  const [reloading, setReloading] = React.useState(false);
  const [reloadedAt, setReloadedAt] = React.useState<number | null>(null);
  const [reloadError, setReloadError] = React.useState<string | null>(null);
  const queryClient = useQueryClient();
  const { ref: gridRef, width } = useElementWidth(960);
  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.dashboard(months),
    queryFn: () => api.dashboard(months),
    retry: 0,
  });
  const settings = useQuery({ queryKey: queryKeys.settings, queryFn: () => api.settings() });
  const hiddenCards = settings.data?.hidden_cards ?? [];
  const kpiKeys =
    settings.data && settings.data.kpi_cards.length > 0
      ? settings.data.kpi_cards
      : KPI_KEYS.map((entry) => entry.key as string);
  const compact = settings.data?.density === "compact";
  const persistPrefs = async (patch: {
    hidden_cards?: string[];
    kpi_cards?: string[];
  }) => {
    const current = settings.data;
    if (!current) return;
    await api.saveSettings({ ...current, ...patch });
    await queryClient.invalidateQueries({ queryKey: queryKeys.settings });
  };

  const plan = useQuery({ queryKey: queryKeys.plan, queryFn: () => api.planOverview(), retry: 0 });
  const insights = useQuery({
    queryKey: queryKeys.insights,
    queryFn: () => api.insights(),
    retry: 0,
  });
  const insightList = insights.data ?? [];

  // Re-reads every ledger file from disk. The watcher covers external edits, but
  // a manual button is what people reach for after editing in another editor.
  const reload = React.useCallback(async () => {
    setReloading(true);
    setReloadError(null);
    try {
      await api.reload();
      await queryClient.invalidateQueries();
      setReloadedAt(Date.now());
    } catch (cause) {
      setReloadError((cause as Error).message);
    } finally {
      setReloading(false);
    }
  }, [queryClient]);

  React.useEffect(() => {
    if (reloadedAt === null) return;
    const timer = window.setTimeout(() => setReloadedAt(null), 2000);
    return () => window.clearTimeout(timer);
  }, [reloadedAt]);

  const layout = React.useMemo<Layout>(() => {
    const raw = settings.data?.dashboard_layout;
    if (!raw) return DEFAULT_LAYOUT;
    try {
      const parsed = JSON.parse(raw) as Layout;
      if (!Array.isArray(parsed) || parsed.length === 0) return DEFAULT_LAYOUT;
      // A panel added by a newer version is missing from an older saved layout;
      // stack the missing ones below the arrangement the user already has
      // instead of silently hiding them.
      const known = new Set(parsed.map((item) => item.i));
      const missing = DEFAULT_LAYOUT.filter((item) => !known.has(item.i));
      if (missing.length === 0) return parsed;
      let bottom = parsed.reduce((low, item) => Math.max(low, item.y + item.h), 0);
      const appended = missing.map((item) => {
        const placed = { ...item, x: 0, y: bottom };
        bottom += item.h;
        return placed;
      });
      return [...parsed, ...appended];
    } catch {
      return DEFAULT_LAYOUT;
    }
  }, [settings.data?.dashboard_layout]);

  // Hiding a card removes its grid slot; the child stays mounted so its query
  // cache survives a toggle.
  const visibleLayout = React.useMemo(
    () => layout.filter((item) => !hiddenCards.includes(item.i)),
    [layout, hiddenCards],
  );

  const saveLayout = React.useMemo(() => {
    let timer: number | undefined;
    return (next: Layout) => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        const current = settings.data;
        if (!current) return;
        const serialised = JSON.stringify(next);
        // The grid is remounted on every resize, which re-emits its layout; only
        // persist when the arrangement actually changed.
        if (current.dashboard_layout === serialised) return;
        // `Promise.resolve` keeps this safe even if a test double returns nothing.
        void Promise.resolve(
          api.saveSettings({ ...current, dashboard_layout: serialised }),
        ).catch(() => {
          /* layout persistence is best effort */
        });
      }, 600);
    };
  }, [settings.data]);

  if (error) {
    return <EmptyState title="看板加载失败" hint={String((error as Error).message)} />;
  }
  if (isLoading || !data) {
    return <EmptyState title="正在读取账本…" />;
  }

  const panels: Record<string, React.ReactNode> = {
      months: (
              <Panel title="月度收支" hint={`最近 ${months} 个月`}>
                <MonthlyBars months={data.months} />
              </Panel>
      ),
      categories: (
              <Panel title="本月支出结构" hint={data.kpis.month ? monthLabel(data.kpis.month) : ""}>
                <CategoryDonut categories={data.categories} />
              </Panel>
      ),
      networth: (
              <Panel title="净资产曲线" hint="含所有资产与负债">
                <NetWorthArea
                  points={data.net_worth_series}
                  target={plan.data?.fire?.targets?.[0]?.target?.value ?? null}
                  targetLabel={plan.data?.fire ? "自由目标" : undefined}
                />
              </Panel>
      ),
      budget: (
              <Panel title="预算 vs 实际" hint="来自 budgets.ledger">
                <BudgetBars rows={data.budget} />
              </Panel>
      ),
      stacked: (
              <Panel title="分类趋势" hint="按二级分类堆叠">
                <StackedArea stacked={data.stacked} />
              </Panel>
      ),
      schedule: (
              <Panel title="固定收支" hint="来自 recurring.ledger">
                <ScheduleCard onOpen={() => (window.location.hash = "#/schedule")} />
              </Panel>
      ),
      forecast: (
              <Panel title="现金流预测" hint="虚线段为预测，不写入账本">
                <ForecastLine points={data.forecast} />
              </Panel>
      ),
      sankey: (
              <Panel title="资金流向" hint="收入 → 账户 → 支出分类">
                <SankeyDiagram data={data.sankey} />
              </Panel>
      ),
      insights: (
              <Panel title="洞察" hint="按规则算出来的变化与提醒">
                <InsightList items={insightList} />
              </Panel>
      ),
      heatmap: (
              <Panel title="支出日历" hint="颜色越深支出越多">
                <div className="flex h-full flex-col justify-between">
                  <Heatmap points={data.heatmap} />
                  <div className="max-h-24 overflow-auto">
                    <CategoryList categories={data.categories} />
                  </div>
                </div>
              </Panel>
      ),
  };


  return (
    <div className="flex h-full min-w-0 flex-col">
      <Header
        data={data}
        months={months}
        onMonths={setMonths}
        onReload={reload}
        reloading={reloading}
        justReloaded={reloadedAt !== null}
        reloadError={reloadError}
        kpiKeys={kpiKeys}
        hiddenCards={hiddenCards}
        onPrefs={(patch) => void persistPrefs(patch)}
      />
      {plan.data?.has_goals ? <PlanStrip plan={plan.data} /> : null}
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 pb-4">
        <div ref={gridRef} className="w-full min-w-0">
          {/*
            `key={width}` remounts the grid whenever the measured width changes:
            react-grid-layout only lays items out from the width it saw on its
            first render, so an in-place width update would leave the items sized
            for the previous window width and overflow the container.
          */}
          <GridLayout
            key={width}
            width={Math.max(320, width)}
            layout={visibleLayout}
            gridConfig={{ ...GRID, rowHeight: compact ? 38 : 46 }}
            dragConfig={{ enabled: true, handle: ".drag-handle" }}
            resizeConfig={{ enabled: true }}
            autoSize
            onLayoutChange={saveLayout}
          >
            {visibleLayout.map((item) => (
              <div key={item.i} className="h-full">
                {panels[item.i] ?? null}
              </div>
            ))}
          </GridLayout>
        </div>
      </div>
    </div>
  );
}

function PlanStrip({ plan }: { plan: PlanOverviewDto_Serialize }) {
  const fire = plan.fire;
  const reserve = plan.reserve;
  const nextDebt = plan.debts.find((debt) => debt.payoff_date) ?? null;
  const fireProgress = Math.max(0, Math.min(100, fire?.progress ?? 0));
  const reserveProgress = Number(reserve?.target_months ?? 0) > 0
    ? Math.max(0, Math.min(100, (Number(reserve?.months ?? 0) / Number(reserve?.target_months ?? 0)) * 100))
    : 0;
  return (
    <a
      href="#/plan"
      className="mx-4 mb-2 grid gap-3 rounded-[var(--radius-card)] border border-[var(--color-line)] bg-[var(--color-surface)] p-3 text-xs transition hover:border-[var(--color-accent)]/60 sm:grid-cols-3"
    >
      <div>
        <div className="flex items-center justify-between">
          <span className="text-[var(--color-muted)]">财富自由</span>
          <span className="font-medium tabular-nums">{pct(Number(fire?.progress ?? 0))}</span>
        </div>
        <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-[var(--color-line)]">
          <div className="h-full rounded-full bg-[var(--color-accent)]" style={{ width: `${fireProgress}%` }} />
        </div>
        <div className="mt-1 text-[11px] text-[var(--color-muted)]">
          {fire?.targets?.[0]
            ? `${money(fire.balance.value, fire.balance.display)} / ${money(fire.targets[0].target.value, fire.targets[0].target.display)} · ${fire.targets[0].eta_label}`
            : "未设置目标"}
        </div>
      </div>
      <div>
        <div className="flex items-center justify-between">
          <span className="text-[var(--color-muted)]">储备月数</span>
          <span className="font-medium tabular-nums">{Number(reserve?.months ?? 0).toFixed(1)} 个月</span>
        </div>
        <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-[var(--color-line)]">
          <div
            className={cn(
              "h-full rounded-full",
              reserveProgress < 100 ? "bg-[var(--color-danger)]" : "bg-[var(--color-accent)]",
            )}
            style={{ width: `${reserveProgress}%` }}
          />
        </div>
        <div className="mt-1 text-[11px] text-[var(--color-muted)]">
          目标 {reserve?.target_months ?? 0} 个月 · 现金 {money(reserve?.cash?.value, reserve?.cash?.display)}
        </div>
      </div>
      <div>
        <div className="flex items-center justify-between">
          <span className="text-[var(--color-muted)]">{nextDebt ? "下一笔还清" : "目标"}</span>
          <span className="font-medium tabular-nums">{nextDebt?.payoff_date ?? `${plan.goals.length} 个`}</span>
        </div>
        <div className="mt-1 text-[11px] text-[var(--color-muted)]">
          {nextDebt
            ? `${nextDebt.label} · 余额 ${money(nextDebt.balance.value, nextDebt.balance.display)}`
            : plan.goals[0]
              ? `${plan.goals[0].label} ${pct(Number(plan.goals[0].progress ?? 0))}`
              : "点开规划设置目标"}
        </div>
      </div>
    </a>
  );
}

function Header({
  data,
  months,
  onMonths,
  onReload,
  reloading,
  justReloaded,
  reloadError,
  kpiKeys,
  hiddenCards,
  onPrefs,
}: {
  data: DashboardDto_Serialize;
  months: number;
  onMonths: (next: number) => void;
  onReload: () => void;
  reloading: boolean;
  justReloaded: boolean;
  reloadError: string | null;
  kpiKeys: string[];
  hiddenCards: string[];
  onPrefs: (patch: { hidden_cards?: string[]; kpi_cards?: string[] }) => void;
}) {
  const kpi = data.kpis;
  const [customize, setCustomize] = React.useState(false);
  const shows = (id: string) => !hiddenCards.includes(id);
  return (
    <div className="space-y-3 px-4 pb-3 pt-4">
      <div className="flex items-center justify-between">
        <div>
          <div className="text-lg font-semibold">看板</div>
          <div className="text-xs text-[var(--color-muted)]">
            {kpi.month ? monthLabel(kpi.month) : "尚无交易"}
            {(kpi.missing_fx_count ?? 0) > 0 ? ` · ${kpi.missing_fx_count} 笔缺少汇率` : ""}
          </div>
        </div>
        <div className="flex items-center gap-2">
          {reloadError ? (
            <span className="max-w-[22rem] truncate text-xs text-[var(--color-danger)]" title={reloadError}>
              刷新失败：{reloadError}
            </span>
          ) : justReloaded ? (
            <span className="text-xs text-[var(--color-muted)]">已刷新</span>
          ) : null}
          <Button
            variant="ghost"
            onClick={onReload}
            disabled={reloading}
            title="重新读取账本文件（外部改动也会自动检测）"
            aria-label="刷新账本"
          >
            <RefreshCw className={cn("mr-1 h-3.5 w-3.5", reloading && "animate-spin")} />
            {reloading ? "刷新中" : "刷新"}
          </Button>
          <Button
            variant="ghost"
            onClick={() => onMonths(months === 12 ? 24 : 12)}
            title="切换统计窗口"
          >
            {months === 12 ? "近 12 月" : "近 24 月"}
          </Button>
          <Button variant="ghost" onClick={() => setCustomize(true)} title="选择卡片与指标">
            自定义
          </Button>
          <Button variant="subtle" onClick={() => void api.showCapture()}>
            记一笔
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2 lg:grid-cols-5">
        {kpiKeys.includes("net_worth") ? (
          <Kpi label="净资产" value={money(kpi.net_worth.value, kpi.net_worth.display)} />
        ) : null}
        {kpiKeys.includes("month_income") ? (
          <Kpi
            label="本月收入"
            value={money(kpi.month_income.value, kpi.month_income.display)}
            delta={kpi.income_change}
          />
        ) : null}
        {kpiKeys.includes("month_expense") ? (
          <Kpi
            label="本月支出"
            value={money(kpi.month_expense.value, kpi.month_expense.display)}
            delta={kpi.expense_change}
            invertDelta
          />
        ) : null}
        {kpiKeys.includes("savings_rate") ? (
          <Kpi label="储蓄率" value={percent(kpi.savings_rate)} />
        ) : null}
        {kpiKeys.includes("missing_fx") ? (
          <Kpi
            label="缺少汇率"
            value={String(kpi.missing_fx_count ?? 0)}
            tone={(kpi.missing_fx_count ?? 0) > 0 ? "warn" : "neutral"}
          />
        ) : null}
      </div>

      <Modal open={customize} onClose={() => setCustomize(false)} title="自定义看板" width="max-w-md">
        <div className="space-y-3">
          <div>
            <div className="mb-1 text-xs font-medium">卡片</div>
            <div className="grid grid-cols-2 gap-1.5">
              {CARD_KEYS.map((card) => (
                <label key={card.key} className="flex items-center gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={shows(card.key)}
                    onChange={(event) => {
                      const next = event.target.checked
                        ? hiddenCards.filter((id) => id !== card.key)
                        : [...hiddenCards, card.key];
                      onPrefs({ hidden_cards: next });
                    }}
                    aria-label={`显示${card.label}`}
                  />
                  {card.label}
                </label>
              ))}
            </div>
          </div>
          <div>
            <div className="mb-1 text-xs font-medium">顶部指标</div>
            <div className="grid grid-cols-2 gap-1.5">
              {KPI_KEYS.map((entry) => (
                <label key={entry.key} className="flex items-center gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={kpiKeys.includes(entry.key)}
                    onChange={(event) => {
                      const current = kpiKeys.filter((key) => key !== entry.key);
                      const next = event.target.checked ? [...current, entry.key] : current;
                      onPrefs({ kpi_cards: next });
                    }}
                    aria-label={`显示${entry.label}`}
                  />
                  {entry.label}
                </label>
              ))}
            </div>
          </div>
          <div className="flex justify-end">
            <Button variant="primary" onClick={() => setCustomize(false)}>
              完成
            </Button>
          </div>
        </div>
      </Modal>

      {data.fx_missing.length > 0 ? (
        <div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-1.5 text-xs text-amber-700 dark:text-amber-300">
          <Badge tone="warn">未折算</Badge>
          <span className="truncate">
            {data.fx_missing.length} 笔交易缺少汇率，未计入合计：
            {data.fx_missing
              .slice(0, 3)
              .map((txn) => txn.payee || txn.id)
              .join("、")}
          </span>
        </div>
      ) : null}
    </div>
  );
}

function Kpi({
  label,
  value,
  delta,
  invertDelta,
  tone = "neutral",
}: {
  label: string;
  value: string;
  delta?: number | null;
  invertDelta?: boolean;
  tone?: "neutral" | "warn";
}) {
  const deltaTone =
    delta === null || delta === undefined
      ? ""
      : (invertDelta ? delta > 0 : delta < 0)
        ? "text-[var(--color-danger)]"
        : "text-[var(--color-accent)]";
  return (
    <Card className={cn("p-3", tone === "warn" && "border-amber-500/40")}>
      <div className="text-[11px] text-[var(--color-muted)]">{label}</div>
      <div className="mt-1 truncate font-mono text-base font-medium">{value}</div>
      {delta !== null && delta !== undefined ? (
        <div className={cn("text-[11px]", deltaTone)}>{percent(delta)} 环比</div>
      ) : null}
    </Card>
  );
}

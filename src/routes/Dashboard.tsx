import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { GridLayout } from "react-grid-layout";
import type { Layout } from "react-grid-layout";
import type { DashboardDto_Serialize } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { useElementWidth } from "../lib/use-element-width";
import { cn, money, monthLabel, percent } from "../lib/format";
import { Badge, Button, Card, CardHeader, EmptyState } from "../components/ui";
import {
  BudgetBars,
  CategoryDonut,
  CategoryList,
  ForecastLine,
  Heatmap,
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
  { i: "sankey", x: 0, y: 10, w: 8, h: 5, minW: 4, minH: 3 },
  { i: "heatmap", x: 8, y: 10, w: 4, h: 5, minW: 3, minH: 3 },
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
      return Array.isArray(parsed) && parsed.length > 0 ? parsed : DEFAULT_LAYOUT;
    } catch {
      return DEFAULT_LAYOUT;
    }
  }, [settings.data?.dashboard_layout]);

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
      />
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
            layout={layout}
            gridConfig={GRID}
            dragConfig={{ enabled: true, handle: ".drag-handle" }}
            resizeConfig={{ enabled: true }}
            autoSize
            onLayoutChange={saveLayout}
          >
            <div key="months">
              <Panel title="月度收支" hint={`最近 ${months} 个月`}>
                <MonthlyBars months={data.months} />
              </Panel>
            </div>
            <div key="categories">
              <Panel title="本月支出结构" hint={data.kpis.month ? monthLabel(data.kpis.month) : ""}>
                <CategoryDonut categories={data.categories} />
              </Panel>
            </div>
            <div key="networth">
              <Panel title="净资产曲线" hint="含所有资产与负债">
                <NetWorthArea points={data.net_worth_series} />
              </Panel>
            </div>
            <div key="budget">
              <Panel title="预算 vs 实际" hint="来自 budgets.ledger">
                <BudgetBars rows={data.budget} />
              </Panel>
            </div>
            <div key="stacked">
              <Panel title="分类趋势" hint="按二级分类堆叠">
                <StackedArea stacked={data.stacked} />
              </Panel>
            </div>
            <div key="forecast">
              <Panel title="现金流预测" hint="虚线段为预测，不写入账本">
                <ForecastLine points={data.forecast} />
              </Panel>
            </div>
            <div key="sankey">
              <Panel title="资金流向" hint="收入 → 账户 → 支出分类">
                <SankeyDiagram data={data.sankey} />
              </Panel>
            </div>
            <div key="heatmap">
              <Panel title="支出日历" hint="颜色越深支出越多">
                <div className="flex h-full flex-col justify-between">
                  <Heatmap points={data.heatmap} />
                  <div className="max-h-24 overflow-auto">
                    <CategoryList categories={data.categories} />
                  </div>
                </div>
              </Panel>
            </div>
          </GridLayout>
        </div>
      </div>
    </div>
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
}: {
  data: DashboardDto_Serialize;
  months: number;
  onMonths: (next: number) => void;
  onReload: () => void;
  reloading: boolean;
  justReloaded: boolean;
  reloadError: string | null;
}) {
  const kpi = data.kpis;
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
          <Button variant="subtle" onClick={() => void api.showCapture()}>
            记一笔
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2 lg:grid-cols-5">
        <Kpi label="净资产" value={money(kpi.net_worth.value, kpi.net_worth.display)} />
        <Kpi
          label="本月收入"
          value={money(kpi.month_income.value, kpi.month_income.display)}
          delta={kpi.income_change}
        />
        <Kpi
          label="本月支出"
          value={money(kpi.month_expense.value, kpi.month_expense.display)}
          delta={kpi.expense_change}
          invertDelta
        />
        <Kpi label="储蓄率" value={percent(kpi.savings_rate)} />
        <Kpi
          label="缺少汇率"
          value={String(kpi.missing_fx_count ?? 0)}
          tone={(kpi.missing_fx_count ?? 0) > 0 ? "warn" : "neutral"}
        />
      </div>

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

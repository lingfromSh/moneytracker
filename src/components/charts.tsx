import * as React from "react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Line,
  ComposedChart,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { sankey, sankeyLinkHorizontal, sankeyJustify } from "d3-sankey";
import type {
  BudgetRowDto,
  CategoryTotalDto,
  ForecastPointDto,
  HeatPointDto,
  MonthPointDto,
  PointDto,
  SankeyDto,
  StackedPointDto,
} from "../lib/bindings";
import { cn, dayLabel, money, monthLabel, palette, percent } from "../lib/format";

const AXIS = { fontSize: 11, fill: "currentColor", opacity: 0.6 } as const;
const GRID = "color-mix(in oklab, currentColor 12%, transparent)";

const tooltipStyle = {
  background: "var(--color-surface)",
  border: "1px solid var(--color-line)",
  borderRadius: 10,
  fontSize: 12,
  color: "var(--color-ink)",
};

export function MonthlyBars({ months }: { months: MonthPointDto[] }) {
  const data = months.map((m) => ({
    label: monthLabel(m.month),
    income: m.income ?? 0,
    expense: m.expense ?? 0,
  }));
  return (
    <ResponsiveContainer width="100%" height="100%">
      <BarChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
        <CartesianGrid stroke={GRID} vertical={false} />
        <XAxis dataKey="label" tick={AXIS} axisLine={false} tickLine={false} />
        <YAxis tick={AXIS} axisLine={false} tickLine={false} width={48} />
        <Tooltip contentStyle={tooltipStyle} formatter={(v) => money(Number(v))} />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        <Bar dataKey="income" name="收入" fill="#3f9e6a" radius={[4, 4, 0, 0]} />
        <Bar dataKey="expense" name="支出" fill="#d98b3f" radius={[4, 4, 0, 0]} />
      </BarChart>
    </ResponsiveContainer>
  );
}

export function NetWorthArea({ points }: { points: PointDto[] }) {
  return (
    <ResponsiveContainer width="100%" height="100%">
      <AreaChart data={points} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
        <defs>
          <linearGradient id="netWorthFill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#3f9e6a" stopOpacity={0.35} />
            <stop offset="100%" stopColor="#3f9e6a" stopOpacity={0.02} />
          </linearGradient>
        </defs>
        <CartesianGrid stroke={GRID} vertical={false} />
        <XAxis dataKey="label" tick={AXIS} axisLine={false} tickLine={false} />
        <YAxis tick={AXIS} axisLine={false} tickLine={false} width={56} />
        <Tooltip contentStyle={tooltipStyle} formatter={(v) => money(Number(v))} />
        <Area
          type="monotone"
          dataKey="value"
          name="净资产"
          stroke="#3f9e6a"
          strokeWidth={2}
          fill="url(#netWorthFill)"
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}

export function CategoryDonut({ categories }: { categories: CategoryTotalDto[] }) {
  const data = categories.slice(0, 8).map((c, i) => ({
    name: c.label,
    value: Math.abs(c.value ?? 0),
    display: c.display,
    fill: palette(i),
  }));
  if (data.length === 0) {
    return <div className="grid h-full place-items-center text-xs text-[var(--color-muted)]">本月还没有支出</div>;
  }
  return (
    <ResponsiveContainer width="100%" height="100%">
      <PieChart>
        <Pie data={data} dataKey="value" nameKey="name" innerRadius="55%" outerRadius="82%" paddingAngle={1}>
          {data.map((entry) => (
            <Cell key={entry.name} fill={entry.fill} />
          ))}
        </Pie>
        <Tooltip contentStyle={tooltipStyle} formatter={(v) => money(Number(v))} />
        <Legend wrapperStyle={{ fontSize: 11 }} />
      </PieChart>
    </ResponsiveContainer>
  );
}

export function StackedArea({ stacked }: { stacked: StackedPointDto[] }) {
  const labels = Array.from(
    new Set(stacked.flatMap((point) => point.values.map((value) => value.label))),
  );
  const data = stacked.map((point) => {
    const row: Record<string, string | number> = { month: monthLabel(point.month) };
    for (const value of point.values) row[value.label] = value.value ?? 0;
    return row;
  });
  return (
    <ResponsiveContainer width="100%" height="100%">
      <AreaChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
        <CartesianGrid stroke={GRID} vertical={false} />
        <XAxis dataKey="month" tick={AXIS} axisLine={false} tickLine={false} />
        <YAxis tick={AXIS} axisLine={false} tickLine={false} width={48} />
        <Tooltip contentStyle={tooltipStyle} formatter={(v) => money(Number(v))} />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        {labels.map((label, index) => (
          <Area
            key={label}
            type="monotone"
            dataKey={label}
            stackId="1"
            stroke={palette(index)}
            fill={palette(index)}
            fillOpacity={0.35}
          />
        ))}
      </AreaChart>
    </ResponsiveContainer>
  );
}

export function Heatmap({ points }: { points: HeatPointDto[] }) {
  const byDate = new Map(points.map((p) => [p.date, p]));
  const days = points.length > 0 ? points[points.length - 1].date : "";
  const base = days ? new Date(`${days}T00:00:00`) : new Date();
  const cells: { date: string; value: number; display: string }[] = [];
  for (let offset = 119; offset >= 0; offset -= 1) {
    const day = new Date(base);
    day.setDate(day.getDate() - offset);
    const iso = day.toISOString().slice(0, 10);
    const hit = byDate.get(iso);
    cells.push({ date: iso, value: hit?.value ?? 0, display: hit?.display ?? "" });
  }
  const max = Math.max(1, ...cells.map((c) => c.value));
  return (
    <div className="grid grid-flow-col grid-rows-7 gap-[3px] p-2" role="img" aria-label="支出日历热力图">
      {cells.map((cell) => {
        const intensity = cell.value / max;
        return (
          <div
            key={cell.date}
            title={`${cell.date} ${cell.display || "无支出"}`}
            className="h-3 w-3 rounded-[3px]"
            style={{
              background:
                cell.value > 0
                  ? `color-mix(in oklab, var(--color-accent) ${Math.max(12, intensity * 100)}%, transparent)`
                  : "var(--color-line)",
            }}
          />
        );
      })}
    </div>
  );
}

export function BudgetBars({ rows }: { rows: BudgetRowDto[] }) {
  if (rows.length === 0) {
    return (
      <div className="grid h-full place-items-center px-4 text-center text-xs text-[var(--color-muted)]">
        <p>
          还没有预算。在 <code>budgets.ledger</code> 里加一段 <code>~ monthly</code> 交易：支出账户是花钱预算，
          <code>assets:</code> 账户是存钱目标。
        </p>
      </div>
    );
  }
  return (
    <div className="space-y-2 overflow-auto p-3">
      {rows.map((row) => {
        const saving = row.kind === "A" || row.kind === "L";
        const over = (row.ratio ?? 0) > 1;
        return (
          <div key={row.account} className="space-y-1">
            <div className="flex items-baseline justify-between gap-2 text-xs">
              <span className="truncate">
                {row.label}
                {saving ? (
                  <span className="ml-1 rounded px-1 text-[10px] text-[var(--color-muted)] ring-1 ring-[var(--color-line)]">
                    储蓄
                  </span>
                ) : null}
              </span>
              <span className={cn(over ? "text-[var(--color-danger)]" : "text-[var(--color-muted)]")}>
                {row.actual_display} / {row.budget_display}
              </span>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-[var(--color-line)]">
              <div
                className="h-full rounded-full"
                style={{
                  width: `${Math.min(100, (row.ratio ?? 0) * 100)}%`,
                  background: over ? "var(--color-danger)" : "var(--color-accent)",
                }}
              />
            </div>
            <div className="text-[10px] text-[var(--color-muted)]">
              {saving ? "已存" : "已花"} {row.actual_display}，{saving ? "还差" : "剩余"} {row.remaining_display}
            </div>
          </div>
        );
      })}
    </div>
  );
}

export function ForecastLine({ points }: { points: ForecastPointDto[] }) {
  const data = points.map((p) => ({
    label: monthLabel(p.month),
    actual: p.label ? null : p.actual,
    forecast: (p.forecast ?? 0) > 0 ? p.forecast : null,
  }));
  return (
    <ResponsiveContainer width="100%" height="100%">
      <ComposedChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
        <CartesianGrid stroke={GRID} vertical={false} />
        <XAxis dataKey="label" tick={AXIS} axisLine={false} tickLine={false} />
        <YAxis tick={AXIS} axisLine={false} tickLine={false} width={48} />
        <Tooltip contentStyle={tooltipStyle} formatter={(v) => money(Number(v))} />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        <Bar dataKey="actual" name="实际" fill="#3f9e6a" radius={[4, 4, 0, 0]} />
        <Line
          type="monotone"
          dataKey="forecast"
          name="预测"
          stroke="#8a7bd8"
          strokeDasharray="5 4"
          strokeWidth={2}
          dot={false}
        />
      </ComposedChart>
    </ResponsiveContainer>
  );
}

type SankeyNode = { name: string; layer: string };
type SankeyGraphLink = { source: number; target: number; value: number };
type LayoutNode = SankeyNode & { x0: number; x1: number; y0: number; y1: number };
type LayoutLink = { source: LayoutNode; target: LayoutNode; width: number; value: number };

export function SankeyDiagram({ data }: { data: SankeyDto }) {
  const width = 640;
  const height = 260;
  const model = React.useMemo(() => {
    if (data.links.length === 0) return null;
    const index = new Map(data.nodes.map((node, i) => [node.id, i]));
    const nodes: SankeyNode[] = data.nodes.map((node) => ({ name: node.label, layer: node.layer }));
    const links: SankeyGraphLink[] = data.links
      .map((link) => ({
        source: index.get(link.source) ?? -1,
        target: index.get(link.target) ?? -1,
        value: Math.abs(link.value ?? 0),
      }))
      .filter((link) => link.source >= 0 && link.target >= 0 && link.value > 0);
    if (links.length === 0) return null;
    try {
      const layout = sankey<SankeyNode, SankeyGraphLink>()
        .nodeAlign(sankeyJustify)
        .nodeWidth(12)
        .nodePadding(10)
        .extent([
          [2, 4],
          [width - 2, height - 4],
        ]);
      return layout({ nodes, links }) as unknown as {
        nodes: LayoutNode[];
        links: LayoutLink[];
      };
    } catch {
      return null;
    }
  }, [data]);

  if (!model) {
    return (
      <div className="grid h-full place-items-center px-4 text-center text-xs text-[var(--color-muted)]">
        本月还没有足够的收支数据来绘制桑基图。
      </div>
    );
  }

  return (
    <svg viewBox={`0 0 ${width} ${height}`} className="h-full w-full">
      <g fill="none" strokeOpacity={0.35}>
        {model.links.map((link, index) => (
          <path
            key={index}
            d={sankeyLinkHorizontal()(link as never) ?? undefined}
            stroke={palette(index)}
            strokeWidth={Math.max(1, link.width)}
          />
        ))}
      </g>
      <g>
        {model.nodes.map((node, index) => (
          <g key={`${node.name}-${index}`}>
            <rect
              x={node.x0}
              y={node.y0}
              width={Math.max(1, node.x1 - node.x0)}
              height={Math.max(1, node.y1 - node.y0)}
              rx={2}
              fill={palette(index)}
            />
            <text
              x={node.x0 < width / 2 ? node.x1 + 4 : node.x0 - 4}
              y={(node.y0 + node.y1) / 2}
              textAnchor={node.x0 < width / 2 ? "start" : "end"}
              dominantBaseline="middle"
              fontSize={10}
              fill="currentColor"
              opacity={0.75}
            >
              {node.name}
            </text>
          </g>
        ))}
      </g>
    </svg>
  );
}

export function CategoryList({ categories }: { categories: CategoryTotalDto[] }) {
  if (categories.length === 0) {
    return <div className="p-3 text-xs text-[var(--color-muted)]">本月还没有支出。</div>;
  }
  return (
    <div className="space-y-1 overflow-auto p-3">
      {categories.map((category, index) => (
        <div key={category.account} className="flex items-center gap-2 text-xs">
          <span className="h-2 w-2 rounded-full" style={{ background: palette(index) }} />
          <span className="min-w-0 flex-1 truncate">{category.label}</span>
          <span className="text-[var(--color-muted)]">{percent(category.share ?? 0)}</span>
          <span className="w-24 text-right font-mono">{category.display}</span>
        </div>
      ))}
    </div>
  );
}

export function HeatmapLegend() {
  return (
    <div className="flex items-center gap-1 px-3 pb-2 text-[10px] text-[var(--color-muted)]">
      <span>{dayLabel(new Date().toISOString().slice(0, 10))}</span>
      <span className="ml-auto">近 17 周支出</span>
    </div>
  );
}

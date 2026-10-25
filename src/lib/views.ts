/** Saved transaction views. Stored in settings as a JSON blob. */
export type SavedView = {
  name: string;
  query: string;
  order?: string;
};

export function parseViews(raw: string | null | undefined): SavedView[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((item): item is Record<string, unknown> => typeof item === "object" && item !== null)
      .map((item) => ({
        name: String(item.name ?? "").trim(),
        query: String(item.query ?? "").trim(),
        order: item.order ? String(item.order) : undefined,
      }))
      .filter((view) => view.name.length > 0);
  } catch {
    return [];
  }
}

export function serializeViews(views: SavedView[]): string {
  return JSON.stringify(views);
}

export function addView(views: SavedView[], view: SavedView): SavedView[] {
  const name = view.name.trim();
  if (name.length === 0) return views;
  const without = views.filter((existing) => existing.name !== name);
  return [...without, { ...view, name }];
}

export function removeView(views: SavedView[], name: string): SavedView[] {
  return views.filter((view) => view.name !== name);
}

export const KPI_KEYS = [
  { key: "net_worth", label: "净资产" },
  { key: "month_income", label: "本月收入" },
  { key: "month_expense", label: "本月支出" },
  { key: "savings_rate", label: "储蓄率" },
  { key: "missing_fx", label: "缺少汇率" },
] as const;

export const CARD_KEYS = [
  { key: "months", label: "月度收支" },
  { key: "categories", label: "本月支出结构" },
  { key: "networth", label: "净资产曲线" },
  { key: "budget", label: "预算 vs 实际" },
  { key: "schedule", label: "固定收支" },
  { key: "stacked", label: "分类趋势" },
  { key: "forecast", label: "现金流预测" },
  { key: "sankey", label: "资金流向" },
  { key: "heatmap", label: "支出日历" },
  { key: "insights", label: "洞察" },
] as const;

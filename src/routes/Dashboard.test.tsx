import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DashboardDto_Serialize, Settings_Serialize } from "../lib/bindings";

const dashboardMock = vi.fn();
const settingsMock = vi.fn();

vi.mock("../lib/ipc", () => ({
  api: {
    dashboard: (months: number) => dashboardMock(months),
    settings: () => settingsMock(),
    saveSettings: vi.fn(),
    showCapture: vi.fn(),
  },
  queryKeys: {
    dashboard: (months: number) => ["dashboard", months],
    settings: ["settings"],
  },
}));

const { Dashboard } = await import("./Dashboard");

const amount = (display: string, value: number) => ({
  quantity: String(value),
  value,
  commodity: "CNY",
  display,
});

const fixture: DashboardDto_Serialize = {
  kpis: {
    net_worth: amount("16,800.00 CNY", 16800),
    month_income: amount("10,000.00 CNY", 10000),
    month_expense: amount("200.00 CNY", 200),
    savings_rate: 0.98,
    income_change: 0.25,
    expense_change: -0.1,
    month: "2026-10",
    missing_fx_count: 1,
  },
  months: [
    {
      month: "2026-09",
      income: 10000,
      expense: 3200,
      net: 6800,
      income_display: "10,000.00 CNY",
      expense_display: "3,200.00 CNY",
      net_display: "6,800.00 CNY",
    },
    {
      month: "2026-10",
      income: 10000,
      expense: 200,
      net: 9800,
      income_display: "10,000.00 CNY",
      expense_display: "200.00 CNY",
      net_display: "9,800.00 CNY",
    },
  ],
  net_worth_series: [{ label: "2026-10", value: 16800, display: "16,800.00 CNY" }],
  categories: [
    {
      account: "expenses:food:coffee",
      label: "expenses:food",
      value: 200,
      display: "200.00 CNY",
      share: 1,
      txn_count: 2,
    },
  ],
  stacked: [
    {
      month: "2026-10",
      values: [{ label: "food", value: 200, display: "200.00 CNY" }],
    },
  ],
  heatmap: [{ date: "2026-10-24", value: 200, display: "200.00 CNY" }],
  budget: [
    {
      account: "expenses:food",
      label: "food",
      budget: 500,
      actual: 200,
      budget_display: "500.00 CNY",
      actual_display: "200.00 CNY",
      remaining_display: "300.00 CNY",
      ratio: 0.4,
    },
  ],
  forecast: [
    { month: "2026-10", actual: 200, forecast: null, label: "" },
    { month: "2026-11", actual: 0, forecast: 260, label: "预测" },
  ],
  sankey: {
    nodes: [
      { id: "income:工资", label: "工资", layer: "income" },
      { id: "assets:cash", label: "现金", layer: "account" },
      { id: "expenses:food", label: "food", layer: "expense" },
    ],
    links: [
      { source: "income:工资", target: "assets:cash", value: 10000 },
      { source: "assets:cash", target: "expenses:food", value: 200 },
    ],
    period: "2026-09 → 2026-10",
  },
  fx_missing: [
    {
      id: "x-1",
      date: "2026-10-24",
      date2: null,
      status: null,
      code: null,
      payee: "酒店",
      comment: null,
      tags: [],
      postings: [],
      file: "2026/2026-10-24.ledger",
      span_start: 0,
      span_end: 0,
      base_value: null,
      base_value_display: null,
      balanced: true,
      fx_missing: true,
      editable: true,
    },
  ],
};

function renderDashboard() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <Dashboard />
    </QueryClientProvider>,
  );
}

describe("Dashboard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    dashboardMock.mockResolvedValue(fixture);
    settingsMock.mockResolvedValue({
      vault_path: "/tmp/vault",
      recent_vaults: [],
      theme: "system",
      shortcut: "CmdOrCtrl+Shift+Space",
      base_commodity: "CNY",
      base_precision: 2,
      default_funding: "assets:cash",
      default_unclassified: "expenses:unclassified",
      fx_enabled: false,
      max_backups: 200,
      dashboard_layout: null,
    } satisfies Settings_Serialize);
  });

  it("renders the KPI cards from the snapshot", async () => {
    renderDashboard();
    expect((await screen.findAllByText("16,800.00 CNY")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("10,000.00 CNY").length).toBeGreaterThan(0);
    expect(screen.getAllByText("200.00 CNY").length).toBeGreaterThan(0);
    expect(screen.getByText("98.0%")).toBeInTheDocument();
    expect(screen.getByText("25.0% 环比")).toBeInTheDocument();
  });

  it("flags missing exchange rates instead of silently converting", async () => {
    renderDashboard();
    expect(await screen.findByText("未折算")).toBeInTheDocument();
    expect(screen.getByText(/1 笔交易缺少汇率/)).toBeInTheDocument();
    expect(screen.getByText(/酒店/)).toBeInTheDocument();
  });

  it("labels the forecast panel as a projection", async () => {
    renderDashboard();
    expect(await screen.findByText("现金流预测")).toBeInTheDocument();
    expect(screen.getByText("虚线段为预测，不写入账本")).toBeInTheDocument();
    expect(screen.getByText("资金流向")).toBeInTheDocument();
    expect(screen.getByText("预算 vs 实际")).toBeInTheDocument();
  });

  it("requests the selected window size", async () => {
    renderDashboard();
    await screen.findByText("16,800.00 CNY");
    expect(dashboardMock).toHaveBeenCalledWith(12);
  });
});

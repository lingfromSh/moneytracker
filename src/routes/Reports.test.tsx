import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  BalanceSheetDto_Serialize,
  BridgeDto,
  IncomeStatementDto_Serialize,
  PeriodDto,
  SectionDto_Serialize,
} from "../lib/bindings";

const periodsMock = vi.fn();
const incomeMock = vi.fn();
const balanceMock = vi.fn();
const bridgeMock = vi.fn();

vi.mock("../lib/ipc", () => ({
  api: {
    reportPeriods: () => periodsMock(),
    statementIncome: (from: string, to: string) => incomeMock(from, to),
    statementBalance: (asOf: string | null) => balanceMock(asOf),
    statementBridge: (from: string, to: string) => bridgeMock(from, to),
    reportMarkdown: vi.fn(),
  },
  queryKeys: {
    periods: ["periods"],
    incomeStatement: (from: string, to: string) => ["income", from, to],
    balanceSheet: (asOf: string | null) => ["balance", asOf],
    bridge: (from: string, to: string) => ["bridge", from, to],
  },
}));

const { Reports } = await import("./Reports");

const amount = (display: string, value: number) => ({
  quantity: String(value),
  value,
  commodity: "CNY",
  display,
});

const section = (title: string, rows: [string, string][], total: string): SectionDto_Serialize => ({
  title,
  rows: rows.map(([account, display]) => ({
    account,
    label: account.split(":").pop() ?? account,
    unit: amount(display, Number(display.replace(/[^0-9.-]/g, ""))),
    hint: null,
  })),
  total: amount(total, Number(total.replace(/[^0-9.-]/g, ""))),
});

const periods: PeriodDto[] = [
  { key: "2026-10", from: "2026-10-01", to: "2026-10-31", label: "2026年10月" },
  { key: "2026-Q3", from: "2026-07-01", to: "2026-09-30", label: "2026年Q3" },
];

const income: IncomeStatementDto_Serialize = {
  from: "2026-10-01",
  to: "2026-10-31",
  income: section("收入", [["income:salary", "12,000.00 CNY"]], "12,000.00 CNY"),
  expenses: section("支出", [["expenses:food", "3,200.00 CNY"]], "3,200.00 CNY"),
  net: amount("8,800.00 CNY", 8800),
};

const balance: BalanceSheetDto_Serialize = {
  as_of: "2026-10-31",
  assets: section("资产", [["assets:cash", "2,000.00 CNY"]], "2,000.00 CNY"),
  liabilities: section("负债", [["liabilities:card", "-500.00 CNY"]], "-500.00 CNY"),
  equity: section("权益", [["equity:opening", "1,000.00 CNY"]], "1,000.00 CNY"),
  income: section("累计收入", [], "0.00 CNY"),
  expense: section("累计支出", [], "0.00 CNY"),
  cumulative_net: amount("1,500.00 CNY", 1500),
  net_worth: amount("1,500.00 CNY", 1500),
  note: null,
};

const bridge: BridgeDto = {
  from: "2026-10-01",
  to: "2026-10-31",
  opening: amount("1,000.00 CNY", 1000),
  income: amount("8,800.00 CNY", 8800),
  expense: amount("8,300.00 CNY", 8300),
  valuation: amount("0.00 CNY", 0),
  closing: amount("1,500.00 CNY", 1500),
  reconciled: true,
};

function wrap(node: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0, staleTime: 0 } } });
  return render(<QueryClientProvider client={client}>{node}</QueryClientProvider>);
}

describe("Reports", () => {
  beforeEach(() => {
    periodsMock.mockReset().mockResolvedValue(periods);
    incomeMock.mockReset().mockResolvedValue(income);
    balanceMock.mockReset().mockResolvedValue(balance);
    bridgeMock.mockReset().mockResolvedValue(bridge);
  });

  it("shows the income statement for the newest period", async () => {
    wrap(<Reports />);
    expect(await screen.findByText(/2026年10月 · 2026-10-01 → 2026-10-31/)).toBeInTheDocument();
    expect(await screen.findByText("本期结余 8,800.00 CNY")).toBeInTheDocument();
    expect(screen.getByText("收入")).toBeInTheDocument();
    expect(screen.getByText("支出")).toBeInTheDocument();
    expect(incomeMock).toHaveBeenCalledWith("2026-10-01", "2026-10-31");
  });

  it("renders the balance sheet with the double-entry identity", async () => {
    wrap(<Reports />);
    await userEvent.click(await screen.findByRole("button", { name: "资产负债表" }));
    expect(await screen.findByText("净资产 1,500.00 CNY")).toBeInTheDocument();
    expect(screen.getByText(/资产 − 负债 = 权益 \+ 累计结余/)).toBeInTheDocument();
    expect(balanceMock).toHaveBeenCalledWith("2026-10-31");
  });

  it("explains the net-worth bridge", async () => {
    wrap(<Reports />);
    await userEvent.click(await screen.findByRole("button", { name: "净值变动" }));
    expect(await screen.findByText("已对齐")).toBeInTheDocument();
    expect(screen.getByText("± 汇率/估值差")).toBeInTheDocument();
    expect(screen.getByText("= 期末净资产")).toBeInTheDocument();
  });
});

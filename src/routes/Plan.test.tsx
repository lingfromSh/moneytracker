import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PlanOverviewDto_Serialize } from "../lib/bindings";

const planMock = vi.fn();
const saveMock = vi.fn();

vi.mock("../lib/ipc", () => ({
  api: {
    planOverview: () => planMock(),
    goalsSave: (text: string) => saveMock(text),
  },
  queryKeys: {
    plan: ["plan"],
    settings: ["settings"],
  },
}));

const { Plan } = await import("./Plan");

const amount = (display: string, value: number) => ({
  quantity: String(value),
  value,
  commodity: "CNY",
  display,
});

function overview(overrides: Partial<PlanOverviewDto_Serialize> = {}): PlanOverviewDto_Serialize {
  return {
    has_goals: true,
    goals_text: "goal fire 财富自由 ; swr:4%",
    assumptions: { nominal_return: 6, inflation: 2.5, reserve_months: 6 },
    fire: {
      annual_expense: amount("120,000.00 CNY", 120000),
      balance: amount("2,000,000.00 CNY", 2000000),
      contribution: amount("15,000.00 CNY", 15000),
      real_return: 3.4,
      primary_swr: 4,
      progress: 66.7,
      targets: [
        {
          swr: 4,
          target: amount("3,000,000.00 CNY", 3000000),
          eta_months: 48,
          eta_date: "2030-10-01",
          eta_label: "按当前节奏约 4 年后达成",
          achieved: false,
        },
      ],
      note: null,
    },
    reserve: {
      cash: amount("80,000.00 CNY", 80000),
      monthly_expense: amount("10,000.00 CNY", 10000),
      months: 8,
      target_months: 6,
    },
    pool: [{ account: "assets:invest:fund", class: "equity", amount: amount("1,200,000.00 CNY", 1200000) }],
    goals: [
      {
        label: "退休金",
        account: "assets:invest:retirement",
        target: amount("1,000,000.00 CNY", 1000000),
        current: amount("400,000.00 CNY", 400000),
        progress: 40,
        by: "2040-01-01",
        months_left: 159,
      },
    ],
    debts: [
      {
        label: "房贷",
        account: "liabilities:mortgage",
        balance: amount("1,200,000.00 CNY", 1200000),
        rate: 3.15,
        payment: amount("8,200.00 CNY", 8200),
        monthly_interest: amount("3,150.00 CNY", 3150),
        interest_paid: amount("42,000.00 CNY", 42000),
        payoff_months: 204,
        payoff_date: "2043-06-01",
        note: null,
        extra: [],
      },
    ],
    allocation: {
      rows: [
        {
          class: "equity",
          current: amount("1,200,000.00 CNY", 1200000),
          current_share: 60,
          target_share: 50,
          drift: 10,
        },
      ],
      untagged: [],
    },
    diagnostics: [],
    ...overrides,
  };
}

function wrap(node: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0, staleTime: 0 } } });
  return render(<QueryClientProvider client={client}>{node}</QueryClientProvider>);
}

describe("Plan", () => {
  beforeEach(() => {
    planMock.mockReset();
    saveMock.mockReset();
    saveMock.mockResolvedValue(undefined);
  });

  it("invites you to create a plan when goals.ledger is missing", async () => {
    planMock.mockResolvedValue(overview({ has_goals: false, goals_text: "" }));
    wrap(<Plan />);
    expect(await screen.findByText("还没有规划文件")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "创建规划" }));
    expect(await screen.findByLabelText("计划内容")).toBeInTheDocument();
  });

  it("shows fire progress, reserve months, goals, debts and allocation drift", async () => {
    planMock.mockResolvedValue(overview());
    wrap(<Plan />);
    expect(await screen.findByText("66.7%")).toBeInTheDocument();
    expect(screen.getByText("8.0 个月")).toBeInTheDocument();
    expect(screen.getByText("退休金")).toBeInTheDocument();
    expect(screen.getByText("2043-06-01")).toBeInTheDocument();
    expect(screen.getByText(/偏离 10.0pp/)).toBeInTheDocument();
    expect(screen.getByText(/SWR 4.0%/)).toBeInTheDocument();
  });

  it("saves edits through goals_save", async () => {
    planMock.mockResolvedValue(overview());
    wrap(<Plan />);
    await userEvent.click(await screen.findByRole("button", { name: "编辑计划" }));
    const editor = await screen.findByLabelText("计划内容");
    await userEvent.clear(editor);
    await userEvent.type(editor, "goal fire 自由 ; swr:3.5%");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(1));
    expect(String(saveMock.mock.calls[0][0])).toContain("swr:3.5%");
  });

  it("surfaces goals.ledger diagnostics", async () => {
    planMock.mockResolvedValue(
      overview({
        diagnostics: [
          {
            severity: "warning",
            code: "plan.unknown_directive",
            message: "不知道 pitch 这一行",
            file: "goals.ledger",
            line: 4,
            col: 1,
          },
        ],
      }),
    );
    wrap(<Plan />);
    expect(await screen.findByText(/goals.ledger 有 1 处问题/)).toBeInTheDocument();
    expect(screen.getByText(/第 4 行：不知道 pitch 这一行/)).toBeInTheDocument();
  });
});

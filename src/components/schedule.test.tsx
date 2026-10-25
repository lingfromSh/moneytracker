import * as React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const scheduleOverview = vi.fn();
const scheduleCommit = vi.fn();
vi.mock("../lib/ipc", () => ({
  api: {
    scheduleOverview: (...args: unknown[]) => scheduleOverview(...args),
    scheduleCommit: (...args: unknown[]) => scheduleCommit(...args),
  },
  queryKeys: { schedule: (months: number) => ["schedule", months] },
}));

const amount = (value: number, display: string) => ({
  quantity: value.toFixed(2),
  value,
  commodity: "CNY",
  display,
});

const occurrence = (over: Record<string, unknown>) => ({
  id: "salary",
  label: "工资",
  kind: "salary",
  month: "2026-11",
  date: "2026-11-05",
  recorded: false,
  skipped: false,
  due: true,
  settle: amount(23550, "23,550.00 CNY"),
  income: amount(42300, "42,300.00 CNY"),
  expense: amount(3750, "3,750.00 CNY"),
  assets: amount(38550, "38,550.00 CNY"),
  debt: amount(0, "0.00 CNY"),
  legs: [],
  txn_id: null,
  ...over,
});

const overview = (over: Record<string, unknown> = {}) => ({
  today: "2026-11-20",
  has_schedule: true,
  file: "recurring.ledger",
  templates: [],
  months: ["2026-11"],
  occurrences: [occurrence({})],
  month_totals: [
    {
      month: "2026-11",
      income: amount(42300, "42,300.00 CNY"),
      expense: amount(3750, "3,750.00 CNY"),
      assets: amount(38550, "38,550.00 CNY"),
      debt: amount(0, "0.00 CNY"),
      template_count: 1,
      recorded: 0,
      pending: 1,
    },
  ],
  diagnostics: [],
  src: "",
  auto_commit: false,
  ...over,
});

function wrap(onOpen = () => {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <ScheduleCard onOpen={onOpen} />
    </QueryClientProvider>,
  );
}

let ScheduleCard: React.ComponentType<{ onOpen: () => void }>;
beforeEach(async () => {
  vi.clearAllMocks();
  scheduleOverview.mockResolvedValue(overview());
  scheduleCommit.mockResolvedValue({ written: 1, already: 0, files: [], months: [], errors: [] });
  ScheduleCard = (await import("./schedule")).ScheduleCard;
});

describe("ScheduleCard", () => {
  it("shows the fixed income, expense and leftover of the month", async () => {
    wrap();
    expect(await screen.findByText("26年11月")).toBeInTheDocument();
    expect(screen.getByText("42,300.00 CNY")).toBeInTheDocument();
    expect(screen.getByText("3,750.00 CNY")).toBeInTheDocument();
    // 42300 - 3750
    expect(screen.getByText("38550.00")).toBeInTheDocument();
  });

  it("books one occurrence on demand", async () => {
    wrap();
    await screen.findByText(/11月5日 · 工资/);
    await userEvent.click(screen.getByRole("button", { name: "记入" }));

    await waitFor(() => expect(scheduleCommit).toHaveBeenCalledWith("salary"));
    expect(await screen.findByText("已记入账本")).toBeInTheDocument();
  });

  it("says nothing will be written while automation is off", async () => {
    wrap();
    expect(await screen.findByText("只提醒，不会写入账本")).toBeInTheDocument();
  });

  it("announces automation when the setting is on", async () => {
    scheduleOverview.mockResolvedValue(overview({ auto_commit: true }));
    wrap();
    expect(await screen.findByText(/自动记入已开启/)).toBeInTheDocument();
  });

  it("invites the user to declare a schedule when the file is empty", async () => {
    scheduleOverview.mockResolvedValue(
      overview({ has_schedule: false, occurrences: [], month_totals: [] }),
    );
    const onOpen = vi.fn();
    wrap(onOpen);
    expect(await screen.findByText("还没有固定收支")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /打开日程/ }));
    expect(onOpen).toHaveBeenCalled();
  });

  it("does not offer to book an occurrence twice", async () => {
    scheduleOverview.mockResolvedValue(
      overview({ occurrences: [occurrence({ recorded: true, due: false })] }),
    );
    wrap();
    expect(await screen.findByText("本月没有待记的固定收支。")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "记入" })).not.toBeInTheDocument();
  });
});

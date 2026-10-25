import * as React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const recurList = vi.fn();
const recurCommit = vi.fn();
const recurSkip = vi.fn();
vi.mock("../lib/ipc", () => ({
  api: {
    recurList: (...args: unknown[]) => recurList(...args),
    recurCommit: (...args: unknown[]) => recurCommit(...args),
    recurSkip: (...args: unknown[]) => recurSkip(...args),
  },
  queryKeys: { recur: (months: number) => ["recur", months] },
}));

const group = (id: string, payee: string, due: boolean) => ({
  id,
  payee,
  direction: "expense",
  amount: { quantity: "-3200.00", value: -3200, commodity: "CNY", display: "-3,200.00 CNY" },
  day: 5,
  occurrences: 4,
  months: ["2026-07", "2026-08", "2026-09"],
  last_date: "2026-09-05",
  next_date: "2026-10-05",
  gap_days: 31,
  confidence: 0.8,
  due,
  done_reason: null,
});

function wrap() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <RecurringBanner />
    </QueryClientProvider>,
  );
}

let RecurringBanner: React.ComponentType;
beforeEach(async () => {
  vi.clearAllMocks();
  recurList.mockResolvedValue({
    groups: [group("c-1", "房租", true), group("c-2", "宽带", true), group("c-3", "已记过", false)],
    due_count: 2,
  });
  recurCommit.mockResolvedValue({
    written: 2,
    skipped: 0,
    files: ["2026/2026-10-05.ledger"],
    groups: [],
    errors: [],
  });
  recurSkip.mockResolvedValue(null);
  RecurringBanner = (await import("./recurring")).RecurringBanner;
});

describe("RecurringBanner", () => {
  it("lists only the occurrences that are due", async () => {
    wrap();
    expect(await screen.findByText("发现 2 笔定期交易")).toBeInTheDocument();
    expect(screen.getByText("房租")).toBeInTheDocument();
    expect(screen.queryByText("已记过")).not.toBeInTheDocument();
  });

  it("writes every due occurrence at once", async () => {
    wrap();
    await screen.findByText("发现 2 笔定期交易");
    await userEvent.click(screen.getByRole("button", { name: "全部记入" }));

    await waitFor(() => expect(recurCommit).toHaveBeenCalledWith(["c-1", "c-2"]));
    expect(await screen.findByText(/已记录 2 笔/)).toBeInTheDocument();
  });

  it("can skip one pattern for this month", async () => {
    wrap();
    await screen.findByText("发现 2 笔定期交易");
    await userEvent.click(screen.getAllByText("本月不算")[0]);

    await waitFor(() => expect(recurSkip).toHaveBeenCalledWith("c-1"));
    await waitFor(() => expect(screen.getByText("发现 1 笔定期交易")).toBeInTheDocument());
  });

  it("renders nothing when everything is already recorded", async () => {
    recurList.mockResolvedValue({ groups: [group("c-3", "已记过", false)], due_count: 0 });
    const { container } = wrap();
    await waitFor(() => expect(recurList).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });
});

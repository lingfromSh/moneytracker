import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const llmAsk = vi.fn();
vi.mock("../lib/ipc", () => ({
  api: { llmAsk: (...args: unknown[]) => llmAsk(...args) },
}));

const { AskCard } = await import("./ask-card");

const answer = {
  question: "今年餐饮花了多少",
  plan: {
    metric: "spend",
    accounts: ["expenses:food"],
    tags: [],
    date_from: "2026-01",
    date_to: "2026-11",
    group_by: "month",
    limit: 12,
    min_amount: null,
  },
  result: {
    metric: "spend",
    group_by: "month",
    from: "2026-01",
    to: "2026-11",
    accounts: ["expenses:food"],
    tags: [],
    facts: [
      { label: "2026-09", value: 980, display: "980.00 CNY" },
      { label: "2026-10", value: 1220, display: "1,220.00 CNY" },
    ],
    total: { quantity: "2200.00", value: 2200, commodity: "CNY", display: "2,200.00 CNY" },
    matched: 12,
    summary: "支出合计 2,200.00 CNY，按月份分成 2 组，最高的是 2026-10 1,220.00 CNY",
  },
  answer: "今年餐饮一共 2,200.00 CNY，10 月花得最多。",
  error: null,
};

function wrap() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <AskCard />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  llmAsk.mockResolvedValue(answer);
});

describe("AskCard", () => {
  it("asks the question and shows the computed facts", async () => {
    wrap();
    await userEvent.type(screen.getByLabelText("问题"), "今年餐饮花了多少");
    await userEvent.click(screen.getByRole("button", { name: "提问" }));

    expect(llmAsk).toHaveBeenCalledWith("今年餐饮花了多少");
    expect(await screen.findByText(/一共 2,200.00 CNY/)).toBeInTheDocument();
    expect(screen.getByText("2026-10")).toBeInTheDocument();
    expect(screen.getByText("1,220.00 CNY")).toBeInTheDocument();
    expect(screen.getByText(/共 12 笔/)).toBeInTheDocument();
    expect(screen.getByText("expenses:food")).toBeInTheDocument();
    expect(screen.getByText("按月份")).toBeInTheDocument();
  });

  it("falls back to the engine summary when the model wrote no prose", async () => {
    llmAsk.mockResolvedValue({ ...answer, answer: null });
    wrap();
    await userEvent.click(screen.getByRole("button", { name: "哪个月支出最高" }));
    expect(await screen.findByText(/支出合计 2,200.00 CNY/)).toBeInTheDocument();
  });

  it("shows the structured error when the question cannot be answered", async () => {
    llmAsk.mockRejectedValue(new Error("问答没有启用，先到设置里打开"));
    wrap();
    await userEvent.click(screen.getByRole("button", { name: "哪个月支出最高" }));
    expect(await screen.findByText("问答没有启用，先到设置里打开")).toBeInTheDocument();
  });
});

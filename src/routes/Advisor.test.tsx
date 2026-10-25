import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";

const { advisorLoad, advisorAsk, advisorClear } = vi.hoisted(() => ({
  advisorLoad: vi.fn(),
  advisorAsk: vi.fn(),
  advisorClear: vi.fn(),
}));

vi.mock("../lib/ipc", () => ({
  api: { advisorLoad, advisorAsk, advisorClear },
  queryKeys: { advisor: ["advisor"] },
}));

import { Advisor } from "./Advisor";
import { seedAdvisor } from "../lib/advisor";

function wrap(ui: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
}

beforeEach(() => {
  advisorLoad.mockReset();
  advisorAsk.mockReset();
  advisorClear.mockReset();
  sessionStorage.clear();
  advisorLoad.mockResolvedValue({ turns: [], updated_at: null });
});

describe("Advisor", () => {
  it("starts with example questions instead of an empty box", async () => {
    wrap(<Advisor />);
    expect(await screen.findByText("这个月可以支出 5000 买手机吗？")).toBeInTheDocument();
    expect(screen.getByText(/顾问会先看你的现金/)).toBeInTheDocument();
  });

  it("sends the question with the turns so far and renders the answer", async () => {
    advisorAsk.mockResolvedValue({ content: "可以，花完还剩 3.2 万，储备 18 个月。", tools: [], rounds: 1, context_chars: 1800, error: null });
    wrap(<Advisor />);
    const box = await screen.findByPlaceholderText(/问问账本/);
    await userEvent.type(box, "这个月可以支出 5000 买手机吗？");
    await userEvent.click(screen.getByRole("button", { name: "发送" }));

    expect(await screen.findByText(/花完还剩 3.2 万/)).toBeInTheDocument();
    expect(advisorAsk).toHaveBeenCalledWith("这个月可以支出 5000 买手机吗？", []);
    expect(screen.getByText("这个月可以支出 5000 买手机吗？")).toBeInTheDocument();
  });

  it("shows which tools the advisor used", async () => {
    advisorAsk.mockResolvedValue({
      content: "先看现金流。",
      tools: [{ tool: "cashflow", input: "mon\ns = 6", output: "| 月份 | 收入 | 支出 |" }],
      rounds: 2,
      context_chars: 1800,
      error: null,
    });
    wrap(<Advisor />);
    await userEvent.type(await screen.findByPlaceholderText(/问问账本/), "现金流");
    await userEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByText("这一轮查了 1 次账本")).toBeInTheDocument();
    expect(screen.getByText(/cashflow/)).toBeInTheDocument();
  });

  it("keeps the question visible and reports the failure when the model is not configured", async () => {
    advisorAsk.mockRejectedValue(new Error("问答没有启用，先到设置里打开"));
    wrap(<Advisor />);
    await userEvent.type(await screen.findByPlaceholderText(/问问账本/), "还能花多少");
    await userEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByText("问答没有启用，先到设置里打开")).toBeInTheDocument();
    expect(screen.getByText("还能花多少")).toBeInTheDocument();
  });

  it("carries the 问一句 answer over when the dashboard hands off", async () => {
    seedAdvisor("今年餐饮花了多少", "餐饮合计 2,200.00 CNY");
    wrap(<Advisor />);
    expect(await screen.findByText("今年餐饮花了多少")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("餐饮合计 2,200.00 CNY")).toBeInTheDocument());
    expect(advisorAsk).not.toHaveBeenCalled();
  });

  it("clears the conversation", async () => {
    advisorLoad.mockResolvedValue({
      turns: [{ role: "user", content: "旧问题", at: "2026-10-25T00:00:00Z" }],
      updated_at: "2026-10-25T00:00:00Z",
    });
    advisorClear.mockResolvedValue(undefined);
    wrap(<Advisor />);
    expect(await screen.findByText("旧问题")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /清空对话/ }));
    await waitFor(() => expect(screen.queryByText("旧问题")).not.toBeInTheDocument());
    expect(advisorClear).toHaveBeenCalled();
  });
});

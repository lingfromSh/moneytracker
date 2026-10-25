import * as React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const scheduleOverview = vi.fn();
const scheduleCommit = vi.fn();
const scheduleCommitDue = vi.fn();
const scheduleSkip = vi.fn();
const scheduleSave = vi.fn();
const scheduleSalaryPreview = vi.fn();

vi.mock("../lib/ipc", () => ({
  api: {
    scheduleOverview: (...args: unknown[]) => scheduleOverview(...args),
    scheduleCommit: (...args: unknown[]) => scheduleCommit(...args),
    scheduleCommitDue: (...args: unknown[]) => scheduleCommitDue(...args),
    scheduleSkip: (...args: unknown[]) => scheduleSkip(...args),
    scheduleSave: (...args: unknown[]) => scheduleSave(...args),
    scheduleSalaryPreview: (...args: unknown[]) => scheduleSalaryPreview(...args),
  },
  queryKeys: { schedule: (months: number) => ["schedule", months] },
}));

const amount = (value: number, display: string) => ({
  quantity: value.toFixed(2),
  value,
  commodity: "CNY",
  display,
});

const salaryTemplate = (over: Record<string, unknown> = {}) => ({
  id: "salary-main",
  label: "工资",
  kind: "salary",
  period: "monthly from 2026-11-15",
  cadence: "monthly",
  day: 15,
  start: "2026-11-15",
  until: null,
  enabled: true,
  settle: amount(29452.5, "24,750.00 CNY"),
  income: amount(39900, "39,900.00 CNY"),
  expense: amount(4462.5, "3,750.00 CNY"),
  assets: amount(43018.5, "43,018.50 CNY"),
  debt: amount(0, "0.00 CNY"),
  next_date: "2026-11-15",
  tax_account: "expenses:个税",
  deduct: amount(1000, "1,000.00 CNY"),
  tax: amount(562.50, "562.50 CNY"),
  line: 8,
  legs: [],
  ...over,
});

const occurrence = (over: Record<string, unknown> = {}) => ({
  id: "salary-main",
  label: "工资",
  kind: "salary",
  month: "2026-11",
  date: "2026-11-15",
  recorded: false,
  skipped: false,
  due: true,
  settle: amount(29452.5, "24,750.00 CNY"),
  income: amount(39900, "39,900.00 CNY"),
  expense: amount(4462.5, "3,750.00 CNY"),
  assets: amount(43018.5, "43,018.50 CNY"),
  debt: amount(0, "0.00 CNY"),
  tax_base: amount(23452.5, "18,750.00 CNY"),
  tax: amount(562.50, "562.50 CNY"),
  legs: [],
  txn_id: null,
  ...over,
});

const overview = (over: Record<string, unknown> = {}) => ({
  today: "2026-11-20",
  has_schedule: true,
  file: "recurring.ledger",
  templates: [salaryTemplate()],
  months: ["2026-11"],
  occurrences: [occurrence()],
  month_totals: [
    {
      month: "2026-11",
      income: amount(39900, "39,900.00 CNY"),
      expense: amount(4462.5, "3,750.00 CNY"),
      assets: amount(43018.5, "43,018.50 CNY"),
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

let Schedule: React.ComponentType;
beforeEach(async () => {
  vi.clearAllMocks();
  scheduleOverview.mockResolvedValue(overview());
  scheduleCommit.mockResolvedValue({ written: 1, already: 0, files: [], months: [], errors: [] });
  Schedule = (await import("./Schedule")).Schedule;
});

function wrap() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <Schedule />
    </QueryClientProvider>,
  );
}

describe("Schedule page", () => {
  it("asks for the tax before booking a salary", async () => {
    wrap();
    const booking = await screen.findAllByRole("button", { name: "记入" });
    await userEvent.click(booking[0]!);

    const title = await screen.findByText("记入 工资 · 2026-11");
    const dialog = title.closest(".fixed") as HTMLElement;
    expect(dialog).not.toBeNull();
    expect(screen.getByText("18,750.00")).toBeInTheDocument();
    expect(screen.getByLabelText(/本月个税/)).toBeDefined();
    expect(screen.queryByText("28,748.92")).not.toBeNull();

    await userEvent.click(within(dialog).getByRole("button", { name: "记入" }));
    await waitFor(() =>
      expect(scheduleCommit).toHaveBeenCalledWith("salary-main", "2026-11", 562.50),
    );
  });

  it("lets the user override the estimated tax", async () => {
    wrap();
    const booking = await screen.findAllByRole("button", { name: "记入" });
    await userEvent.click(booking[0]!);

    const input = await screen.findByLabelText(/本月个税/);
    await userEvent.clear(input);
    await userEvent.type(input, "562.50");
    expect(screen.getByText("24,187.50")).toBeInTheDocument();

    const dialog = screen.getByText("记入 工资 · 2026-11").closest(".fixed") as HTMLElement;
    await userEvent.click(within(dialog).getByRole("button", { name: "记入" }));
    await waitFor(() =>
      expect(scheduleCommit).toHaveBeenCalledWith("salary-main", "2026-11", 562.50),
    );
  });

  it("books a tax-free template without asking anything", async () => {
    scheduleOverview.mockResolvedValue(
      overview({
        templates: [salaryTemplate({ id: "loan-home", label: "房贷", kind: "loan", tax_account: null })],
        occurrences: [occurrence({ id: "loan-home", label: "房贷", kind: "loan", tax: amount(0, "0.00 CNY") })],
      }),
    );
    wrap();
    const booking = await screen.findAllByRole("button", { name: "记入" });
    await userEvent.click(booking[0]!);

    await waitFor(() =>
      expect(scheduleCommit).toHaveBeenCalledWith("loan-home", "2026-11", null),
    );
    expect(screen.queryByText(/记入 房贷 · 2026-11/)).not.toBeInTheDocument();
  });

  it("shows the tax column of the templates and lets the salary modal set deductions", async () => {
    wrap();
    await screen.findAllByRole("button", { name: "记入" });
    await userEvent.click(screen.getByRole("button", { name: /工资模板/ }));
    const input = await screen.findByLabelText(/专项附加扣除/);
    expect(input).toHaveValue("1000.00");
  });
});

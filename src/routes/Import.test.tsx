import * as React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const openMock = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...args: unknown[]) => openMock(...args) }));

const importProbe = vi.fn();
const importPreview = vi.fn();
const importCommit = vi.fn();
vi.mock("../lib/ipc", () => ({
  api: {
    importProbe: (...args: unknown[]) => importProbe(...args),
    importPreview: (...args: unknown[]) => importPreview(...args),
    importCommit: (...args: unknown[]) => importCommit(...args),
  },
  queryKeys: { snapshot: ["snapshot"] },
}));

const probe = {
  path: "/tmp/招行流水.csv",
  name: "招行流水.csv",
  encoding: "UTF-8",
  delimiter: ",",
  header_row: true,
  headers: ["交易日期", "交易摘要", "收入", "支出"],
  sample: [],
  row_count: 3,
  guess: {
    date_col: "交易日期",
    payee_col: "交易摘要",
    memo_col: null,
    amount_col: null,
    outflow_col: "支出",
    inflow_col: "收入",
    account: "assets:bank:cmb",
    preset: "招商银行",
  },
  presets: ["支付宝", "微信支付", "招商银行"],
};

const row = (line: number, payee: string, duplicate = false) => ({
  line,
  date: "2026-10-0" + line,
  payee,
  memo: null,
  amount: {
    quantity: "-12.50",
    value: -12.5,
    commodity: "CNY",
    display: "-12.50 CNY",
  },
  fingerprint: "fp" + line,
  duplicate,
  category: null,
  tags: [],
  warning: null,
  selected: true,
});

const previewDto = {
  path: probe.path,
  name: probe.name,
  batch_id: "batch-1",
  account: "assets:bank:cmb",
  commodity: "CNY",
  delimiter: ",",
  encoding: "UTF-8",
  headers: probe.headers,
  rows: [row(1, "星巴克"), row(2, "滴滴", true)],
  duplicates: 1,
  total: { quantity: "-25.00", value: -25, commodity: "CNY", display: "-25.00 CNY" },
  errors: [],
  warnings: [],
};

function wrap() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <Import />
    </QueryClientProvider>,
  );
}

let Import: React.ComponentType;
beforeEach(async () => {
  vi.clearAllMocks();
  openMock.mockResolvedValue(probe.path);
  importProbe.mockResolvedValue(probe);
  importPreview.mockResolvedValue(previewDto);
  importCommit.mockResolvedValue({
    batch_id: "batch-1",
    written: 1,
    skipped: 1,
    files: ["2026/2026-10-01.ledger"],
    diagnostics: [],
  });
  Import = (await import("./Import")).Import;
});

describe("Import", () => {
  it("probes a picked file and pre-fills the guess", async () => {
    wrap();
    await userEvent.click(screen.getByRole("button", { name: /选择 CSV 文件/ }));

    await waitFor(() => expect(importProbe).toHaveBeenCalledWith(probe.path));
    expect(await screen.findByText("招行流水.csv")).toBeInTheDocument();
    expect(screen.getByLabelText("账号")).toHaveValue("assets:bank:cmb");
    expect(screen.getByText("招商银行")).toBeInTheDocument();
  });

  it("previews rows, marks duplicates and commits the selected lines", async () => {
    wrap();
    await userEvent.click(screen.getByRole("button", { name: /选择 CSV 文件/ }));
    await screen.findByText("招行流水.csv");

    await userEvent.click(screen.getByRole("button", { name: "预览" }));
    await waitFor(() => expect(importPreview).toHaveBeenCalled());
    expect(await screen.findByText("星巴克")).toBeInTheDocument();
    expect(screen.getByText("重复")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("checkbox", { name: "选择第 2 行" }));
    await userEvent.click(screen.getByRole("button", { name: /导入 1 笔/ }));

    await waitFor(() => expect(importCommit).toHaveBeenCalled());
    const profile = importCommit.mock.calls[0][0] as { include_lines: number[] };
    expect(profile.include_lines).toEqual([1]);
    expect(await screen.findByText(/写入 1 笔，跳过 1 笔/)).toBeInTheDocument();
  });

  it("turns rule lines into category mappings", async () => {
    wrap();
    await userEvent.click(screen.getByRole("button", { name: /选择 CSV 文件/ }));
    await screen.findByText("招行流水.csv");

    await userEvent.type(
      screen.getByLabelText("分类规则"),
      "星巴克 => expenses:food:coffee :coffee",
    );
    await userEvent.click(screen.getByRole("button", { name: "预览" }));

    await waitFor(() => expect(importPreview).toHaveBeenCalled());
    const profile = importPreview.mock.calls[0][0] as {
      mapping: { pattern: string; category: string; tags: string[] }[];
      account: string;
    };
    expect(profile.mapping).toEqual([
      { pattern: "星巴克", category: "expenses:food:coffee", tags: ["coffee"] },
    ]);
    expect(profile.account).toBe("assets:bank:cmb");
  });
});

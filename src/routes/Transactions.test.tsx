import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TxnDto_Serialize } from "../lib/bindings";

const queryMock = vi.fn();
const updateMock = vi.fn();
const deleteMock = vi.fn();
const bulkMock = vi.fn();
const settingsMock = vi.fn();

vi.mock("../lib/ipc", () => ({
  api: {
    transactions: (req: unknown) => queryMock(req),
    txnUpdate: (id: string, patch: unknown) => updateMock(id, patch),
    txnDelete: (id: string) => deleteMock(id),
    txnBulkUpdate: (ids: string[], patch: unknown) => bulkMock(ids, patch),
    showCapture: vi.fn(),
    settings: () => settingsMock(),
  },
  queryKeys: {
    transactions: (req: unknown) => ["transactions", req],
    settings: ["settings"],
  },
}));

const { Transactions } = await import("./Transactions");

function txn(overrides: Partial<TxnDto_Serialize> = {}): TxnDto_Serialize {
  return {
    id: "01JA3KQ7",
    date: "2026-10-24",
    date2: null,
    status: null,
    code: null,
    payee: "星巴克",
    comment: null,
    tags: [{ key: "coffee", value: null }],
    postings: [
      {
        account: "expenses:food:coffee",
        amount: null,
        commodity: "CNY",
        quantity: "45.00",
        display: "45.00 CNY",
        price: null,
        price_kind: null,
        assertion: null,
        comment: null,
        virtual_kind: "real",
        tags: [],
      },
      {
        account: "assets:微信",
        amount: null,
        commodity: "CNY",
        quantity: "-45.00",
        display: "-45.00 CNY",
        price: null,
        price_kind: null,
        assertion: null,
        comment: null,
        virtual_kind: "real",
        tags: [],
      },
    ],
    file: "2026/2026-10-24.ledger",
    span_start: 0,
    span_end: 120,
    base_value: 45,
    base_value_display: "45.00 CNY",
    balanced: true,
    fx_missing: false,
    editable: true,
    ...overrides,
  };
}

const page = {
  items: [
    txn(),
    txn({ id: "01JA3KQ8", payee: "地铁", base_value: 6, base_value_display: "6.00 CNY" }),
    txn({
      id: "01JA3KQ9",
      date: "2026-10-23",
      payee: "超市",
      base_value: 128,
      base_value_display: "128.00 CNY",
      postings: [
        {
          account: "expenses:unclassified",
          amount: null,
          commodity: "CNY",
          quantity: "128.00",
          display: "128.00 CNY",
          price: null,
          price_kind: null,
          assertion: null,
          comment: null,
          virtual_kind: "real",
          tags: [],
        },
      ],
    }),
  ],
  total: 3,
  offset: 0,
  limit: 200,
};

function renderList() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <Transactions />
    </QueryClientProvider>,
  );
}

describe("Transactions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    queryMock.mockResolvedValue(page);
    updateMock.mockResolvedValue(txn({ payee: "星巴克臻选" }));
    deleteMock.mockResolvedValue(null);
    bulkMock.mockResolvedValue(2);
    settingsMock.mockResolvedValue({ shortcut: "CmdOrCtrl+Shift+Space" });
  });

  it("groups transactions by day and renders amounts", async () => {
    renderList();
    expect(await screen.findByText("星巴克")).toBeInTheDocument();
    expect(screen.getByText("地铁")).toBeInTheDocument();
    expect(screen.getByText("超市")).toBeInTheDocument();
    expect(screen.getAllByText("10月24日").length).toBeGreaterThan(0);
    expect(screen.getByText("10月23日")).toBeInTheDocument();
    expect(screen.getByText("45.00 CNY")).toBeInTheDocument();
    expect(screen.getByText("3 笔 · 显示 1–3")).toBeInTheDocument();
  });

  it("shows the configured capture shortcut in the empty state", async () => {
    settingsMock.mockResolvedValue({ shortcut: "CmdOrCtrl+Alt+K" });
    queryMock.mockResolvedValue({ items: [], total: 0, offset: 0, limit: 200 });
    renderList();
    expect(await screen.findByText("按 ⌘ ⌥ K 记第一笔")).toBeInTheDocument();
  });

  it("marks inbox transactions", async () => {
    renderList();
    expect(await screen.findByText("Inbox")).toBeInTheDocument();
  });

  it("sends exactly one txn_update when saving an edit", async () => {
    renderList();
    await userEvent.click(await screen.findByText("星巴克"));

    const payee = await screen.findByLabelText("描述");
    await userEvent.clear(payee);
    await userEvent.type(payee, "星巴克臻选");
    await userEvent.click(screen.getByText("保存"));

    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    const [id, patch] = updateMock.mock.calls[0];
    expect(id).toBe("01JA3KQ7");
    expect((patch as { payee: string }).payee).toBe("星巴克臻选");
  });

  it("never exposes the engine-owned txn tag to the editor", async () => {
    queryMock.mockResolvedValue({
      ...page,
      items: [txn({ tags: [{ key: "txn", value: "01JA3KQ7" }, { key: "coffee", value: null }] })],
      total: 1,
    });
    renderList();
    await userEvent.click(await screen.findByText("星巴克"));

    const tags = (await screen.findByLabelText(/标签/)) as HTMLInputElement;
    expect(tags.value).toBe("coffee");

    await userEvent.click(screen.getByText("保存"));
    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    const [, patch] = updateMock.mock.calls[0];
    const sent = (patch as { tags: { key: string }[] }).tags;
    expect(sent.map((t) => t.key)).toEqual(["coffee"]);
  });

  it("deletes through txn_delete", async () => {
    renderList();
    await userEvent.click(await screen.findByText("地铁"));
    await userEvent.click(await screen.findByText("删除"));
    await waitFor(() => expect(deleteMock).toHaveBeenCalledWith("01JA3KQ8"));
  });

  it("applies bulk tag changes to the selection", async () => {
    renderList();
    const boxes = await screen.findAllByLabelText(/^选择 /);
    await userEvent.click(boxes[0]);
    await userEvent.click(boxes[1]);
    expect(screen.getByText("已选 2 笔")).toBeInTheDocument();

    await userEvent.click(screen.getByText("批量修改"));
    await userEvent.type(screen.getByLabelText(/添加标签/), "报销");
    await userEvent.click(screen.getByText("应用"));

    await waitFor(() => expect(bulkMock).toHaveBeenCalledTimes(1));
    const [ids, patch] = bulkMock.mock.calls[0];
    expect(ids).toEqual(["01JA3KQ7", "01JA3KQ8"]);
    expect((patch as { add_tags: string[] }).add_tags).toEqual(["报销"]);
  });
});

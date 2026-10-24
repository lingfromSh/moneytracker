import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CaptureParseDto_Serialize, SnapshotDto_Serialize } from "../lib/bindings";

const parseMock = vi.fn();
const commitMock = vi.fn();
const ruleMock = vi.fn();
const snapshotMock = vi.fn();
const hideMock = vi.fn();
const focusMainMock = vi.fn();

vi.mock("../lib/ipc", () => ({
  api: {
    captureParse: (text: string) => parseMock(text),
    captureCommit: (text: string) => commitMock(text),
    ruleUpsert: (text: string) => ruleMock(text),
    snapshot: () => snapshotMock(),
  },
  queryKeys: { snapshot: ["snapshot"] },
}));

vi.mock("../lib/window", () => ({
  hideCaptureWindow: () => hideMock(),
  focusMain: (search?: string) => focusMainMock(search),
}));

const { CaptureWindow } = await import("./Capture");

function preview(overrides: Partial<CaptureParseDto_Serialize> = {}): CaptureParseDto_Serialize {
  return {
    kind: "transaction",
    canonical_text: "",
    postings: [
      {
        account: "expenses:餐饮",
        amount: null,
        commodity: "CNY",
        quantity: "4.50",
        display: "4.50 CNY",
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
        quantity: "-4.50",
        display: "-4.50 CNY",
        price: null,
        price_kind: null,
        assertion: null,
        comment: null,
        virtual_kind: "real",
        tags: [],
      },
    ],
    warnings: [],
    errors: [],
    rule_id: null,
    rule_preview: null,
    summary: "expenses:餐饮  →  assets:微信",
    date: "2026-10-25",
    payee: "咖啡",
    amount_display: "4.50 CNY",
    ...overrides,
  };
}

function renderCapture() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <CaptureWindow />
    </QueryClientProvider>,
  );
}

describe("CaptureWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    snapshotMock.mockResolvedValue({ accounts: [] } as unknown as SnapshotDto_Serialize);
    commitMock.mockResolvedValue({ file: "2026/2026-10-25.ledger" });
  });

  it("previews the expanded postings while typing", async () => {
    parseMock.mockResolvedValue(preview());
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "咖啡 4.5 @微信");

    await waitFor(() => expect(parseMock).toHaveBeenCalledWith("咖啡 4.5 @微信"));
    expect(await screen.findByText("expenses:餐饮")).toBeInTheDocument();
    expect(screen.getByText("4.50 CNY")).toBeInTheDocument();
  });

  it("commits on Enter and keeps the window open", async () => {
    parseMock.mockResolvedValue(preview());
    renderCapture();

    const input = screen.getByLabelText("快录输入");
    await userEvent.type(input, "咖啡 4.5 @微信");
    await waitFor(() => expect(parseMock).toHaveBeenCalled());
    await userEvent.keyboard("{Enter}");

    await waitFor(() => expect(commitMock).toHaveBeenCalledWith("咖啡 4.5 @微信"));
    expect(hideMock).not.toHaveBeenCalled();
    expect(input).toHaveValue("");
  });

  it("hides the window on Cmd+Enter", async () => {
    parseMock.mockResolvedValue(preview());
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "咖啡 4.5 @微信");
    await waitFor(() => expect(parseMock).toHaveBeenCalled());
    await userEvent.keyboard("{Meta>}{Enter}{/Meta}");

    await waitFor(() => expect(hideMock).toHaveBeenCalled());
  });

  it("shows parser errors instead of a preview", async () => {
    parseMock.mockResolvedValue(
      preview({
        postings: [],
        errors: [
          {
            severity: "error",
            code: "core.parse.bad_amount",
            message: "金额不合法",
            file: "<capture>",
            line: 1,
            col: 5,
          },
        ],
      }),
    );
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "咖啡 ??");
    expect(await screen.findByText("金额不合法")).toBeInTheDocument();
  });

  it("saves rules through rule_upsert", async () => {
    parseMock.mockResolvedValue(preview({ kind: "rule", postings: [] }));
    ruleMock.mockResolvedValue({ id: "r-abc123" });
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "= desc:星巴克 -> 餐饮=100%");
    await waitFor(() => expect(parseMock).toHaveBeenCalled());
    await userEvent.keyboard("{Enter}");

    await waitFor(() => expect(ruleMock).toHaveBeenCalled());
    expect(commitMock).not.toHaveBeenCalled();
    expect(await screen.findByText("规则 r-abc123 已保存")).toBeInTheDocument();
  });

  it("sends / searches to the main window", async () => {
    parseMock.mockResolvedValue(preview({ kind: "search", postings: [] }));
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "/acct:餐饮");
    await waitFor(() => expect(parseMock).toHaveBeenCalled());
    await userEvent.keyboard("{Enter}");

    await waitFor(() => expect(focusMainMock).toHaveBeenCalledWith("acct:餐饮"));
  });
});
